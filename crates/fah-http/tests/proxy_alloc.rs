use std::alloc::{GlobalAlloc, Layout};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_model::Event;
use fah_rules::{Matcher, MatcherBuilder};
use http_body_util::{BodyExt, Full};
use hyper::header::{HeaderName, HeaderValue, ACCEPT, CONNECTION, HOST};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use mimalloc::MiMalloc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use fah_http::Proxy;

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

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

async fn origin() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let service = service_fn(|_request| async {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(
                        b"ORIGIN PAYLOAD",
                    ))))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    addr
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

async fn spawn_proxy(proxy: Arc<Proxy>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                return;
            };
            let proxy = Arc::clone(&proxy);
            tokio::spawn(async move { proxy.serve_connection(stream, peer).await });
        }
    });
    addr
}

async fn connect(addr: SocketAddr) -> hyper::client::conn::http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
}

struct Case {
    label: &'static str,
    path: &'static str,
    host: &'static str,
    extra: Option<(HeaderName, &'static str)>,
    ceiling_per_request: usize,
}

fn request(case: &Case) -> Request<Full<Bytes>> {
    let mut builder = Request::builder().uri(case.path).header(HOST, case.host);
    if let Some((name, value)) = &case.extra {
        builder = builder.header(name.clone(), HeaderValue::from_static(value));
    }
    builder.body(Full::new(Bytes::new())).unwrap()
}

#[test]
fn warm_proxy_requests_allocate_a_steady_amount() {
    const REQUESTS: usize = 64;
    const JITTER_ALLOWANCE: usize = 4;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let origin_addr = rt.block_on(origin());
    let (events, mut receiver) = mpsc::channel::<Event>(4096);
    let policy = DestinationPolicy::new(
        origin_addr.port(),
        vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
    );
    let proxy = Arc::new(
        Proxy::new(
            Arc::new(FixedResolver(vec![origin_addr.ip()])),
            policy,
            origin_addr.port(),
            Duration::from_secs(120),
            Duration::from_secs(120),
            8,
            false,
        )
        .with_rules(rules_with("||ads.example^\n"))
        .with_events(events),
    );
    let proxy_addr = rt.block_on(spawn_proxy(proxy));
    let mut sender = rt.block_on(connect(proxy_addr));

    let cases = [
        Case {
            label: "pass-through GET",
            path: "/resource",
            host: "allowed.example",
            extra: None,
            ceiling_per_request: 51,
        },
        Case {
            label: "pass-through GET with Connection header",
            path: "/resource",
            host: "allowed.example",
            extra: Some((CONNECTION, "keep-alive")),
            ceiling_per_request: 53,
        },
        Case {
            label: "blocked script",
            path: "/ad.js",
            host: "ads.example",
            extra: None,
            ceiling_per_request: 20,
        },
        Case {
            label: "blocked document",
            path: "/",
            host: "ads.example",
            extra: Some((ACCEPT, "text/html")),
            ceiling_per_request: 32,
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
        for _ in 0..2 {
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
            "proxy/allocations over {REQUESTS} warm requests ({}): first batch {} allocations {} bytes, second batch {} allocations {} bytes",
            case.label, measured[0].0, measured[0].1, measured[1].0, measured[1].1
        );

        assert!(
            measured[1].0 <= measured[0].0 + JITTER_ALLOWANCE,
            "{REQUESTS} warm requests ({}) allocated {} then {}; the proxy must not accumulate. \
             This checks the absence of growth, not bit-for-bit equality between two \
             measurements a scheduler and TCP chunking both touch: one leaked allocation per \
             request would show as +{REQUESTS} here, so a difference within {JITTER_ALLOWANCE} \
             is noise",
            case.label,
            measured[0].0,
            measured[1].0
        );
        assert!(
            measured[1].0 <= REQUESTS * case.ceiling_per_request,
            "{REQUESTS} warm requests ({}) allocated {}; the ceiling is {} per request",
            case.label,
            measured[1].0,
            case.ceiling_per_request
        );
    }
}
