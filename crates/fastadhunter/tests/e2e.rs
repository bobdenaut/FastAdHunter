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

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use hickory_proto::op::ResponseCode;
use serde_json::Value;
use tokio::net::UdpSocket;

use common::{
    await_stats, boot, client_config_trusting, connect_events, get_json, insecure_client_config,
    post_json, put_user_rules, resolve, resolve_doh_get, resolve_doh_post, resolve_dot,
    run_mock_upstream, Ports, UPSTREAM_IP,
};

const DOT_HOSTNAME: &str = "dns.fah.test";

/// `[dns.blocking] ttl_seconds` in the generated config — asserted on the
/// synthesized blocked answer.
const BLOCK_TTL: u32 = 10;

/// Ceiling for the whole test. The acceptance criterion is <= 60s; every wait
/// inside is individually bounded well below this, so blowing it means
/// something hung rather than something being slow.
const TEST_BUDGET: Duration = Duration::from_secs(60);

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
    let (_child, ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports, upstream_addr)
    })
    .await;
    let dns_port = ports.dns;

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
    assert_eq!(event["data"]["transport"], "udp");

    let insecure = Arc::new(insecure_client_config());
    let answer = resolve_dot(
        ports.dot,
        "ads.example.com",
        Arc::clone(&insecure),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "the same domain must be blocked over DoT exactly as over UDP"
    );
    let event = await_query_event(&mut socket, "ads.example.com").await;
    assert_eq!(event["data"]["transport"], "dot");
    assert_eq!(event["data"]["verdict"], "block");
    let answer = resolve_dot(
        ports.dot,
        "allowed.example.com",
        Arc::clone(&insecure),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(answer.a_records, vec![UPSTREAM_IP]);

    let answer = resolve_doh_post(&http, &base, "ads.example.com").await;
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "the same domain must be blocked over DoH exactly as over UDP"
    );
    let event = await_query_event(&mut socket, "ads.example.com").await;
    assert_eq!(event["data"]["transport"], "doh");
    assert_eq!(event["data"]["verdict"], "block");
    let answer = resolve_doh_get(&http, &base, "allowed.example.com").await;
    assert_eq!(answer.a_records, vec![UPSTREAM_IP]);

    let created = http
        .post(format!("{base}/api/v1/policies"))
        .bearer_auth(&key)
        .json(&serde_json::json!({
            "id": "phone",
            "assignments": [{ "client": "127.0.0.1" }]
        }))
        .send()
        .await
        .expect("create a policy assigned to the test client");
    assert!(
        created.status().is_success(),
        "POST /api/v1/policies returned {}",
        created.status()
    );
    let answer = resolve_dot(ports.dot, "ads.example.com", insecure, DOT_HOSTNAME).await;
    assert_eq!(answer.a_records, vec![Ipv4Addr::UNSPECIFIED]);
    let answer = resolve_doh_post(&http, &base, "ads.example.com").await;
    assert_eq!(answer.a_records, vec![Ipv4Addr::UNSPECIFIED]);
    await_stats(&http, &base, &key, |stats| {
        stats["policies"].as_array().is_some_and(|policies| {
            policies.iter().any(|entry| {
                entry["policy"] == "phone"
                    && entry["queries"].as_u64().unwrap_or(0) >= 2
                    && entry["blocked"].as_u64().unwrap_or(0) >= 2
            })
        })
    })
    .await;

    let generated = http
        .post(format!("{base}/api/v1/certificates/ca/generate"))
        .bearer_auth(&key)
        .json(&serde_json::json!({ "confirm": true }))
        .send()
        .await
        .expect("generate a CA");
    assert_eq!(generated.status(), 200);
    let ca_der = http
        .get(format!("{base}/api/v1/certificates/ca/export?format=der"))
        .bearer_auth(&key)
        .send()
        .await
        .expect("export the CA")
        .bytes()
        .await
        .expect("CA DER")
        .to_vec();
    let answer = resolve_dot(
        ports.dot,
        "allowed.example.com",
        client_config_trusting(ca_der),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(
        answer.a_records,
        vec![UPSTREAM_IP],
        "a client trusting only the exported CA must validate the DoT leaf minted for its SNI"
    );

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

    // ── telemetry, reflecting the traffic just driven through ──
    // fah-api's own tests fake the telemetry port, so only this test proves the
    // registry actually reaches the endpoint through the binary's adapter.
    //
    // The engine blocks deserialize into their `fah_model` types rather than
    // being indexed as a `Value`: a renamed field then fails to compile instead
    // of reading as silently absent.
    let body: Value = get_json(&http, &base, &key, "/api/v1/telemetry").await;
    let engine: fah_model::EngineTelemetry =
        serde_json::from_value(body.clone()).expect("the engine blocks of /telemetry");

    assert!(
        engine.counters.dns.block >= 1,
        "the blocked query must be counted, got {:?}",
        engine.counters
    );
    assert!(
        engine.latency.dns.forward.count >= 1,
        "the forwarded query must have been timed, got {:?}",
        engine.latency
    );
    assert_eq!(
        engine.upstreams.len(),
        1,
        "the one configured upstream must be reported, got {:?}",
        engine.upstreams
    );
    assert_eq!(engine.upstreams[0].protocol, fah_model::Protocol::Udp);
    assert!(
        body["process"]["uptime_seconds"].is_u64(),
        "counters are lifetime-cumulative, so uptime must ship beside them"
    );
    // Key-for-key, not value-for-value: `fresh`/`stale`/`expired` are a walk
    // against `now`, so two reads a round trip apart legitimately differ once a
    // TTL boundary falls between them. Field-set equality is what proves both
    // render through the same `From<CacheStats>`; `api.rs` asserts the values
    // against a frozen fake, where equality is actually decidable.
    let cache = get_json(&http, &base, &key, "/api/v1/cache").await;
    assert_eq!(
        sorted_keys(&body["cache"]),
        sorted_keys(&cache),
        "the telemetry cache block and /cache derive from one snapshot"
    );
    assert_eq!(
        body["cache"]["capacity"], cache["capacity"],
        "config-derived bounds cannot drift between two reads"
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

/// A JSON object's field names, sorted — for comparing shape without comparing
/// values that legitimately move between two reads.
fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort_unstable();
    keys
}

// ─── the binary under test ──────────────────────────────────────────────

fn config_toml(ports: &Ports, upstream: SocketAddr) -> String {
    let dns_port = ports.dns;
    let dot_port = ports.dot;
    let api_port = ports.api;
    // Loopback everywhere and zero rule lists: nothing in this test may reach
    // the network, and an empty `lists` keeps the refresh scheduler idle.
    format!(
        r#"
[engine]
mode = "dns"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}
dot_port = {dot_port}

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

// ─── WebSocket ──────────────────────────────────────────────────────────

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
