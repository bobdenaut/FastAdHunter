//! End-to-end proof that the shipped binary filters HTTP (p2-08).
//!
//! `e2e.rs` proves the DNS half. This one boots the same real executable in
//! `mode = "dns+http"` against a mock origin and drives it the way a browser
//! would: fetch a page through the proxy, have an ad script on that page
//! blocked while the page itself still renders, then repeat from a second
//! client whose rule blocks the page host outright — and watch both verdicts
//! arrive on the events socket tagged `kind=http`, and counted as HTTP by
//! `GET /api/v1/telemetry`.
//!
//! **Why the origin is on port 80.** `HTTP_ORIGIN_PORT` is a constant in
//! `main.rs`, and the egress guard refuses any destination on another port
//! without exception (`DestinationPolicy::check`) — deliberately: the router
//! only dst-nats 80, so a request naming another port was never intercepted.
//! That is correct in production and awkward in a test, because it leaves no
//! way to put the origin on an ephemeral port. The test therefore needs
//! 127.0.0.1:80 and skips when it cannot have it, rather than reporting a busy
//! port or an unprivileged Linux box as a product defect.
//!
//! **Why two loopback addresses.** Per-client enforcement keys on the peer
//! address, so the second client has to genuinely come from somewhere else.
//! 127.0.0.2 is still loopback and needs no configuration on Linux or Windows.
//!
//! Fully offline: the mock upstream resolves every name to 127.0.0.1, the
//! origin is in this process, and the config ships zero rule lists.

mod common;

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, UdpSocket};

use common::{boot, connect_events, get_json, put_user_rules, run_mock_upstream_answering, Ports};

/// The only port the proxy will ever originate to — see the module note.
const ORIGIN_PORT: u16 = 80;

/// Hosts the mock origin serves. Both resolve to 127.0.0.1 via the mock
/// upstream; the rules below tell them apart.
const PAGE_HOST: &str = "shop.example.com";
const AD_HOST: &str = "ads.example.com";

/// The second client. Per-client rules key on the peer address.
const STRICT_CLIENT: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

const IP_LITERAL_HOST: &str = "192.0.2.1";

const UNINTERCEPTED_PORT: u16 = 8080;

const PAGE_BODY: &str = "<html><head><script src=\"http://ads.example.com/track.js\">\
                         </script></head><body>shop</body></html>";
const SCRIPT_BODY: &str = "/* tracker */";

const TEST_BUDGET: Duration = Duration::from_secs(60);

/// The page, the ad script it references, and the stricter client's blocked
/// fetch — what this test drives through the proxy, and so what telemetry must
/// have counted before the assertions below mean anything.
const EXPECTED_HTTP_REQUESTS: u64 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_binary_proxies_filters_and_reports_http() {
    let started = Instant::now();

    let Some(origin) = bind_origin().await else {
        eprintln!(
            "SKIPPED: 127.0.0.1:{ORIGIN_PORT} is unavailable, and the egress guard \
             permits no other origin port. Free port {ORIGIN_PORT} to run this test."
        );
        return;
    };
    tokio::spawn(run_origin(origin));

    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");

    // Every name resolves to the loopback origin, so the proxy's own resolve
    // step is exercised without leaving the machine.
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream_answering(upstream, Ipv4Addr::LOCALHOST));

    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");

    let (_child, ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports, upstream_addr)
    })
    .await;
    let proxy_port = ports.http();

    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();

    let mut socket = connect_events(&base, &key).await;

    // `$client` scopes the second rule to one peer; the first applies to
    // everyone. Both go in as user rules so the test needs no rule list.
    put_user_rules(
        &http,
        &base,
        &key,
        &["||ads.example.com^", "||shop.example.com^$client=127.0.0.2"],
    )
    .await;

    // ── the ordinary client ──

    let page = fetch(None, proxy_port, PAGE_HOST, "/index.html").await;
    assert_eq!(
        page.status, 200,
        "the page host is not blocked for this client"
    );
    assert!(
        page.body.contains("<html"),
        "the page must be relayed from the origin intact, got: {}",
        page.body
    );

    let script = fetch(None, proxy_port, AD_HOST, "/track.js").await;
    assert_eq!(
        script.status, 200,
        "a blocked script is an empty success, not an error — breaking the page \
         is worse than the ad (fah-http::block)"
    );
    assert!(
        script.body.is_empty(),
        "a blocked script must have an empty body, got: {}",
        script.body
    );
    assert!(
        !script.body.contains("tracker"),
        "the origin must never have been reached"
    );

    // ── the stricter client ──

    let blocked_page = fetch(Some(STRICT_CLIENT), proxy_port, PAGE_HOST, "/index.html").await;
    assert_eq!(
        blocked_page.status, 403,
        "a blocked document gets an explaining page, not a blank one"
    );
    assert!(
        !blocked_page.body.contains("shop</body>"),
        "the origin's page must not leak through a block"
    );

    // ── both verdicts are reported ──

    // Collected rather than awaited one at a time: the three events share one
    // stream in request order, so searching for the second would consume the
    // first.
    let events = collect_http_events(&mut socket, 3).await;
    let seen: Vec<(&str, &str)> = events
        .iter()
        .map(|event| {
            (
                event["data"]["domain"].as_str().unwrap_or_default(),
                event["data"]["verdict"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert!(
        seen.contains(&(AD_HOST, "block")),
        "the blocked ad script must reach the events socket: {seen:?}"
    );
    assert!(
        seen.contains(&(PAGE_HOST, "pass")),
        "the relayed page must reach the events socket: {seen:?}"
    );
    assert!(
        seen.contains(&(PAGE_HOST, "block")),
        "the stricter client's block must reach the events socket: {seen:?}"
    );

    // Per-request attribution (which client, which verdict) is proven on the
    // events socket above; what telemetry adds is that HTTP is counted as HTTP
    // and never folded into the DNS counters.
    let counters = await_http_counters(&http, &base, &key).await;
    assert!(
        counters.http.block >= 1,
        "the stricter client's blocked request must be counted: {counters:?}"
    );
    assert!(
        counters.http.response_bytes > 0,
        "the relayed page must be counted in bytes: {counters:?}"
    );

    assert!(
        started.elapsed() < TEST_BUDGET,
        "http end-to-end test took {:?}, over its {TEST_BUDGET:?} budget",
        started.elapsed()
    );
}

// ─── the binary under test ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_refusal_causes_reach_telemetry_on_their_own_fields() {
    let started = Instant::now();

    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");

    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream_answering(upstream, Ipv4Addr::LOCALHOST));

    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");

    let (_child, ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports, upstream_addr)
    })
    .await;
    let proxy_port = ports.http();

    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();

    let ip_literal = fetch(None, proxy_port, IP_LITERAL_HOST, "/index.html").await;
    assert_eq!(
        ip_literal.status, 403,
        "a bare-IP Host is refused on the request line, before any resolution"
    );

    let wrong_port = fetch(
        None,
        proxy_port,
        &format!("{PAGE_HOST}:{UNINTERCEPTED_PORT}"),
        "/index.html",
    )
    .await;
    assert_eq!(
        wrong_port.status, 403,
        "the name resolves to 127.0.0.1, which `allow_destinations` permits, so \
         only the egress guard's port rule can have refused this"
    );

    let counters = await_refusal_counters(&http, &base, &key).await;
    assert_eq!(
        (
            counters.http.refused_claim,
            counters.http.refused_destination
        ),
        (1, 1),
        "each cause must reach /telemetry on its own field, neither summed nor \
         swapped: one bare-IP Host, one unintercepted destination: {counters:?}"
    );
    assert_eq!(
        counters.http.block, 0,
        "no rule list is loaded, so neither refusal may be counted as a \
         filtering block: {counters:?}"
    );
    assert_eq!(
        counters.http.pass, 1,
        "characterizing today's behaviour, not endorsing it: the egress refusal \
         runs after the verdict and emits that Pass event, so the request is in \
         `pass` as well as in `refused_destination`. The claim refusal returns \
         before the verdict and stays out of both: {counters:?}"
    );
    assert_eq!(
        counters.http.response_bytes, 0,
        "a refused request must ship nothing downstream: {counters:?}"
    );

    assert!(
        started.elapsed() < TEST_BUDGET,
        "refusal telemetry test took {:?}, over its {TEST_BUDGET:?} budget",
        started.elapsed()
    );
}

fn config_toml(ports: &Ports, upstream: SocketAddr) -> String {
    let dns_port = ports.dns;
    let dot_port = ports.dot;
    let api_port = ports.api;
    let http_port = ports.http();
    // `allow_destinations` names loopback because the origin is in this
    // process. On the router this list stays empty — that is the default-deny
    // the egress guard exists for (CONFIGURATION.md §Egress).
    format!(
        r#"
[engine]
mode = "dns+http"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}
dot_port = {dot_port}

[dns.blocking]
mode = "null_ip"
ttl_seconds = 10

[dns.upstreams]
strategy = "adaptive"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "{upstream}"
protocol = "udp"

[http.listen]
address = "127.0.0.1"
port = {http_port}

[egress]
allow_destinations = ["127.0.0.1"]

[rules]
refresh_hours_default = 24
lists = []

[stats]
snapshot_interval_seconds = 300

[api]
address = "127.0.0.1"
port = {api_port}
tls = true


[log]
level = "warn"
format = "text"
"#
    )
}

// ─── the mock origin ────────────────────────────────────────────────────

/// `None` when the port is taken or forbidden — the caller skips rather than
/// failing, since neither is a defect in the product.
async fn bind_origin() -> Option<TcpListener> {
    match TcpListener::bind((Ipv4Addr::LOCALHOST, ORIGIN_PORT)).await {
        Ok(listener) => Some(listener),
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::AddrInUse | ErrorKind::PermissionDenied
            ) =>
        {
            None
        }
        Err(err) => panic!("binding the mock origin: {err}"),
    }
}

/// Raw HTTP/1.1 rather than hyper: the responses are fixed, and this keeps the
/// binary's test dependencies to what `e2e.rs` already needs.
async fn run_origin(listener: TcpListener) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        tokio::spawn(async move {
            let Some(head) = read_head(&mut stream).await else {
                return;
            };
            let body = if head.contains("/track.js") {
                SCRIPT_BODY
            } else {
                PAGE_BODY
            };
            let content_type = if head.contains("/track.js") {
                "application/javascript"
            } else {
                "text/html"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}

// ─── the clients ────────────────────────────────────────────────────────

struct HttpResponse {
    status: u16,
    body: String,
}

/// One request through the proxy, optionally from a chosen local address so
/// per-client rules have something to key on.
async fn fetch(from: Option<Ipv4Addr>, proxy_port: u16, host: &str, path: &str) -> HttpResponse {
    let socket = TcpSocket::new_v4().expect("client socket");
    if let Some(address) = from {
        socket
            .bind(SocketAddr::from((address, 0)))
            .unwrap_or_else(|err| panic!("binding a client on {address}: {err}"));
    }
    let mut stream = socket
        .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, proxy_port)))
        .await
        .expect("connect to the proxy");

    // `Connection: close` so the read below ends at EOF rather than on a
    // keep-alive timeout.
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\n\
         Accept: */*\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("send request");

    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .unwrap_or_else(|_| panic!("no response for {host}{path} within 10s"))
        .expect("read response");

    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response for {host}{path}: {text}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in: {head}"));

    HttpResponse {
        status,
        body: body.to_string(),
    }
}

async fn read_head(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            return Some(String::from_utf8_lossy(&buffer).into_owned());
        }
    }
}

// ─── reporting ──────────────────────────────────────────────────────────

/// Reads the events socket until `wanted` HTTP query events have arrived,
/// skipping DNS events and the periodic `stats` pushes that share the socket.
async fn collect_http_events<S>(socket: &mut S, wanted: usize) -> Vec<Value>
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut events = Vec::with_capacity(wanted);
    while events.len() < wanted {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "only {} of {wanted} http events arrived: {events:?}",
            events.len()
        );

        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .unwrap_or_else(|_| panic!("only {} of {wanted} http events arrived", events.len()))
            .expect("the socket closed early")
            .expect("socket error");

        let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(&text).expect("event json");
        if json["type"] == "query" && json["data"]["kind"] == "http" {
            events.push(json);
        }
    }
    events
}

/// The counters are fed by the same fan-out task as statistics, so they land a
/// beat after the response — poll rather than sleep on a guessed delay.
///
/// Deserialized into `fah_model::EngineCounters` rather than indexed as a
/// `Value`: a renamed field then fails to compile instead of silently reading
/// as absent.
async fn await_refusal_counters(
    client: &reqwest::Client,
    base: &str,
    key: &str,
) -> fah_model::EngineCounters {
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let body = get_json(client, base, key, "/api/v1/telemetry").await;
        let counters: fah_model::EngineCounters =
            serde_json::from_value(body["counters"].clone()).expect("counters block");
        let http = &counters.http;
        if http.refused_claim >= 1 && http.refused_destination >= 1 {
            return counters;
        }
        assert!(
            Instant::now() < deadline,
            "telemetry never published both refusal causes: {counters:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn await_http_counters(
    client: &reqwest::Client,
    base: &str,
    key: &str,
) -> fah_model::EngineCounters {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let body = get_json(client, base, key, "/api/v1/telemetry").await;
        let counters: fah_model::EngineCounters =
            serde_json::from_value(body["counters"].clone()).expect("counters block");
        let http = &counters.http;
        if http.pass + http.allow + http.block >= EXPECTED_HTTP_REQUESTS {
            return counters;
        }
        assert!(
            Instant::now() < deadline,
            "telemetry never counted all {EXPECTED_HTTP_REQUESTS} http requests: {counters:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
