use std::alloc::{GlobalAlloc, Layout};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use fah_certs::{CaParams, CertStore};
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_config::{HttpsConfig, HttpsListenConfig, NoSni};
use fah_model::Event;
use fah_rules::interception::{Active, InterceptionState};
use fah_rules::{Matcher, MatcherBuilder};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderValue, ACCEPT, HOST};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use mimalloc::MiMalloc;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::RootCertStore;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use fah_http::{Interception, TlsProxy, TlsServer};
use fah_model::InterceptionDocument;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: every method forwards to `MiMalloc`, a sound `GlobalAlloc`; the counters never touch the returned memory nor alter the layout.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: `layout` satisfies the trait contract at the call site and is forwarded verbatim.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come from a prior `alloc` with the same layout, as the trait requires.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        // SAFETY: `ptr`/`layout`/`new_size` satisfy the trait contract at the call site.
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const ORIGIN_NAME: &str = "origin.test";

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

struct FixedRules(Arc<Matcher>);

impl fah_http::Ruleset for FixedRules {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

fn rules_with(lines: &str) -> Arc<dyn fah_http::Ruleset> {
    let parsed = fah_rules::parse_rule_list(lines);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("test-list", &parsed);
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

struct OriginCa {
    root: CertificateDer<'static>,
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}

fn origin_ca() -> OriginCa {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    OriginCa {
        root: cert.der().clone(),
        issuer: rcgen::Issuer::new(params, key),
    }
}

fn leaf_signed_by(ca: &OriginCa) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec![ORIGIN_NAME.to_string()]).unwrap();
    let cert = params.signed_by(&key, &ca.issuer).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::try_from(key.serialize_der()).unwrap(),
    )
}

async fn tls_origin(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> SocketAddr {
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let service = service_fn(|_request: Request<Incoming>| async {
                    Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from_static(
                        b"ORIGIN PAYLOAD",
                    ))))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(tls), service)
                    .await;
            });
        }
    });
    addr
}

fn client_trusting(root: CertificateDer<'static>) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.add(root).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    TlsConnector::from(Arc::new(config))
}

struct Case {
    label: &'static str,
    path: &'static str,
    accept: &'static str,
    ceiling_per_request: usize,
}

fn request(case: &Case) -> Request<Full<Bytes>> {
    Request::builder()
        .uri(case.path)
        .header(HOST, ORIGIN_NAME)
        .header(ACCEPT, HeaderValue::from_static(case.accept))
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[test]
fn warm_intercepted_requests_allocate_a_steady_amount() {
    const REQUESTS: usize = 64;
    const BATCHES: usize = 4;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    fah_certs::install_crypto_provider();
    let upstream_ca = origin_ca();
    let (leaf, key) = leaf_signed_by(&upstream_ca);
    let origin_addr = rt.block_on(tls_origin(leaf, key));

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CertStore::open(dir.path()).unwrap());
    store.generate_ca(&CaParams::default()).unwrap();
    let fah_root = CertificateDer::from(store.ca_public_der().unwrap());

    let mut roots = RootCertStore::empty();
    roots.add(upstream_ca.root.clone()).unwrap();
    let state = Arc::new(InterceptionState::new(
        Active::compile(InterceptionDocument {
            clients: vec!["127.0.0.1".to_string()],
            exclude_domains: Vec::new(),
        })
        .unwrap(),
    ));
    let interception = Interception::new(
        fah_http::server_config(Arc::clone(&store)).unwrap(),
        fah_http::client_config_with_roots(roots).unwrap(),
        Arc::clone(&store),
        state,
    );

    let (events, mut receiver) = mpsc::channel::<Event>(4096);
    let policy = DestinationPolicy::new(
        origin_addr.port(),
        vec![AllowedNet::host(IpAddr::V4(Ipv4Addr::LOCALHOST))],
    );
    let proxy = Arc::new(
        TlsProxy::new(
            Arc::new(FixedResolver(vec![origin_addr.ip()])),
            policy,
            origin_addr.port(),
            Duration::from_secs(120),
            Duration::from_secs(120),
            NoSni::Pass,
        )
        .with_interception(interception)
        .with_rules(rules_with("/ad-banner\n"))
        .with_events(events),
    );

    let config = HttpsConfig {
        listen: HttpsListenConfig {
            address: Ipv4Addr::LOCALHOST.to_string(),
            port: 0,
        },
        ..HttpsConfig::default()
    };
    let mut server = rt.block_on(TlsServer::bind(&config)).unwrap();
    let proxy_addr = server.local_addr();
    rt.block_on(async { server.serve(proxy) });

    let connector = client_trusting(fah_root);
    let mut sender = rt.block_on(async {
        let stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.set_nodelay(true).unwrap();
        let tls = connector
            .connect(ServerName::try_from(ORIGIN_NAME).unwrap(), stream)
            .await
            .expect("the client trusts the FAH CA and gets a minted leaf");
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        sender
    });

    let cases = [
        Case {
            label: "intercepted pass-through GET",
            path: "/resource",
            accept: "*/*",
            ceiling_per_request: 50,
        },
        Case {
            label: "intercepted blocked script",
            path: "/ad-banner.js",
            accept: "*/*",
            ceiling_per_request: 25,
        },
        Case {
            label: "intercepted blocked document",
            path: "/ad-banner",
            accept: "text/html",
            ceiling_per_request: 38,
        },
    ];

    for case in &cases {
        let mut one = || {
            let response = rt.block_on(sender.send_request(request(case))).unwrap();
            let _ = rt.block_on(response.into_body().collect()).unwrap();
            while receiver.try_recv().is_ok() {}
        };
        for _ in 0..REQUESTS {
            one();
        }

        let mut measured = Vec::new();
        for _ in 0..BATCHES {
            let allocations = ALLOCATIONS.load(Ordering::Relaxed);
            let bytes = BYTES.load(Ordering::Relaxed);
            for _ in 0..REQUESTS {
                one();
            }
            measured.push((
                ALLOCATIONS.load(Ordering::Relaxed) - allocations,
                BYTES.load(Ordering::Relaxed) - bytes,
            ));
        }

        println!(
            "intercept/allocations over {REQUESTS} warm requests ({}): {:?}",
            case.label, measured
        );

        let last = measured[BATCHES - 1].0;
        assert_eq!(
            last,
            measured[BATCHES - 2].0,
            "{REQUESTS} warm requests ({}) allocated {} then {} in the last two batches; the \
             intercepted path must reach a steady amount, not keep growing: {measured:?}",
            case.label,
            measured[BATCHES - 2].0,
            last
        );
        assert!(
            last <= REQUESTS * case.ceiling_per_request,
            "{REQUESTS} warm requests ({}) allocated {last}; the ceiling is {} per request",
            case.label,
            case.ceiling_per_request
        );
    }
}
