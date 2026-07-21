//! End-to-end proof that the shipped binary is the product (p1-10).
//!
//! Every other test in this workspace exercises a crate, or the assembly of a
//! few of them in-process. This one spawns the real `fastadhunter` executable
//! against tempdir `/config` and `/data` volumes and a mock upstream, then
//! drives it the way an operator and a client would: resolve a name over UDP
//! DNS, read the counters back over HTTPS, watch the same query arrive on the
//! WebSocket, change the ruleset through the API and see the verdict flip
//! without a restart, and rotate the API key.
//!
//! Fully offline: the only upstream is a mock UDP resolver in this process,
//! and the config ships zero rule lists so nothing is ever fetched.
//!
//! It runs as one `#[tokio::test]` rather than several, deliberately — booting
//! the binary costs seconds, and the interesting assertions are about state
//! accumulated in a single running instance (a query is visible in stats
//! *because* it was resolved earlier). Splitting it would mean either booting
//! repeatedly or sharing mutable state between tests that cargo runs in
//! parallel.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, RecordType};
use serde_json::Value;
use tokio::net::UdpSocket;

/// What the mock upstream answers for anything it is asked.
const UPSTREAM_IP: Ipv4Addr = Ipv4Addr::new(93, 184, 216, 34);

/// `[dns.blocking] ttl_seconds` in the generated config — asserted on the
/// synthesized blocked answer.
const BLOCK_TTL: u32 = 10;

/// Ceiling for the whole test. The acceptance criterion is <= 60s; every wait
/// inside is individually bounded well below this, so blowing it means
/// something hung rather than something being slow.
const TEST_BUDGET: Duration = Duration::from_secs(60);

/// How many times to re-pick ports when another process claims the one we
/// just released. Three losses in a row is a machine problem, not a race.
const BOOT_ATTEMPTS: u32 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_binary_blocks_resolves_reports_and_reconfigures_live() {
    let started = Instant::now();

    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");

    // ── mock upstream ──
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream(upstream));

    let http = reqwest::Client::builder()
        // The appliance certificate is self-signed by design (SECURITY.md);
        // this is curl's `-k`.
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");

    // ── boot the real binary ──
    let (_child, dns_port, base) =
        boot(config_dir.path(), data_dir.path(), upstream_addr, &http).await;

    // The key is generated on first boot and persisted to `/config` — read it
    // the way an operator who missed the log line would.
    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();
    assert_eq!(key.len(), 64, "expected a 256-bit hex key");

    // ── the live event stream, connected before any traffic ──
    let mut socket = connect_events(&base, &key).await;

    // ── a blocked domain ──
    put_user_rules(&http, &base, &key, &["||ads.example.com^"]).await;

    let answer = resolve(dns_port, "ads.example.com").await;
    assert_eq!(
        answer.rcode,
        ResponseCode::NoError,
        "a blocked query is answered, not refused (CONFIGURATION.md: mode = null_ip)"
    );
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "a blocked query must be answered with the null IP"
    );
    assert_eq!(
        answer.ttl,
        Some(BLOCK_TTL),
        "the synthesized answer must carry [dns.blocking] ttl_seconds"
    );

    // ── an allowed domain, forwarded to the mock upstream ──
    let answer = resolve(dns_port, "allowed.example.com").await;
    assert_eq!(answer.rcode, ResponseCode::NoError);
    assert_eq!(
        answer.a_records,
        vec![UPSTREAM_IP],
        "an unblocked query must carry the upstream's answer through"
    );

    // ── the same queries, seen on the WebSocket ──
    let event = await_query_event(&mut socket, "ads.example.com").await;
    assert_eq!(event["data"]["verdict"], "block");
    assert_eq!(
        event["data"]["list"], "user-rules",
        "the decisive rule's list must be attributed"
    );
    assert_eq!(event["data"]["rule"], "||ads.example.com^");

    // ── and counted in the statistics ──
    let stats = await_stats(&http, &base, &key, |stats| {
        stats["queries_total"].as_u64().unwrap_or(0) >= 2
            && stats["blocked_total"].as_u64().unwrap_or(0) >= 1
    })
    .await;
    let blocked_domains: Vec<&str> = stats["top_blocked_domains"]
        .as_array()
        .expect("top_blocked_domains")
        .iter()
        .filter_map(|entry| entry["domain"].as_str())
        .collect();
    assert!(
        blocked_domains.contains(&"ads.example.com"),
        "the blocked domain must appear in top_blocked_domains, got {blocked_domains:?}"
    );

    // ── the query log ──
    let queries: Value = get_json(&http, &base, &key, "/api/v1/queries?limit=100").await;
    let logged: Vec<&str> = queries["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["domain"].as_str())
        .collect();
    assert!(
        logged.contains(&"allowed.example.com"),
        "the forwarded query must be in the query log, got {logged:?}"
    );

    // ── the cache, inspected and cleaned through the admin API ──
    // A repeat of the forwarded query is served by the cache, and the real
    // cache's counters must reach the API through the binary's CacheSource
    // adapter — fah-api's own tests fake that source, so only this test
    // proves the wiring.
    let answer = resolve(dns_port, "allowed.example.com").await;
    assert_eq!(answer.a_records, vec![UPSTREAM_IP]);

    let cache: Value = get_json(&http, &base, &key, "/api/v1/cache").await;
    assert!(
        cache["entries"].as_u64().expect("entries") >= 1,
        "the forwarded answer must have been cached, got {cache}"
    );
    assert!(
        cache["hits"].as_u64().expect("hits") >= 1,
        "the repeated query must have been a cache hit, got {cache}"
    );
    assert_eq!(
        cache["fresh"], cache["entries"],
        "seconds-old entries are all still fresh, got {cache}"
    );

    let clean: Value = post_json(&http, &base, &key, "/api/v1/cache/clean").await;
    assert_eq!(
        clean["removed_expired"], 0,
        "nothing has had time to expire, got {clean}"
    );
    assert_eq!(
        clean["removed_stale"], 0,
        "stale entries are kept by default — and none exist yet, got {clean}"
    );
    assert_eq!(
        clean["entries_after"], cache["entries"],
        "a clean of an all-fresh cache removes nothing"
    );

    let memory: Value = get_json(&http, &base, &key, "/api/v1/debug/memory").await;
    assert!(
        memory["ruleset_bytes"].as_u64().expect("ruleset_bytes") > 0,
        "one user rule still compiles to a non-empty ruleset"
    );
    assert!(
        memory["cache_estimated_bytes"]
            .as_u64()
            .expect("cache_estimated_bytes")
            > 0,
        "a populated cache must estimate above zero bytes"
    );
    assert!(
        memory
            .as_object()
            .expect("memory object")
            .contains_key("process_rss"),
        "process_rss must be present even when null off-Linux"
    );

    // ── a verdict that flips live, with no restart ──
    let answer = resolve(dns_port, "flip.example.net").await;
    assert_eq!(
        answer.a_records,
        vec![UPSTREAM_IP],
        "flip.example.net starts out unblocked"
    );

    put_user_rules(
        &http,
        &base,
        &key,
        &["||ads.example.com^", "||flip.example.net^"],
    )
    .await;

    let answer = resolve(dns_port, "flip.example.net").await;
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "the new rule must apply to the running engine — and beat the cached \
         upstream answer, because the Rule Engine runs before the cache (ADR-0001)"
    );

    // ── key rotation invalidates the old key immediately ──
    let rotated: Value = post_json(&http, &base, &key, "/api/v1/config/apikey/rotate").await;
    let new_key = rotated["api_key"].as_str().expect("api_key").to_string();
    assert_ne!(new_key, key);

    let status = http
        .get(format!("{base}/api/v1/stats"))
        .bearer_auth(&key)
        .send()
        .await
        .expect("request with the old key")
        .status();
    assert_eq!(
        status, 401,
        "the rotated-away key must stop working at once"
    );

    let status = http
        .get(format!("{base}/api/v1/stats"))
        .bearer_auth(&new_key)
        .send()
        .await
        .expect("request with the new key")
        .status();
    assert_eq!(status, 200, "the new key must work immediately");

    assert!(
        started.elapsed() < TEST_BUDGET,
        "end-to-end test took {:?}, over its {TEST_BUDGET:?} budget",
        started.elapsed()
    );
}

// ─── the binary under test ──────────────────────────────────────────────

/// Kills the child on the way out — including on a panicking assertion, which
/// would otherwise leave a process holding the DNS and API ports.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn config_toml(dns_port: u16, api_port: u16, upstream: SocketAddr) -> String {
    // Loopback everywhere and zero rule lists: nothing in this test may reach
    // the network, and an empty `lists` keeps the refresh scheduler idle.
    format!(
        r#"
[engine]
mode = "dns"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}

[dns.blocking]
mode = "null_ip"
ttl_seconds = {BLOCK_TTL}

[dns.upstreams]
strategy = "fallback"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "{upstream}"
protocol = "udp"

[rules]
refresh_hours_default = 24
lists = []

[query_log]
enabled = true

[stats]
snapshot_interval_seconds = 300

[api]
address = "127.0.0.1"
port = {api_port}
tls = true
metrics_public = true

[log]
level = "warn"
format = "text"
"#
    )
}

/// Boots the binary on freshly-picked ports and waits for it to serve.
///
/// Ports are chosen by binding an ephemeral socket and releasing it, which is
/// inherently racy: between the release and the engine's own `bind`, anything
/// else on the machine can claim the port — and under `cargo test --workspace`
/// there are dozens of other tests binding ephemeral sockets in parallel. That
/// race made this test flaky, so a lost race is retried with fresh ports
/// instead of being reported as a product failure. Every *other* startup
/// failure still fails immediately, with the engine's log attached — masking
/// those is exactly what this must not do.
async fn boot(
    config_dir: &Path,
    data_dir: &Path,
    upstream: SocketAddr,
    client: &reqwest::Client,
) -> (Guard, u16, String) {
    for attempt in 1..=BOOT_ATTEMPTS {
        let dns_port = free_udp_port();
        let api_port = free_tcp_port();

        let config_path = config_dir.join("fastadhunter.toml");
        std::fs::write(&config_path, config_toml(dns_port, api_port, upstream))
            .expect("write config");

        // The engine's output goes to a file rather than the terminal: a
        // failing assertion should be readable, not buried in the log — but
        // when the binary dies during startup, that log is the only thing that
        // can say why, so it has to be kept somewhere retrievable.
        let log_path = config_dir.join("engine.log");
        let log = std::fs::File::create(&log_path).expect("engine log");
        let child = Guard(
            Command::new(env!("CARGO_BIN_EXE_fastadhunter"))
                .arg("--config")
                .arg(&config_path)
                .arg("--data")
                .arg(data_dir)
                .stdout(Stdio::from(log.try_clone().expect("clone log handle")))
                .stderr(Stdio::from(log))
                .spawn()
                .expect("spawn fastadhunter"),
        );

        let base = format!("https://127.0.0.1:{api_port}");
        match await_api_ready(client, &base, child, &log_path).await {
            Ok(child) => return (child, dns_port, base),
            // Dropping the guard on the way out of the match killed the child.
            Err(log) if is_port_conflict(&log) && attempt < BOOT_ATTEMPTS => continue,
            Err(log) => panic!(
                "fastadhunter failed to start (attempt {attempt}/{BOOT_ATTEMPTS})\
                 \n--- engine log ---\n{log}"
            ),
        }
    }
    unreachable!("the loop either returns or panics on the last attempt")
}

/// Polls the public health endpoint until the API answers. Returns the child
/// on success, or the engine's log on failure — a dead child would otherwise
/// show up as an opaque connection-refused timeout.
async fn await_api_ready(
    client: &reqwest::Client,
    base: &str,
    mut child: Guard,
    log_path: &Path,
) -> Result<Guard, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.0.try_wait() {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!("exited with {status}\n{log}"));
        }
        if let Ok(response) = client.get(format!("{base}/health")).send().await {
            if response.status().is_success() {
                let body: Value = response.json().await.expect("health json");
                assert_eq!(body["status"], "ok");
                return Ok(child);
            }
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!("never became ready at {base}\n{log}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Whether a startup failure was somebody else taking the port we picked,
/// rather than a defect worth failing the test over.
fn is_port_conflict(log: &str) -> bool {
    let log = log.to_ascii_lowercase();
    // Windows WSAEADDRINUSE, Linux EADDRINUSE, and both platforms' prose.
    [
        "10048",
        "os error 98",
        "address already in use",
        "socket address",
    ]
    .iter()
    .any(|needle| log.contains(needle))
}

// ─── DNS ────────────────────────────────────────────────────────────────

/// Answers every query with a single A record. Stands in for a real resolver
/// so the test never leaves the machine (PERFORMANCE.md's "in-engine latency
/// excludes upstream RTT" applies here too: this upstream is instant).
async fn run_mock_upstream(socket: UdpSocket) {
    let mut buffer = vec![0u8; 4096];
    loop {
        let Ok((len, from)) = socket.recv_from(&mut buffer).await else {
            return;
        };
        let Ok(request) = Message::from_vec(&buffer[..len]) else {
            continue;
        };
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        if let Some(query) = request.queries.first() {
            response.add_query(query.clone());
            response.add_answer(hickory_proto::rr::Record::from_rdata(
                query.name().clone(),
                300,
                RData::A(A(UPSTREAM_IP)),
            ));
        }
        let Ok(bytes) = response.to_vec() else {
            continue;
        };
        let _ = socket.send_to(&bytes, from).await;
    }
}

struct Answer {
    rcode: ResponseCode,
    a_records: Vec<Ipv4Addr>,
    ttl: Option<u32>,
}

/// One real UDP query against the binary's listener, the way any client on the
/// LAN would ask.
async fn resolve(port: u16, domain: &str) -> Answer {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client socket");
    socket
        .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .expect("connect to the DNS listener");

    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(&format!("{domain}.")).expect("valid name"),
        RecordType::A,
    ));
    let request = message.to_vec().expect("encodable query");

    socket.send(&request).await.expect("send query");

    let mut buffer = vec![0u8; 4096];
    let len = tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buffer))
        .await
        .unwrap_or_else(|_| panic!("no DNS response for {domain} within 5s"))
        .expect("receive response");

    let response = Message::from_vec(&buffer[..len]).expect("decodable response");
    Answer {
        rcode: response.metadata.response_code,
        a_records: response
            .answers
            .iter()
            .filter_map(|record| match record.data {
                RData::A(a) => Some(a.0),
                _ => None,
            })
            .collect(),
        ttl: response.answers.first().map(|record| record.ttl),
    }
}

// ─── API ────────────────────────────────────────────────────────────────

async fn get_json(client: &reqwest::Client, base: &str, key: &str, path: &str) -> Value {
    let response = client
        .get(format!("{base}{path}"))
        .bearer_auth(key)
        .send()
        .await
        .unwrap_or_else(|err| panic!("GET {path}: {err}"));
    assert!(
        response.status().is_success(),
        "GET {path} returned {}",
        response.status()
    );
    response.json().await.expect("json body")
}

async fn post_json(client: &reqwest::Client, base: &str, key: &str, path: &str) -> Value {
    let response = client
        .post(format!("{base}{path}"))
        .bearer_auth(key)
        .send()
        .await
        .unwrap_or_else(|err| panic!("POST {path}: {err}"));
    assert!(
        response.status().is_success(),
        "POST {path} returned {}",
        response.status()
    );
    response.json().await.expect("json body")
}

/// Replaces the user-rules block and waits for the API to confirm — the
/// handler recompiles the ruleset before responding, so a 200 here means the
/// next query already sees the new verdict.
async fn put_user_rules(client: &reqwest::Client, base: &str, key: &str, rules: &[&str]) {
    let response = client
        .put(format!("{base}/api/v1/rules/user"))
        .bearer_auth(key)
        .json(&serde_json::json!({ "rules": rules }))
        .send()
        .await
        .expect("PUT /api/v1/rules/user");
    assert!(
        response.status().is_success(),
        "PUT /api/v1/rules/user returned {}",
        response.status()
    );
}

/// Statistics are fed by the binary's event fan-out task, so they land a beat
/// after the query is answered — poll rather than sleep on a guessed delay.
async fn await_stats<F>(client: &reqwest::Client, base: &str, key: &str, ready: F) -> Value
where
    F: Fn(&Value) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stats = get_json(client, base, key, "/api/v1/stats").await;
        if ready(&stats) {
            return stats;
        }
        assert!(
            Instant::now() < deadline,
            "statistics never caught up with the queries: {stats}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ─── WebSocket ──────────────────────────────────────────────────────────

async fn connect_events(
    base: &str,
    key: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!(
        "{}/api/v1/events?token={key}",
        base.replace("https://", "wss://")
    );
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    let (socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
            .await
            .expect("the events socket must accept a ?token= upgrade");
    socket
}

/// Reads the stream until the query for `domain` shows up, skipping the
/// periodic `stats` pushes that share the socket.
async fn await_query_event<S>(socket: &mut S, domain: &str) -> Value
where
    S: StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "no `query` event for {domain} arrived on the events socket"
        );

        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .unwrap_or_else(|_| {
                panic!("no `query` event for {domain} arrived on the events socket")
            })
            .expect("the socket closed early")
            .expect("socket error");

        let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(&text).expect("event json");
        if json["type"] == "query" && json["data"]["domain"] == domain {
            return json;
        }
    }
}

/// A rustls client that accepts the self-signed appliance certificate —
/// tungstenite has no `danger_accept_invalid_certs` switch, so the
/// verification bypass is spelled out here. Test-only.
fn insecure_client_config() -> rustls::ClientConfig {
    fah_api::install_crypto_provider();

    #[derive(Debug)]
    struct AcceptAny;

    impl rustls::client::danger::ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny))
        .with_no_client_auth()
}

// ─── ports ──────────────────────────────────────────────────────────────

/// Asks the OS for a free port by binding and releasing it. Inherently racy —
/// but the alternative is hard-coding ports, which collides with whatever the
/// developer is already running (an AdGuard Home on 53, say). The window is
/// microseconds and the test fails loudly rather than silently if it loses.
fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .expect("bind an ephemeral UDP port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral TCP port")
        .local_addr()
        .expect("local addr")
        .port()
}
