//! Pass-through cost: what the proxy adds over talking to the origin directly.
//!
//! p2-02's acceptance criterion is added latency below 1 ms p99 in-process, so
//! both arms run against the same origin over loopback with a warm keep-alive
//! connection on each side. The difference between them is the proxy's own
//! work — parse the head, resolve, judge, re-emit, relay — with connection
//! setup excluded from both, because a transparent proxy amortises it across
//! every request on the connection.
//!
//! `pass_through` keeps its body deliberately small: it measures the *head*
//! path. `opaque_body` is the second budget row — bytes the proxy never parses
//! (images, archives, video), where the verdict is taken on the head and the
//! body is relayed untouched. Its arms are the same direct/proxied pair at
//! growing sizes, so a cost that scales with body size shows up as a widening
//! gap rather than a constant offset. Which cost it is, these benches cannot
//! say — see `opaque_body`.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use http_body_util::{BodyExt, Full};
use hyper::header::HOST;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

use fah_config::{HttpsConfig, HttpsListenConfig};
use fah_http::{Proxy, ProxyCounters, TlsServer};
use fah_model::Event;
use fah_rules::{Matcher, MatcherBuilder};

/// The bench never exercises real DNS; resolution is a fixed answer so the
/// measurement is the proxy's own work, not a resolver's.
struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

const PAYLOAD: &[u8] = b"HTTP pass-through benchmark payload; small on purpose.";

/// Body sizes for the opaque row. 8 KiB is a small asset, 1 MiB a photo,
/// 8 MiB a video chunk — enough spread that a per-byte cost cannot hide.
const BODY_SIZES: [usize; 3] = [8 * 1024, 1024 * 1024, 8 * 1024 * 1024];

async fn origin() -> SocketAddr {
    origin_serving(Bytes::from_static(PAYLOAD)).await
}

/// Allocated once and cloned per response: `Bytes` clones are a refcount bump,
/// so the origin's own cost stays out of the measurement at every size.
async fn origin_serving(body: Bytes) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            tokio::spawn(async move {
                let service = service_fn(move |_request: Request<hyper::body::Incoming>| {
                    let body = body.clone();
                    async move { Ok::<_, Infallible>(Response::new(Full::new(body))) }
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

async fn proxy_in_front_of(origin: SocketAddr) -> SocketAddr {
    let proxy = Arc::new(Proxy::new(
        Arc::new(FixedResolver(vec![origin.ip()])),
        // Loopback is denied by default; the bench's origin is on it, so the
        // exception is what a real internal-service allow-list would look like.
        DestinationPolicy::new(
            origin.port(),
            vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
        ),
        origin.port(),
        // Generous: the idle arm must survive while the other arm is being
        // measured. At the shipped 10 s default the header timeout closes it
        // mid-run — which is the slowloris bound doing its job, not a bug.
        Duration::from_secs(120),
        Duration::from_secs(120),
        8,
        false,
    ));
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

fn request(host: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .uri("/resource")
        .header(HOST, host)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn pass_through(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let origin_addr = rt.block_on(origin());
    let proxy_addr = rt.block_on(proxy_in_front_of(origin_addr));
    let host = format!("origin.test:{}", origin_addr.port());

    let mut group = c.benchmark_group("http_pass_through");

    let mut direct = rt.block_on(connect(origin_addr));
    rt.block_on(async {
        direct.ready().await.unwrap();
        let _ = direct
            .send_request(request(&host))
            .await
            .unwrap()
            .into_body()
            .collect()
            .await;
    });

    group.bench_function("direct_to_origin", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Reused across iterations, so wait for capacity rather
                // than assuming the connection is idle.
                direct.ready().await.unwrap();
                let response = direct.send_request(request(&host)).await.unwrap();
                response.into_body().collect().await.unwrap().to_bytes()
            })
        });
    });

    // Connected only now: an idle keep-alive connection would not have
    // survived the arm above. The warm-up request also pays for the upstream
    // connection, which every measured iteration then reuses.
    let mut proxied = rt.block_on(connect(proxy_addr));
    rt.block_on(async {
        proxied.ready().await.unwrap();
        let _ = proxied
            .send_request(request(&host))
            .await
            .unwrap()
            .into_body()
            .collect()
            .await;
    });

    group.bench_function("through_proxy", |b| {
        b.iter(|| {
            rt.block_on(async {
                proxied.ready().await.unwrap();
                let response = proxied.send_request(request(&host)).await.unwrap();
                response.into_body().collect().await.unwrap().to_bytes()
            })
        });
    });

    group.finish();
}

/// Opaque bodies: the verdict is taken on the head, then the bytes are relayed
/// with no parsing, buffering or rewriting. Reported as throughput so the two
/// arms are directly comparable across sizes — the pass criterion is that the
/// proxied/direct ratio stays flat as the body grows.
///
/// **What a widening ratio does and does not say.** It shows a cost that scales
/// with body size; it does not say which cost. Copying, socket buffering, task
/// wakeups and cache behaviour all scale, and this bench separates none of them.
/// Treat a regression here as a signal to profile, not as a diagnosis.
fn opaque_body(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("http_opaque_body");
    // Multi-megabyte transfers over loopback are slow enough that criterion's
    // default 100 samples would run for minutes per arm.
    group.sample_size(20);

    for size in BODY_SIZES {
        let body = Bytes::from(vec![b'x'; size]);
        let origin_addr = rt.block_on(origin_serving(body));
        let proxy_addr = rt.block_on(proxy_in_front_of(origin_addr));
        let host = format!("origin.test:{}", origin_addr.port());

        group.throughput(Throughput::Bytes(size as u64));

        let mut direct = rt.block_on(connect(origin_addr));
        rt.block_on(async {
            direct.ready().await.unwrap();
            let _ = direct
                .send_request(request(&host))
                .await
                .unwrap()
                .into_body()
                .collect()
                .await;
        });

        group.bench_with_input(BenchmarkId::new("direct_to_origin", size), &size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    direct.ready().await.unwrap();
                    let response = direct.send_request(request(&host)).await.unwrap();
                    response.into_body().collect().await.unwrap().to_bytes()
                })
            });
        });

        // Connected after the direct arm for the same reason as `pass_through`:
        // an idle keep-alive connection would not have survived it.
        let mut proxied = rt.block_on(connect(proxy_addr));
        rt.block_on(async {
            proxied.ready().await.unwrap();
            let _ = proxied
                .send_request(request(&host))
                .await
                .unwrap()
                .into_body()
                .collect()
                .await;
        });

        group.bench_with_input(BenchmarkId::new("through_proxy", size), &size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    proxied.ready().await.unwrap();
                    let response = proxied.send_request(request(&host)).await.unwrap();
                    response.into_body().collect().await.unwrap().to_bytes()
                })
            });
        });
    }

    group.finish();
}

const SPLICE_SIZE: usize = 1024 * 1024;

const SPLICE_HOST: &str = "origin.test";

fn client_hello(host: &str) -> Vec<u8> {
    let mut entry = vec![0u8];
    entry.extend_from_slice(&(host.len() as u16).to_be_bytes());
    entry.extend_from_slice(host.as_bytes());

    let mut sni = Vec::new();
    sni.extend_from_slice(&(entry.len() as u16).to_be_bytes());
    sni.extend_from_slice(&entry);

    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0u16.to_be_bytes());
    extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni);

    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&[0x42; 32]);
    body.push(0);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&[0x13, 0x01]);
    body.push(1);
    body.push(0);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    let mut handshake = vec![0x01u8];
    handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    handshake.extend_from_slice(&body);

    let mut record = vec![0x16u8, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

const SPLICE_STEADY_SIZE: usize = 64 * 1024 * 1024;

const DRAIN_CHUNK: usize = 64 * 1024;

async fn raw_origin(payload: Arc<Vec<u8>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let payload = Arc::clone(&payload);
            tokio::spawn(async move {
                let (mut reader, mut writer) = stream.into_split();
                tokio::spawn(async move {
                    let mut sink = tokio::io::sink();
                    let _ = tokio::io::copy(&mut reader, &mut sink).await;
                });
                let _ = writer.write_all(&payload).await;
                let _ = writer.shutdown().await;
            });
        }
    });
    addr
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

async fn splice_in_front_of(origin: SocketAddr) -> (SocketAddr, Arc<ProxyCounters>) {
    let proxy = Arc::new(
        fah_http::TlsProxy::new(
            Arc::new(FixedResolver(vec![origin.ip()])),
            DestinationPolicy::new(
                origin.port(),
                vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
            ),
            origin.port(),
            Duration::from_secs(120),
            Duration::from_secs(120),
            fah_config::NoSni::Pass,
        )
        .with_rules(rules_with(BENCH_RULES))
        .with_events(drained_events()),
    );
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
    server.serve(proxy);
    (addr, counters)
}

async fn drain(addr: SocketAddr, hello: Option<&[u8]>, size: usize) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    if let Some(hello) = hello {
        stream.write_all(hello).await.unwrap();
    }
    let mut buf = vec![0u8; DRAIN_CHUNK];
    let mut received = 0usize;
    while received < size {
        let read = stream.read(&mut buf).await.unwrap();
        assert!(read > 0, "origin closed after {received} of {size} bytes");
        received += read;
    }
}

fn splice_arms(
    c: &mut Criterion,
    rt: &Runtime,
    name: &str,
    size: usize,
    sample_size: usize,
    hello: &[u8],
) {
    let origin = rt.block_on(raw_origin(Arc::new(vec![b'x'; size])));
    let (proxy, counters) = rt.block_on(splice_in_front_of(origin));

    let mut group = c.benchmark_group(name);
    group.sample_size(sample_size);
    group.throughput(Throughput::Bytes(size as u64));

    group.bench_function("direct_to_origin", |b| {
        b.iter(|| rt.block_on(drain(origin, None, size)));
    });

    group.bench_function("through_splice", |b| {
        b.iter(|| rt.block_on(drain(proxy, Some(hello), size)));
    });

    group.finish();

    let stats = counters.snapshot();
    println!(
        "{name} verdict path: connections={} requests={} blocked={} \
         refused_destination={} resolve_failures={} dropped_events={}",
        stats.connections,
        stats.requests,
        stats.blocked,
        stats.refused_destination,
        stats.resolve_failures,
        stats.dropped_events
    );
}

fn splice(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let hello = client_hello(SPLICE_HOST);
    splice_arms(c, &rt, "https_sni_splice", SPLICE_SIZE, 20, &hello);
    splice_arms(
        c,
        &rt,
        "https_sni_splice_steady_state",
        SPLICE_STEADY_SIZE,
        10,
        &hello,
    );
}

criterion_group!(benches, pass_through, opaque_body, splice);
criterion_main!(benches);
