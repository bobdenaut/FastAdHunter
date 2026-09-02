mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hickory_proto::op::ResponseCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use common::{
    await_event, bind_origin, boot_full, client_config_trusting, client_hello_without_sni,
    connect_events, get_json, insecure_client_config, pseudo_random_payload, put_user_rules,
    resolve, resolve_doh_post, resolve_dot, run_tls_origin, self_signed_origin,
    skip_origin_message, tls_connect_from, FullMode, AD_HOST, DOT_HOSTNAME, PAGE_HOST,
};

const ORIGIN_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 30);
const TEST_BUDGET: Duration = Duration::from_secs(90);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_mode_blocks_at_every_layer() {
    let started = Instant::now();
    let origin = match bind_origin(ORIGIN_IP).await {
        Some(listener) => {
            let (cert, key) = self_signed_origin(&[PAGE_HOST, AD_HOST]);
            Some(run_tls_origin(
                listener,
                cert,
                key,
                Arc::new(pseudo_random_payload(1024, 30)),
            ))
        }
        None => {
            eprintln!(
                "{} The intercepted leg runs without an origin.",
                skip_origin_message(ORIGIN_IP)
            );
            None
        }
    };

    let instance = boot_full(FullMode {
        origin_ip: ORIGIN_IP,
        clients: vec!["127.0.0.1".to_string()],
        api_tls: true,
    })
    .await;
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
    assert!(
        intercepted.is_err(),
        "5/7 intercepted: the listed client enters the terminate leg, and an origin the binary \
         cannot verify is closed unanswered"
    );
    let event = await_event(&mut events, "https event for the listed client", |data| {
        data["kind"] == "https" && data["domain"] == PAGE_HOST
    })
    .await;
    match &origin {
        Some(origin) => {
            assert_eq!(
                event["status"], 526,
                "5/7 intercepted: upstream certificate failure is reported as 526: {event}"
            );
            assert!(origin.accepts.load(Ordering::Relaxed) >= 1);
        }
        None => assert_eq!(
            event["status"], 0,
            "5/7 intercepted: with no origin the upstream connect fails: {event}"
        ),
    }
    let certificates = instance.certificates().await;
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 0,
        "5/7 intercepted: verify-before-mint — no leaf for an unverifiable origin"
    );

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
    assert_eq!(
        certificates["leaf_cache"]["minted_total"], 1,
        "one leaf: the DoT hostname; the intercepted leg minted none"
    );
    assert_eq!(certificates["leaf_cache"]["unwarmed_misses"], 0);

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

    assert!(
        started.elapsed() < TEST_BUDGET,
        "the scenario took {:?}, over the {TEST_BUDGET:?} budget",
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
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("send");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .expect("the proxy closes within 10 s")
        .expect("read");
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("an HTTP head");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .expect("a status code");
    (status, body.to_string())
}

async fn raw_probe(port: u16, bytes: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .expect("connect to the HTTPS listener");
    stream.write_all(bytes).await.expect("send the hello");
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut received))
        .await
        .expect("the listener closes a no-SNI hello within 10 s")
        .ok();
    received
}
