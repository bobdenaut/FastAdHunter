use std::convert::Infallible;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use fah_certs::{CaParams, CertStore};
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_config::{HttpsConfig, HttpsListenConfig, NoSni};
use fah_http::{Interception, ProxyCounters, TlsProxy, TlsServer};
use fah_model::Event;
use fah_model::InterceptionDocument;
use fah_rules::interception::{Active, InterceptionState};
use fah_rules::{Matcher, MatcherBuilder};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::{http1, http2};
use hyper::header::{CONNECTION, HOST};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::RootCertStore;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio_rustls::{TlsAcceptor, TlsConnector};

const ORIGIN_NAME: &str = "origin.test";
const SMALL_BODY: usize = 1024;
const BIG_BODY: usize = 8 * 1024 * 1024;
const BIG_PATH: &str = "/big";

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn h1() -> Vec<Vec<u8>> {
    vec![b"http/1.1".to_vec()]
}

fn h2() -> Vec<Vec<u8>> {
    vec![b"h2".to_vec()]
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

async fn tls_origin(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    alpn: Vec<Vec<u8>>,
) -> SocketAddr {
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    config.alpn_protocols = alpn;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let small = Bytes::from(vec![b's'; SMALL_BODY]);
    let big = Bytes::from(vec![b'b'; BIG_BODY]);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let small = small.clone();
            let big = big.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let service = service_fn(move |request: Request<Incoming>| {
                    let body = if request.uri().path() == BIG_PATH {
                        big.clone()
                    } else {
                        small.clone()
                    };
                    async move { Ok::<_, Infallible>(Response::new(Full::new(body))) }
                });
                let _ = auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), service)
                    .await;
            });
        }
    });
    addr
}

fn store_with_ca() -> (tempfile::TempDir, Arc<CertStore>) {
    fah_certs::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CertStore::open(dir.path()).unwrap());
    store.generate_ca(&CaParams::default()).unwrap();
    (dir, store)
}

fn interception(store: &Arc<CertStore>, upstream_root: &CertificateDer<'static>) -> Interception {
    let mut roots = RootCertStore::empty();
    roots.add(upstream_root.clone()).unwrap();
    Interception::new(
        fah_http::server_config(Arc::clone(store)).unwrap(),
        fah_http::client_config_with_roots(roots).unwrap(),
        Arc::clone(store),
        Arc::new(InterceptionState::new(
            Active::compile(InterceptionDocument {
                clients: vec!["127.0.0.1".to_string()],
                exclude_domains: Vec::new(),
            })
            .unwrap(),
        )),
    )
}

const BENCH_RULES: &str = "||ads.example^\n\
    ||tracker.example^\n\
    ||metrics.example^$script\n\
    /track.js\n\
    /banner.\n\
    ||cdn.example/analytics/\n";

struct FixedRules(Arc<Matcher>);

impl fah_http::Ruleset for FixedRules {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

fn rules_with(lines: &str) -> Arc<dyn fah_http::Ruleset> {
    let parsed = fah_rules::parse_rule_list(lines);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("bench-list", &parsed);
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn drained_events() -> tokio::sync::mpsc::Sender<Event> {
    let (events, mut drain) = tokio::sync::mpsc::channel(1024);
    tokio::spawn(async move { while drain.recv().await.is_some() {} });
    events
}

async fn tls_server(
    origin: SocketAddr,
    interception: Option<Interception>,
) -> (SocketAddr, Arc<ProxyCounters>) {
    let mut proxy = TlsProxy::new(
        Arc::new(FixedResolver(vec![origin.ip()])),
        DestinationPolicy::new(
            origin.port(),
            vec![AllowedNet::host(IpAddr::V4(Ipv4Addr::LOCALHOST))],
        ),
        origin.port(),
        Duration::from_secs(120),
        Duration::from_secs(120),
        NoSni::Pass,
    )
    .with_rules(rules_with(BENCH_RULES))
    .with_events(drained_events());
    if let Some(interception) = interception {
        proxy = proxy.with_interception(interception);
    }
    let counters = proxy.counters();
    let config = HttpsConfig {
        listen: HttpsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        ..HttpsConfig::default()
    };
    let mut server = TlsServer::bind(&config).await.unwrap();
    let addr = server.local_addr();
    server.serve(Arc::new(proxy));
    (addr, counters)
}

fn report_verdict_path(label: &str, counters: &ProxyCounters) {
    let stats = counters.snapshot();
    println!(
        "{label} verdict path: connections={} requests={} blocked={} refused_claim={} \
         upstream_cert_failures={} dropped_events={}",
        stats.connections,
        stats.requests,
        stats.blocked,
        stats.refused_claim,
        stats.upstream_cert_failures,
        stats.dropped_events
    );
}

fn connector(root: &CertificateDer<'static>, alpn: Vec<Vec<u8>>) -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.add(root.clone()).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = alpn;
    config.resumption = rustls::client::Resumption::disabled();
    TlsConnector::from(Arc::new(config))
}

async fn tls_to(
    addr: SocketAddr,
    connector: &TlsConnector,
) -> tokio_rustls::client::TlsStream<TcpStream> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    tcp.set_nodelay(true).unwrap();
    connector
        .connect(ServerName::try_from(ORIGIN_NAME).unwrap(), tcp)
        .await
        .unwrap()
}

async fn fetch_h1_once(addr: SocketAddr, connector: &TlsConnector, path: &str) -> usize {
    let tls = tls_to(addr, connector).await;
    let (mut sender, connection) = http1::handshake(TokioIo::new(tls)).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri(path)
        .header(HOST, ORIGIN_NAME)
        .header(CONNECTION, "close")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    drain_body(response.into_body()).await
}

async fn h2_session(addr: SocketAddr, connector: &TlsConnector) -> http2::SendRequest<Full<Bytes>> {
    let tls = tls_to(addr, connector).await;
    let (sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
}

async fn h2_get(sender: &mut http2::SendRequest<Full<Bytes>>, path: &str) -> usize {
    let request = Request::builder()
        .uri(format!("https://{ORIGIN_NAME}{path}"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    drain_body(response.into_body()).await
}

async fn drain_body(mut body: Incoming) -> usize {
    let mut received = 0usize;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            received += data.len();
        }
    }
    received
}

struct Rig {
    origin: SocketAddr,
    spliced: SocketAddr,
    intercepted: SocketAddr,
    spliced_counters: Arc<ProxyCounters>,
    intercepted_counters: Arc<ProxyCounters>,
    to_origin: TlsConnector,
    to_fah: TlsConnector,
    _dir: tempfile::TempDir,
    store: Arc<CertStore>,
}

impl Rig {
    fn report_verdict_paths(&self, group: &str) {
        report_verdict_path(&format!("{group}/spliced"), &self.spliced_counters);
        report_verdict_path(&format!("{group}/intercepted"), &self.intercepted_counters);
    }
}

fn rig(rt: &Runtime, alpn: fn() -> Vec<Vec<u8>>) -> Rig {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = rt.block_on(tls_origin(cert, key, alpn()));
    let (dir, store) = store_with_ca();
    let fah_root = CertificateDer::from(store.ca_public_der().unwrap());
    let (spliced, spliced_counters) = rt.block_on(tls_server(origin, None));
    let (intercepted, intercepted_counters) =
        rt.block_on(tls_server(origin, Some(interception(&store, &ca.root))));
    Rig {
        origin,
        spliced,
        intercepted,
        spliced_counters,
        intercepted_counters,
        to_origin: connector(&ca.root, alpn()),
        to_fah: connector(&fah_root, alpn()),
        _dir: dir,
        store,
    }
}

fn handshake(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let rig = rig(&rt, h1);

    let mut group = c.benchmark_group("https_handshake");
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    group.bench_function("direct_to_origin", |b| {
        b.iter(|| rt.block_on(fetch_h1_once(rig.origin, &rig.to_origin, "/")));
    });
    group.bench_function("spliced", |b| {
        b.iter(|| rt.block_on(fetch_h1_once(rig.spliced, &rig.to_origin, "/")));
    });
    group.bench_function("intercepted", |b| {
        b.iter(|| rt.block_on(fetch_h1_once(rig.intercepted, &rig.to_fah, "/")));
    });
    group.finish();

    let stats = rig.store.leaf_cache_stats();
    println!(
        "https_handshake leaf cache after the run: minted_total={} prewarm_hits={} unwarmed_misses={}",
        stats.minted_total, stats.prewarm_hits, stats.unwarmed_misses
    );
    rig.report_verdict_paths("https_handshake");
}

fn h2_download(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let rig = rig(&rt, h2);
    let mut direct = rt.block_on(h2_session(rig.origin, &rig.to_origin));
    let mut spliced = rt.block_on(h2_session(rig.spliced, &rig.to_origin));
    let mut intercepted = rt.block_on(h2_session(rig.intercepted, &rig.to_fah));

    let mut group = c.benchmark_group("https_h2_download");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(BIG_BODY as u64));
    group.bench_function("direct_to_origin", |b| {
        b.iter(|| assert_eq!(rt.block_on(h2_get(&mut direct, BIG_PATH)), BIG_BODY));
    });
    group.bench_function("spliced", |b| {
        b.iter(|| assert_eq!(rt.block_on(h2_get(&mut spliced, BIG_PATH)), BIG_BODY));
    });
    group.bench_function("intercepted", |b| {
        b.iter(|| assert_eq!(rt.block_on(h2_get(&mut intercepted, BIG_PATH)), BIG_BODY));
    });
    group.finish();
    rig.report_verdict_paths("https_h2_download");
}

fn prewarm_hop(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let (_dir, store) = store_with_ca();
    store.prewarm(ORIGIN_NAME).unwrap();

    let mut group = c.benchmark_group("prewarm_hop");
    group.bench_function("inline_cached_leaf", |b| {
        b.iter(|| rt.block_on(async { black_box(store.cached_leaf(ORIGIN_NAME)) }));
    });
    group.bench_function("inline_prewarm_warm", |b| {
        b.iter(|| rt.block_on(async { black_box(store.prewarm(ORIGIN_NAME).unwrap()) }));
    });
    group.bench_function("spawn_blocking_prewarm", |b| {
        b.iter(|| {
            let store = Arc::clone(&store);
            rt.block_on(async move {
                black_box(
                    tokio::task::spawn_blocking(move || store.prewarm(ORIGIN_NAME))
                        .await
                        .unwrap()
                        .unwrap(),
                )
            })
        });
    });
    group.finish();
}

criterion_group!(benches, handshake, h2_download, prewarm_hop);
criterion_main!(benches);
