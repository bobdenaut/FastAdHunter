//! Pass-through cost: what the proxy adds over talking to the origin directly.
//!
//! p2-02's acceptance criterion is added latency below 1 ms p99 in-process, so
//! both arms run against the same origin over loopback with a warm keep-alive
//! connection on each side. The difference between them is the proxy's own
//! work — parse the head, resolve, judge, re-emit, relay — with connection
//! setup excluded from both, because a transparent proxy amortises it across
//! every request on the connection.
//!
//! The body is deliberately small. This measures the *head* path; the body path
//! is a copy with no per-byte work, which
//! `the_body_streams_rather_than_being_buffered_whole` covers for behaviour and
//! `a_multi_megabyte_body_is_relayed_intact` for integrity.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, Criterion};
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use http_body_util::{BodyExt, Full};
use hyper::header::HOST;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

use fah_http::Proxy;

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

async fn origin() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let service = service_fn(|_request: Request<hyper::body::Incoming>| async {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(PAYLOAD))))
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

criterion_group!(benches, pass_through);
criterion_main!(benches);
