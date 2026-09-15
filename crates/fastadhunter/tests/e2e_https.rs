mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hickory_proto::op::ResponseCode;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use common::{
    await_event, bind_origin, boot_full, client_config_trusting, client_hello_without_sni,
    connect_events, get_json, insecure_client_config, put_user_rules, raw_probe, resolve,
    resolve_doh_post, resolve_dot, run_tls_http_origin, self_signed_origin, skip_origin_message,
    skips_allowed, tls_connect_from, FullMode, AD_HOST, ALLOW_SKIP_ENV, DOMAIN_LANE_LOG,
    DOT_HOSTNAME, FULL_MODE_HTTP_RUNTIMES, PAGE_HOST,
};

const ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 30);
const LIVE_APPLY_ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 31);
const ORIGIN_PAGE: &[u8] = b"<html>origin page</html>\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_mode_blocks_at_every_layer() {
    let started = Instant::now();
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST, AD_HOST]);
    let origin_page = ORIGIN_PAGE.repeat(40);
    let origin = match bind_origin(ORIGIN_IP).await {
        Some(listener) => Some(run_tls_http_origin(
            listener,
            origin_cert.clone(),
            origin_key,
            Arc::new(origin_page.clone()),
        )),
        None => {
            eprintln!(
                "{} The intercepted leg degrades to the fail-closed path.",
                skip_origin_message(ORIGIN_IP)
            );
            None
        }
    };
    let url_judge_inside_tls = origin.is_some() && cfg!(feature = "test-harness");

    let instance = boot_full(FullMode {
        origin_ip: ORIGIN_IP,
        clients: vec!["127.0.0.1".to_string()],
        api_tls: true,
        upstream_root: Some(origin_cert.to_vec()),
        https_limits: None,
    })
    .await;
    let log = instance.engine_log();
    assert!(
        log.contains(DOMAIN_LANE_LOG),
        "0/7 lane: with runtime.http_runtimes = {FULL_MODE_HTTP_RUNTIMES} the HTTPS \
         listener must feed the HTTP allocation domains, not the shared runtime\
         \n--- engine log ---\n{log}"
    );
    assert!(
        log.contains(&format!("http_runtimes={FULL_MODE_HTTP_RUNTIMES}")),
        "0/7 lane: the pinned domain count must be the one that started\
         \n--- engine log ---\n{log}"
    );

    let ca_der = instance.generate_ca().await;
    let mut events = connect_events(&instance.base, &instance.key).await;
    put_user_rules(
        &instance.http,
        &instance.base,
        &instance.key,
        &[&format!("||{AD_HOST}^"), &format!("||{PAGE_HOST}/track.js")],
    )
    .await;
    let https = instance.ports.https();

    let answer = resolve(instance.ports.dns, AD_HOST).await;
    assert_eq!(answer.rcode, ResponseCode::NoError);
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "1/7 DNS: the blocked domain is answered with the null IP over UDP"
    );

    let script = fetch_http(instance.ports.http(), PAGE_HOST, "/track.js").await;
    assert_eq!(
        script.0, 200,
        "2/7 HTTP: a blocked script collapses to an empty 200"
    );
    assert!(
        script.1.is_empty(),
        "2/7 HTTP: the blocked script body is empty, got {} bytes",
        script.1.len()
    );

    let sni_block =
        tls_connect_from(None, https, AD_HOST, Arc::new(insecure_client_config())).await;
    assert!(
        sni_block.is_err(),
        "3/7 SNI: a blocked SNI is closed before any ServerHello"
    );
    let event = await_event(&mut events, "https-sni block", |data| {
        data["kind"] == "https-sni" && data["domain"] == AD_HOST
    })
    .await;
    assert_eq!(event["verdict"], "block", "3/7 SNI: {event}");
    assert_eq!(event["bytes"], 0);

    let no_sni = raw_probe(https, &client_hello_without_sni()).await;
    assert!(
        no_sni.is_empty(),
        "4/7 no-SNI: a hello without SNI is closed, never answered: {} bytes",
        no_sni.len()
    );
    let event = await_event(&mut events, "https-sni no-SNI", |data| {
        data["kind"] == "https-sni" && data["domain"] == ""
    })
    .await;
    assert_eq!(
        event["verdict"], "pass",
        "4/7 no-SNI: [https.sni] no_sni = pass classifies it"
    );
    let still_serving =
        tls_connect_from(None, https, AD_HOST, Arc::new(insecure_client_config())).await;
    assert!(
        still_serving.is_err(),
        "4/7 no-SNI: the listener keeps judging after it"
    );
    await_event(&mut events, "https-sni block after no-SNI", |data| {
        data["kind"] == "https-sni" && data["domain"] == AD_HOST
    })
    .await;

    let intercepted = tls_connect_from(
        None,
        https,
        PAGE_HOST,
        client_config_trusting(ca_der.clone()),
    )
    .await;
    if url_judge_inside_tls {
        let origin = origin.as_ref().expect("checked above");
        let mut inside = intercepted.expect(
            "5/7 intercepted: the listed client, trusting only our CA, completes our \
             handshake — the binary verified the origin against the injected root and \
             minted a leaf for it",
        );
        let (status, body) = fetch_over(&mut inside, PAGE_HOST, "/track.js").await;
        assert_eq!(
            status, 200,
            "5/7 intercepted: the blocked URL inside TLS collapses to an empty 200"
        );
        assert!(
            body.is_empty(),
            "5/7 intercepted: the blocked script body is empty, got {} bytes",
            body.len()
        );
        let event = await_event(&mut events, "https block for the listed client", |data| {
            data["kind"] == "https" && data["domain"] == PAGE_HOST && data["path"] == "/track.js"
        })
        .await;
        assert_eq!(event["verdict"], "block", "5/7 intercepted: {event}");
        assert_eq!(event["client"], "127.0.0.1");
        let verified_before_block = origin.accepts.load(Ordering::Relaxed);
        assert!(
            verified_before_block >= 1,
            "5/7 intercepted: the binary contacted the origin to verify it before minting"
        );

        let mut inside = tls_connect_from(
            None,
            https,
            PAGE_HOST,
            client_config_trusting(ca_der.clone()),
        )
        .await
        .expect("5/7 intercepted: a second listed-client session completes our handshake");
        let (status, body) = fetch_over(&mut inside, PAGE_HOST, "/page").await;
        assert_eq!(
            status, 200,
            "5/7 intercepted: an allowed URL is answered by the origin through the terminate leg"
        );
        assert_eq!(
            body.as_bytes(),
            &origin_page[..],
            "5/7 intercepted: the origin body arrives byte-identical through the terminate leg"
        );
        let event = await_event(&mut events, "https pass for the listed client", |data| {
            data["kind"] == "https" && data["domain"] == PAGE_HOST && data["path"] == "/page"
        })
        .await;
        assert_eq!(event["verdict"], "pass", "5/7 intercepted: {event}");
        assert_eq!(event["status"], 200, "5/7 intercepted: {event}");
        assert!(
            origin.accepts.load(Ordering::Relaxed) > verified_before_block,
            "5/7 intercepted: the second session verified a fresh upstream"
        );
        let certificates = instance.certificates().await;
        assert_eq!(
            certificates["leaf_cache"]["minted_total"], 1,
            "5/7 intercepted: one leaf for {PAGE_HOST}, reused by the second session: \
             {certificates}"
        );
    } else {
        let reason = if origin.is_none() {
            "no origin bound"
        } else {
            "the binary was built without the `test-harness` feature, so no upstream root \
             could be injected — run `cargo test --all-features`"
        };
        assert!(
            skips_allowed(),
            "5/7 intercepted: {reason}. The URL judge inside TLS did not run; set \
             {ALLOW_SKIP_ENV}=1 to accept the fail-closed path only"
        );
        assert!(
            intercepted.is_err(),
            "5/7 intercepted (degraded): the terminate leg closes unanswered"
        );
        let event = await_event(&mut events, "https event for the listed client", |data| {
            data["kind"] == "https" && data["domain"] == PAGE_HOST
        })
        .await;
        let expected = if origin.is_some() { 526 } else { 0 };
        assert_eq!(
            event["status"], expected,
            "5/7 intercepted (degraded): fail-closed status: {event}"
        );
        eprintln!(
            "5/7 intercepted: DEGRADED — {reason}; only the fail-closed path ran, the URL judge \
             inside TLS was not exercised"
        );
    }

    let answer = resolve_dot(
        instance.ports.dot,
        AD_HOST,
        client_config_trusting(ca_der.clone()),
        DOT_HOSTNAME,
    )
    .await;
    assert_eq!(
        answer.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "6/7 DoT: the blocked domain is blocked over DoT, leaf minted for {DOT_HOSTNAME} \
         under the exported CA"
    );

    let answer = resolve_doh_post(&instance.http, &instance.base, "allowed.example.com").await;
    assert_eq!(
        answer.a_records,
        vec![ORIGIN_IP],
        "7/7 DoH: an allowed domain is forwarded over DoH"
    );
    let blocked = resolve_doh_post(&instance.http, &instance.base, AD_HOST).await;
    assert_eq!(
        blocked.a_records,
        vec![Ipv4Addr::UNSPECIFIED],
        "7/7 DoH: blocked over DoH"
    );

    let certificates = instance.certificates().await;
    let expected_leaves = if url_judge_inside_tls { 2 } else { 1 };
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], expected_leaves,
        "leaves: one for {PAGE_HOST} on the terminate leg, one for the DoT hostname"
    );
    assert_eq!(certificates["leaf_cache"]["unwarmed_misses"], 0);
    assert_eq!(
        certificates["dot"]["state"], "listening",
        "the DoT listener reports itself on the certificates document: {certificates}"
    );

    let non_tls = raw_probe(https, b"GET / HTTP/1.1\r\nHost: plain.invalid\r\n\r\n").await;
    assert!(
        non_tls.is_empty(),
        "plain HTTP on the HTTPS port is closed, never answered: {} bytes",
        non_tls.len()
    );

    let telemetry = get_json(
        &instance.http,
        &instance.base,
        &instance.key,
        "/api/v1/telemetry",
    )
    .await;
    assert_eq!(
        telemetry["listeners"]["https"]["non_tls"]
            .as_u64()
            .unwrap_or_default(),
        1,
        "https: the plain-HTTP probe is the one event only the HTTPS listener can produce — \
         the plain listener never reads a ClientHello: {}",
        telemetry["listeners"]["https"]
    );
    assert_eq!(
        telemetry["listeners"]["http"]["non_tls"]
            .as_u64()
            .unwrap_or_default(),
        0,
        "http: non_tls belongs to the HTTPS listener alone. A non-zero value here means the \
         two counter sets reached /api/v1/telemetry transposed: {}",
        telemetry["listeners"]["http"]
    );
    for listener in ["http", "https"] {
        let counters = &telemetry["listeners"][listener];
        let (connections, requests, blocked) = (
            counters["connections"].as_u64().unwrap_or_default(),
            counters["requests"].as_u64().unwrap_or_default(),
            counters["blocked"].as_u64().unwrap_or_default(),
        );
        assert!(
            connections >= 1 && requests >= 1 && blocked >= 1,
            "{listener}: the scenario drove at least one connection, request and block: {counters}"
        );
        assert!(
            blocked <= requests,
            "{listener}: every block is a judged request (p3-04 L5): {counters}"
        );
    }
    assert!(
        telemetry["listeners"]["https"]["connections"]
            .as_u64()
            .unwrap_or_default()
            > telemetry["listeners"]["https"]["requests"]
                .as_u64()
                .unwrap_or_default()
            || telemetry["listeners"]["https"]["requests"]
                .as_u64()
                .unwrap_or_default()
                > telemetry["listeners"]["https"]["connections"]
                    .as_u64()
                    .unwrap_or_default(),
        "https: connections and requests are different units — the no-SNI hello is a \
         connection with a verdict and the intercepted session carries several requests: {}",
        telemetry["listeners"]["https"]
    );

    let memory = get_json(
        &instance.http,
        &instance.base,
        &instance.key,
        "/api/v1/debug/memory",
    )
    .await;
    println!(
        "full-mode dev-box RSS after the scenario: /debug/memory process_rss={} \
         process_peak_rss={}; OS reading for pid {}: {} (test build — diagnostic only)",
        memory["process_rss"],
        memory["process_peak_rss"],
        instance.child.0.id(),
        child_rss(instance.child.0.id())
            .map(|bytes| format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0)))
            .unwrap_or_else(|| "unavailable".to_string())
    );

    println!(
        "full-mode scenario wall time: {:?} (diagnostic, not asserted)",
        started.elapsed()
    );
}

#[cfg(windows)]
fn child_rss(pid: u32) -> Option<u64> {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.contains(&format!("\"{pid}\"")))?;
    let field = line.rsplit("\",\"").next()?.trim_matches('"');
    let digits: String = field.chars().filter(char::is_ascii_digit).collect();
    digits.parse::<u64>().ok().map(|kib| kib * 1024)
}

#[cfg(not(windows))]
fn child_rss(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let digits: String = line.chars().filter(char::is_ascii_digit).collect();
    digits.parse::<u64>().ok().map(|kib| kib * 1024)
}

async fn fetch_http(proxy_port: u16, host: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, proxy_port)))
        .await
        .expect("connect to the HTTP proxy");
    fetch_over(&mut stream, host, path).await
}

async fn fetch_over<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    path: &str,
) -> (u16, String) {
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .expect("the proxy closes within 10 s")
        .ok();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("an HTTP head");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .expect("a status code");
    (status, body.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn listing_a_client_through_the_api_applies_on_the_next_connection() {
    let (origin_cert, origin_key) = self_signed_origin(&[PAGE_HOST]);
    let Some(listener) = bind_origin(LIVE_APPLY_ORIGIN_IP).await else {
        assert!(
            skips_allowed(),
            "{}. Set {ALLOW_SKIP_ENV}=1 to accept the skip",
            skip_origin_message(LIVE_APPLY_ORIGIN_IP)
        );
        eprintln!("{}", skip_origin_message(LIVE_APPLY_ORIGIN_IP));
        return;
    };
    let page = ORIGIN_PAGE.repeat(4);
    let _origin = run_tls_http_origin(
        listener,
        origin_cert.clone(),
        origin_key,
        Arc::new(page.clone()),
    );

    let instance = boot_full(FullMode {
        origin_ip: LIVE_APPLY_ORIGIN_IP,
        clients: Vec::new(),
        api_tls: true,
        upstream_root: Some(origin_cert.to_vec()),
        https_limits: None,
    })
    .await;
    let https = instance.ports.https();
    let ca_der = instance.generate_ca().await;

    assert_eq!(
        get_json(
            &instance.http,
            &instance.base,
            &instance.key,
            "/api/v1/interception"
        )
        .await,
        serde_json::json!({"clients": [], "exclude_domains": []}),
        "the empty document booted as the source of truth"
    );

    let spliced = tls_connect_from(
        None,
        https,
        PAGE_HOST,
        client_config_trusting(ca_der.clone()),
    )
    .await;
    assert!(
        spliced.is_err(),
        "an unlisted client is spliced, so a client trusting only our CA must reject the \
         origin's own certificate"
    );
    assert_eq!(
        instance.certificates().await["leaf_cache"]["minted_total"],
        0,
        "nothing is minted while the document lists no client"
    );

    let response = instance
        .http
        .put(format!("{}/api/v1/interception", instance.base))
        .bearer_auth(&instance.key)
        .json(&serde_json::json!({"clients": ["127.0.0.1"], "exclude_domains": []}))
        .send()
        .await
        .expect("PUT /api/v1/interception");
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json body");
    assert_eq!(
        body,
        serde_json::json!({"clients": ["127.0.0.1"], "exclude_domains": []})
    );
    assert!(
        body.get("restart_required").is_none(),
        "a document replacement never asks for a restart: {body}"
    );

    if cfg!(feature = "test-harness") {
        let mut inside = tls_connect_from(
            None,
            https,
            PAGE_HOST,
            client_config_trusting(ca_der.clone()),
        )
        .await
        .expect(
            "the next connection after the PUT is intercepted — no restart, no rebuild of \
             the machinery",
        );
        let (status, served) = fetch_over(&mut inside, PAGE_HOST, "/page").await;
        assert_eq!(status, 200);
        assert_eq!(served.as_bytes(), &page[..]);
        assert_eq!(
            instance.certificates().await["leaf_cache"]["minted_total"],
            1,
            "the newly listed client got a minted leaf on its next connection"
        );
    } else {
        assert!(
            skips_allowed(),
            "the binary was built without the `test-harness` feature, so no upstream root \
             could be injected — run `cargo test --all-features`, or set {ALLOW_SKIP_ENV}=1"
        );
    }

    let on_disk = std::fs::read_to_string(instance.config_dir.path().join("interception.json"))
        .expect("the document is persisted beside the TOML");
    assert_eq!(
        serde_json::from_str::<Value>(&on_disk).unwrap(),
        serde_json::json!({"clients": ["127.0.0.1"], "exclude_domains": []})
    );
    let toml = std::fs::read_to_string(instance.config_dir.path().join("fastadhunter.toml"))
        .expect("the TOML is readable");
    assert!(
        !toml.contains("interception"),
        "the TOML never gains the keys back: {toml}"
    );
}
