//! p2-04: verdicts wired into the proxy.
//!
//! Every test here drives a **real** proxy against a **real** origin over a
//! real socket, so "blocked" means the client got a block response and the
//! origin was never touched — not that a function returned an enum.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_model::{Event, EventKind, ResourceType, Verdict};
use fah_rules::{Matcher, MatcherBuilder, PolicySet, PolicyState};
use http_body_util::{BodyExt, Full};
use hyper::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, HOST, REFERER};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use fah_http::Proxy;

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

/// An origin that counts how many connections it ever accepted. A blocked
/// request must leave this at zero.
async fn origin() -> (SocketAddr, Arc<AtomicU64>) {
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
    (addr, accepts)
}

/// A fixed compiled ruleset — the whole reason `Ruleset` is a trait rather
/// than `Arc<ListManager>`: these tests need a matcher, not a list lifecycle.
struct FixedRules(Arc<Matcher>);

impl fah_http::Ruleset for FixedRules {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

/// Compiles `lines` as an adblock list into a ruleset the proxy can consult.
fn rules_with(lines: &str) -> Arc<dyn fah_http::Ruleset> {
    let parsed = fah_rules::parse_rule_list(lines);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("test-list", &parsed);
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn proxy_with(
    origin_port: u16,
    origin_ip: IpAddr,
    rules: Option<Arc<dyn fah_http::Ruleset>>,
    events: Option<mpsc::Sender<Event>>,
) -> Arc<Proxy> {
    let policy = DestinationPolicy::new(
        origin_port,
        vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
    );
    let mut proxy = Proxy::new(
        Arc::new(FixedResolver(vec![origin_ip])),
        policy,
        origin_port,
        Duration::from_secs(5),
        Duration::from_secs(5),
        8,
        false,
    );
    if let Some(rules) = rules {
        proxy = proxy.with_rules(rules);
    }
    if let Some(events) = events {
        proxy = proxy.with_events(events);
    }
    Arc::new(proxy)
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

async fn send(
    proxy: SocketAddr,
    request: Request<Full<Bytes>>,
) -> (StatusCode, hyper::HeaderMap, Bytes) {
    let stream = TcpStream::connect(proxy).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let response = sender.send_request(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body)
}

fn request(path: &str, host: &str) -> hyper::http::request::Builder {
    Request::builder().uri(path).header(HOST, host)
}

// ─── The acceptance criterion: blocked means nothing is fetched ───────────

#[tokio::test]
async fn a_blocked_request_fetches_zero_bytes_upstream() {
    let (origin, accepts) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (status, _, body) = send(
        proxy,
        request("/pixel.gif", "ads.example.com")
            .header(ACCEPT, "image/webp,*/*")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "an image block collapses quietly");
    assert!(body.is_empty(), "a block must ship no payload");
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "the origin must never have been contacted"
    );
}

#[tokio::test]
async fn an_allowed_request_still_streams_from_the_origin() {
    let (origin, accepts) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (status, _, body) = send(
        proxy,
        request("/index.html", "cdn.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"ORIGIN PAYLOAD");
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
}

// ─── Block shape follows the resource type ────────────────────────────────

#[tokio::test]
async fn a_blocked_script_gets_an_empty_success_and_a_document_gets_a_page() {
    let (origin, _) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (status, headers, body) = send(
        proxy,
        request("/track.js", "ads.example.com")
            .header("sec-fetch-dest", "script")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[CONTENT_TYPE], "application/javascript");
    assert!(body.is_empty());
    assert_eq!(headers[CACHE_CONTROL], "no-store");

    let (status, headers, body) = send(
        proxy,
        request("/page.html", "ads.example.com")
            .header("sec-fetch-dest", "document")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(headers[CONTENT_TYPE]
        .to_str()
        .unwrap()
        .contains("text/html"));
    let page = String::from_utf8(body.to_vec()).unwrap();
    assert!(page.contains("FastAdHunter"), "{page}");
    assert!(page.contains("ads.example.com"), "{page}");
}

// ─── The URL tier actually decides, not just the domain tier ──────────────

#[tokio::test]
async fn a_url_rule_blocks_one_path_and_leaves_the_rest_of_the_host_alone() {
    let (origin, accepts) = origin().await;
    let rules = rules_with("||cdn.example.com/ads/banner\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (status, _, _) = send(
        proxy,
        request("/ads/banner.gif", "cdn.example.com")
            .header(ACCEPT, "image/webp")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepts.load(Ordering::Relaxed), 0, "blocked path");

    let (status, _, body) = send(
        proxy,
        request("/assets/app.js", "cdn.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"ORIGIN PAYLOAD", "the rest of the host is fine");
}

/// The p2-03 review's port-stripping requirement, proven through the proxy: the
/// document host is taken from `Referer` and must lose its port, or
/// `$domain=` silently stops applying.
#[tokio::test]
async fn a_domain_scoped_rule_is_judged_on_the_referer_host_without_its_port() {
    let (origin, accepts) = origin().await;
    let rules = rules_with("||tracker.example.com^$domain=news.org\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (status, _, _) = send(
        proxy,
        request("/collect", "tracker.example.com")
            .header(REFERER, "http://news.org:8080/article")
            .header("sec-fetch-dest", "empty")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "a port on the referer must not stop $domain= from applying"
    );

    // …and a document the rule does not name is still fetched.
    let (_, _, body) = send(
        proxy,
        request("/collect", "tracker.example.com")
            .header(REFERER, "http://other.com/x")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(&body[..], b"ORIGIN PAYLOAD");
}

/// An exception must override a block from either tier — and, since p2-03's
/// review, must apply even when the resource type could not be determined.
#[tokio::test]
async fn an_exception_overrides_a_block_across_tiers() {
    let (origin, _) = origin().await;
    let rules = rules_with("||cdn.example.com^\n@@||cdn.example.com/assets/$script\n");
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), Some(rules), None)).await;

    let (_, _, body) = send(
        proxy,
        request("/assets/app.js", "cdn.example.com")
            .header("sec-fetch-dest", "script")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(&body[..], b"ORIGIN PAYLOAD", "the exception must win");
}

// ─── Events ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_blocked_request_emits_an_event_naming_the_rule() {
    let (origin, _) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let (tx, mut rx) = mpsc::channel(16);
    let proxy = spawn_proxy(proxy_with(
        origin.port(),
        origin.ip(),
        Some(rules),
        Some(tx),
    ))
    .await;

    send(
        proxy,
        request("/pixel.gif?id=7", "ads.example.com")
            .header(ACCEPT, "image/webp")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("an event must arrive")
        .expect("channel open");
    assert_eq!(event.kind(), EventKind::Http);
    let Event::Http(event) = event else {
        panic!("expected an HTTP event");
    };
    assert_eq!(event.request.host, "ads.example.com");
    assert_eq!(event.request.path, "/pixel.gif?id=7");
    assert_eq!(event.request.method, "GET");
    assert_eq!(event.request.resource_type, ResourceType::Image);
    assert_eq!(event.status, 200);
    assert_eq!(event.bytes, 0, "a block relays nothing");
    match &event.verdict {
        Verdict::Block(rule) => assert!(rule.rule.contains("ads.example.com"), "{}", rule.rule),
        other => panic!("expected a block verdict, got {other:?}"),
    }
}

#[tokio::test]
async fn a_forwarded_request_emits_a_pass_event_with_the_relayed_byte_count() {
    let (origin, _) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let (tx, mut rx) = mpsc::channel(16);
    let proxy = spawn_proxy(proxy_with(
        origin.port(),
        origin.ip(),
        Some(rules),
        Some(tx),
    ))
    .await;

    send(
        proxy,
        request("/index.html", "cdn.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("an event must arrive")
        .expect("channel open");
    let Event::Http(event) = event else {
        panic!("expected an HTTP event");
    };
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 200);
    assert_eq!(event.bytes, b"ORIGIN PAYLOAD".len() as u64);
    assert!(event.duration > Duration::ZERO);
}

/// The observability queue must never become backpressure on traffic: a full
/// channel sheds, counts, and the request still completes.
#[tokio::test]
async fn a_full_event_channel_sheds_rather_than_stalling_the_request() {
    let (origin, _) = origin().await;
    let rules = rules_with("||ads.example.com^\n");
    let (tx, rx) = mpsc::channel(1);
    let proxy_handle = proxy_with(origin.port(), origin.ip(), Some(rules), Some(tx));
    let counters = proxy_handle.counters();
    let proxy = spawn_proxy(proxy_handle).await;
    // Held but never read: the channel fills after the first event and stays
    // full, which is what a stalled consumer looks like.
    let _never_read = rx;

    for _ in 0..8 {
        let (status, _, _) = send(
            proxy,
            request("/pixel.gif", "ads.example.com")
                .header(ACCEPT, "image/webp")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "the request must still complete");
    }
    let stats = counters.snapshot();
    assert_eq!(stats.blocked, 8);
    assert!(
        stats.dropped_events > 0,
        "a full channel must shed and count, not block"
    );
}

// ─── Per-client policy (p2-06) ────────────────────────────────────────────

/// Compiles `lines` under `policies`, exactly as the list lifecycle does.
fn rules_under(lines: &str, policies: &PolicySet) -> Arc<dyn fah_http::Ruleset> {
    let parsed = fah_rules::parse_rule_list(lines);
    let mut builder = MatcherBuilder::new();
    builder.set_policy_universe(policies.universe());
    builder.add_parsed_list_masked("test-list", &parsed, policies.mask_for_list("test-list"));
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn policy_config(id: &str, lists: &[&str], client: &str) -> fah_config::PolicyConfig {
    fah_config::PolicyConfig {
        id: id.to_string(),
        name: None,
        lists: Some(lists.iter().map(|l| l.to_string()).collect()),
        blocking_mode: None,
        assignments: vec![fah_config::AssignmentConfig {
            client: client.to_string(),
            days: None,
            start: None,
            end: None,
        }],
    }
}

/// Binds the proxy on the dual-stack wildcard so two *different* loopback
/// clients can reach it — `127.0.0.1` and `::1` are the only two distinct
/// source addresses available in a test.
async fn spawn_dual_stack_proxy(proxy: Arc<Proxy>) -> u16 {
    // `fah_common::bind_tcp`, not `TcpListener::bind`: Windows defaults
    // `IPV6_V6ONLY` on, so a raw `[::]` bind would refuse the v4 client.
    let listener = fah_common::listen::bind_tcp("[::]:0".parse().unwrap())
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                return;
            };
            let proxy = Arc::clone(&proxy);
            tokio::spawn(async move { proxy.serve_connection(stream, peer).await });
        }
    });
    port
}

/// The DoD scenario on the HTTP side: one client's policy carries the
/// blocklist, the other's does not — same URL, two outcomes.
#[tokio::test]
async fn two_clients_on_two_policies_get_different_verdicts_for_one_url() {
    let (origin, accepts) = origin().await;
    let policies = PolicySet::from_config(
        "UTC",
        &[
            // The IPv4 loopback client sees the list; the IPv6 one does not.
            policy_config("kids", &["test-list"], "127.0.0.1"),
            policy_config("open", &["nothing"], "::1"),
        ],
    )
    .unwrap();
    let state = Arc::new(PolicyState::default());
    state.publish(policies.active_at(0, &[]));

    let proxy = proxy_with(
        origin.port(),
        origin.ip(),
        Some(rules_under("||ads.example.com^\n", &policies)),
        None,
    );
    // `proxy_with` returns an `Arc`; rebuild it with the policy state attached.
    let proxy = Arc::new(
        Arc::try_unwrap(proxy)
            .unwrap_or_else(|_| unreachable!("sole owner"))
            .with_policies(Arc::clone(&state)),
    );
    let port = spawn_dual_stack_proxy(proxy).await;

    let blocked = send(
        SocketAddr::new("127.0.0.1".parse().unwrap(), port),
        request("/pixel.gif", "ads.example.com")
            .header(ACCEPT, "image/webp,*/*")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert!(blocked.2.is_empty(), "the kids client is blocked");
    assert_eq!(accepts.load(Ordering::Relaxed), 0);

    let allowed = send(
        SocketAddr::new("::1".parse().unwrap(), port),
        request("/pixel.gif", "ads.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(
        &allowed.2[..],
        b"ORIGIN PAYLOAD",
        "the open client's policy enables no list, so the fetch goes through"
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
}

/// A v4 client arriving through the dual-stack listener is reported as
/// `::ffff:127.0.0.1`. It must be canonicalized before anything looks at it,
/// or a policy assigned to the v4 address never matches — and the same device
/// appears twice in the query log, once per pipeline.
#[tokio::test]
async fn a_v4_mapped_peer_matches_its_v4_policy_assignment() {
    let (origin, accepts) = origin().await;
    let policies =
        PolicySet::from_config("UTC", &[policy_config("kids", &["test-list"], "127.0.0.1")])
            .unwrap();
    let state = Arc::new(PolicyState::default());
    state.publish(policies.active_at(0, &[]));

    // The list is visible only to `kids`, so a block proves the assignment
    // matched — the default policy would not see this rule.
    let mut only_kids = policies.mask_for_list("test-list");
    only_kids &= !fah_model::PolicyId::DEFAULT.bit();
    let parsed = fah_rules::parse_rule_list("||ads.example.com^\n");
    let mut builder = MatcherBuilder::new();
    builder.set_policy_universe(policies.universe());
    builder.add_parsed_list_masked("test-list", &parsed, only_kids);
    let rules: Arc<dyn fah_http::Ruleset> = Arc::new(FixedRules(Arc::new(builder.build())));

    let proxy = proxy_with(origin.port(), origin.ip(), Some(rules), None);
    let proxy = Arc::new(
        Arc::try_unwrap(proxy)
            .unwrap_or_else(|_| unreachable!("sole owner"))
            .with_policies(state),
    );
    let port = spawn_dual_stack_proxy(proxy).await;

    let (_, _, body) = send(
        SocketAddr::new("127.0.0.1".parse().unwrap(), port),
        request("/pixel.gif", "ads.example.com")
            .header(ACCEPT, "image/webp,*/*")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert!(
        body.is_empty(),
        "the v4-mapped peer must resolve to the 127.0.0.1 assignment"
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 0);
}

// ─── No ruleset attached: p2-02 behaviour is preserved ────────────────────

#[tokio::test]
async fn a_proxy_with_no_ruleset_forwards_everything() {
    let (origin, accepts) = origin().await;
    let proxy = spawn_proxy(proxy_with(origin.port(), origin.ip(), None, None)).await;

    let (status, _, body) = send(
        proxy,
        request("/pixel.gif", "ads.example.com")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"ORIGIN PAYLOAD");
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
}
