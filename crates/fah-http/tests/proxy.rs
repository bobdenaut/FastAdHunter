//! End-to-end proxy behaviour: a real origin server, a real client, and the
//! egress guard between them.
//!
//! The origin binds an ephemeral port, so each test builds its policy with that
//! port as the intercepted one. Loopback is denied by default — every test that
//! wants to reach the origin has to allow it explicitly, which is itself the
//! proof that the default refuses it.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use http_body_util::{BodyExt, Full};
use hyper::header::{HOST, VIA};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use fah_http::Proxy;

/// Resolves every name to a fixed set of addresses — the seam the rebind tests
/// use to make a perfectly ordinary hostname point somewhere private.
struct FixedResolver {
    addresses: Vec<IpAddr>,
    calls: Arc<AtomicU64>,
}

impl FixedResolver {
    fn new(addresses: Vec<IpAddr>) -> (Arc<Self>, Arc<AtomicU64>) {
        let calls = Arc::new(AtomicU64::new(0));
        let resolver = Arc::new(Self {
            addresses,
            calls: Arc::clone(&calls),
        });
        (resolver, calls)
    }
}

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let addresses = self.addresses.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

fn loopback_policy(origin_port: u16) -> DestinationPolicy {
    DestinationPolicy::new(
        origin_port,
        vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
    )
}

fn proxy_for(
    origin_port: u16,
    policy: DestinationPolicy,
    resolver: Arc<dyn HostResolver>,
) -> Arc<Proxy> {
    Arc::new(Proxy::new(
        resolver,
        policy,
        origin_port,
        Duration::from_secs(5),
        Duration::from_secs(5),
        8,
        false,
    ))
}

/// Runs the proxy over a real TCP listener and returns its address.
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

/// A minimal origin answering every request with `body`, echoing back the
/// headers it saw, and counting TCP accepts — the connection-reuse evidence.
async fn origin_echoing(body: Bytes) -> (SocketAddr, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&accepts);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::Relaxed);
            let body = body.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let body = body.clone();
                    async move {
                        let header = |name: &str| {
                            request
                                .headers()
                                .get(name)
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("")
                                .to_string()
                        };
                        Ok::<_, Infallible>(
                            Response::builder()
                                .header("x-seen-host", header("host"))
                                .header("x-seen-via", header("via"))
                                .header("x-seen-proxy-auth", header("proxy-authorization"))
                                .body(Full::new(body))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (addr, accepts)
}

async fn client_to(addr: SocketAddr) -> hyper::client::conn::http1::SendRequest<Full<Bytes>> {
    let stream = TcpStream::connect(addr).await.unwrap();
    client_over(stream).await
}

/// Same client, over **any** stream — this is what proves the proxy is not
/// bound to `TcpStream`.
async fn client_over<S>(stream: S) -> hyper::client::conn::http1::SendRequest<Full<Bytes>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
}

fn get(host: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .uri("/resource")
        .header(HOST, host)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[tokio::test]
async fn a_request_is_proxied_to_the_origin_and_the_response_comes_back() {
    let (origin, _) = origin_echoing(Bytes::from_static(b"hello from the origin")).await;
    let (resolver, resolve_calls) = FixedResolver::new(vec![origin.ip()]);
    let proxy_addr = spawn_proxy(proxy_for(
        origin.port(),
        loopback_policy(origin.port()),
        resolver,
    ))
    .await;

    let mut client = client_to(proxy_addr).await;
    let response = client
        .send_request(get(&format!("origin.test:{}", origin.port())))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let seen_host = response.headers().get("x-seen-host").unwrap().clone();
    let seen_via = response.headers().get("x-seen-via").unwrap().clone();
    let via_back = response.headers().get(VIA).unwrap().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body, Bytes::from_static(b"hello from the origin"));
    assert_eq!(
        seen_host,
        format!("origin.test:{}", origin.port()).as_str(),
        "the origin must see the name the client asked for, not the address we reached it at"
    );
    assert_eq!(seen_via, "1.1 fastadhunter");
    assert_eq!(via_back, "1.1 fastadhunter");
    assert_eq!(resolve_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn hop_by_hop_headers_do_not_reach_the_origin() {
    let (origin, _) = origin_echoing(Bytes::from_static(b"ok")).await;
    let (resolver, _) = FixedResolver::new(vec![origin.ip()]);
    let proxy_addr = spawn_proxy(proxy_for(
        origin.port(),
        loopback_policy(origin.port()),
        resolver,
    ))
    .await;

    let mut client = client_to(proxy_addr).await;
    let request = Request::builder()
        .uri("/resource")
        .header(HOST, format!("origin.test:{}", origin.port()))
        .header("proxy-authorization", "Basic bogus")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = client.send_request(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-seen-proxy-auth").unwrap(),
        "",
        "Proxy-Authorization is hop-by-hop and must stop here"
    );
}

#[tokio::test]
async fn a_multi_megabyte_body_is_relayed_intact() {
    const SIZE: usize = 8 * 1024 * 1024;
    let payload = Bytes::from(vec![0xA5u8; SIZE]);
    let (origin, _) = origin_echoing(payload).await;
    let (resolver, _) = FixedResolver::new(vec![origin.ip()]);
    let proxy_addr = spawn_proxy(proxy_for(
        origin.port(),
        loopback_policy(origin.port()),
        resolver,
    ))
    .await;

    let mut client = client_to(proxy_addr).await;
    let response = client
        .send_request(get(&format!("origin.test:{}", origin.port())))
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(body.len(), SIZE, "every byte must survive the hop");
    assert!(
        body.iter().all(|byte| *byte == 0xA5),
        "and survive it unmodified"
    );
}

/// **Streaming, not buffering** — the property RSS sampling would only hint at.
///
/// The origin writes a chunked response by hand: one chunk, then a stall, then
/// the rest. Reading that first chunk while the origin is still holding the
/// response open is only possible if the proxy relays as it goes. A proxy that
/// collected the body first would leave the client waiting for the stall.
#[tokio::test]
async fn the_body_streams_rather_than_being_buffered_whole() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Read past the request head; the body is empty.
        let mut head = [0u8; 1024];
        let _ = stream.read(&mut head).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        // "FIRST-CHUNK" is 11 bytes -> 0xB.
        stream.write_all(b"B\r\nFIRST-CHUNK\r\n").await.unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        // "LAST-CHUNK" is 10 bytes -> 0xA.
        stream.write_all(b"A\r\nLAST-CHUNK\r\n").await.unwrap();
        stream.write_all(b"0\r\n\r\n").await.unwrap();
        stream.flush().await.unwrap();
    });

    let (resolver, _) = FixedResolver::new(vec![origin.ip()]);
    let proxy_addr = spawn_proxy(proxy_for(
        origin.port(),
        loopback_policy(origin.port()),
        resolver,
    ))
    .await;

    let mut client = client_to(proxy_addr).await;
    let mut body = client
        .send_request(get(&format!("origin.test:{}", origin.port())))
        .await
        .unwrap()
        .into_body();

    let first = tokio::time::timeout(Duration::from_millis(250), body.frame())
        .await
        .expect("the first chunk must arrive while the origin is still stalled")
        .expect("a frame")
        .unwrap();
    assert_eq!(
        first.into_data().unwrap(),
        Bytes::from_static(b"FIRST-CHUNK")
    );
}

/// Keep-alive on both sides: two client requests, one upstream connection.
#[tokio::test]
async fn upstream_connections_are_reused_across_requests() {
    let (origin, accepts) = origin_echoing(Bytes::from_static(b"ok")).await;
    let (resolver, _) = FixedResolver::new(vec![origin.ip()]);
    let proxy_addr = spawn_proxy(proxy_for(
        origin.port(),
        loopback_policy(origin.port()),
        resolver,
    ))
    .await;

    let mut client = client_to(proxy_addr).await;
    for _ in 0..3 {
        let response = client
            .send_request(get(&format!("origin.test:{}", origin.port())))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // The body must be drained before the pooled connection can be reused.
        let _ = response.into_body().collect().await.unwrap();
    }

    assert_eq!(
        accepts.load(Ordering::Relaxed),
        1,
        "three requests must share one upstream connection"
    );
}

/// **The Phase 3 reuse proof.** The proxy is driven over a `DuplexStream` — an
/// in-memory pipe that is emphatically not a `TcpStream`. If this compiles and
/// passes, a rustls-terminated stream fits the same signature.
#[tokio::test]
async fn the_proxy_serves_a_connection_that_is_not_a_tcp_stream() {
    let (origin, _) = origin_echoing(Bytes::from_static(b"over a duplex pipe")).await;
    let (resolver, _) = FixedResolver::new(vec![origin.ip()]);
    let proxy = proxy_for(origin.port(), loopback_policy(origin.port()), resolver);

    let (client_side, proxy_side) = tokio::io::duplex(64 * 1024);
    let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();
    tokio::spawn(async move { proxy.serve_connection(proxy_side, peer).await });

    let mut client = client_over(client_side).await;
    let response = client
        .send_request(get(&format!("origin.test:{}", origin.port())))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"over a duplex pipe"));
}

/// **The proxy is not an open relay.** A perfectly ordinary hostname whose
/// address is private — a DNS rebind — must be refused, which is only possible
/// because the guard runs after resolution.
#[tokio::test]
async fn a_public_name_resolving_to_a_private_address_is_refused() {
    let (resolver, _) = FixedResolver::new(vec!["192.168.10.1".parse().unwrap()]);
    let proxy = proxy_for(80, DestinationPolicy::new(80, Vec::new()), resolver);
    let counters = proxy.counters();
    let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

    let mut client = client_to(proxy_addr).await;
    let response = client
        .send_request(get("totally-normal.example"))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a rebind to the router must not be followed"
    );
    assert_eq!(counters.snapshot().refused_destination, 1);
}

/// Every address the task names, refused and counted.
#[tokio::test]
async fn our_own_infrastructure_is_never_reachable_through_the_proxy() {
    for address in [
        "172.17.0.2",      // our own API
        "192.168.10.1",    // the router
        "172.17.0.1",      // the container subnet
        "127.0.0.1",       // loopback
        "169.254.169.254", // link-local / metadata
        "10.0.0.1",        // RFC 1918
    ] {
        let (resolver, _) = FixedResolver::new(vec![address.parse().unwrap()]);
        let proxy = proxy_for(80, DestinationPolicy::new(80, Vec::new()), resolver);
        let counters = proxy.counters();
        let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

        let mut client = client_to(proxy_addr).await;
        let response = client.send_request(get("anything.example")).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{address} must be refused"
        );
        assert_eq!(
            counters.snapshot().refused_destination,
            1,
            "{address} must be counted"
        );
    }
}

#[tokio::test]
async fn an_ip_literal_host_is_refused_and_counted() {
    let (resolver, calls) = FixedResolver::new(vec!["93.184.216.34".parse().unwrap()]);
    let proxy = proxy_for(80, DestinationPolicy::new(80, Vec::new()), resolver);
    let counters = proxy.counters();
    let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

    let mut client = client_to(proxy_addr).await;
    let response = client.send_request(get("93.184.216.34")).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(counters.snapshot().refused_claim, 1);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "an unusable claim must be rejected before any resolution is attempted"
    );
}

#[tokio::test]
async fn a_request_with_no_host_is_a_bad_request() {
    let (resolver, _) = FixedResolver::new(vec!["93.184.216.34".parse().unwrap()]);
    let proxy = proxy_for(80, DestinationPolicy::new(80, Vec::new()), resolver);
    let counters = proxy.counters();
    let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

    // Written by hand: hyper's client will not build an HTTP/1.1 request
    // without a Host.
    let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
    stream
        .write_all(b"GET /resource HTTP/1.0\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();

    let text = String::from_utf8_lossy(&response);
    assert!(text.contains(" 400 "), "got: {text:?}");
    assert_eq!(counters.snapshot().refused_claim, 1);
}

/// A dead origin is a gateway problem, reported as one rather than as a hang.
#[tokio::test]
async fn an_unreachable_origin_becomes_a_bad_gateway() {
    // Bind then drop, so the port is free but nothing listens.
    let dead = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    };
    let (resolver, _) = FixedResolver::new(vec![dead.ip()]);
    let proxy = proxy_for(dead.port(), loopback_policy(dead.port()), resolver);
    let counters = proxy.counters();
    let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

    let mut client = client_to(proxy_addr).await;
    let response = client
        .send_request(get(&format!("origin.test:{}", dead.port())))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(counters.snapshot().upstream_failures, 1);
}

/// Malformed input must be answered or closed, never panic the task.
#[tokio::test]
async fn malformed_requests_do_not_panic_the_proxy() {
    let (resolver, _) = FixedResolver::new(vec!["93.184.216.34".parse().unwrap()]);
    let proxy = proxy_for(80, DestinationPolicy::new(80, Vec::new()), resolver);
    let proxy_addr = spawn_proxy(Arc::clone(&proxy)).await;

    for garbage in [
        &b"\x00\x01\x02\x03"[..],
        b"GET\r\n\r\n",
        b"GET / HTTP/9.9\r\nHost: x\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: \x7f\x00\r\n\r\n",
        b"\x16\x03\x01\x00\xa5\x01\x00\x00\xa1\x03\x03",
    ] {
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        let _ = stream.write_all(garbage).await;
        let mut response = Vec::new();
        // Either an error response or a clean close; both are fine, a hang is
        // not.
        let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .expect("malformed input must not hang the connection");
    }

    // Still serving after all of that.
    let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert!(
        !response.is_empty(),
        "the listener must survive every malformed connection above"
    );
}
