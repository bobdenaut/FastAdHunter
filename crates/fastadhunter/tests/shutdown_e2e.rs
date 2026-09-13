#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

mod common;

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use hickory_proto::op::ResponseCode;
use serde_json::Value;
use tokio::net::UdpSocket;

use common::{await_stats, boot, resolve, run_mock_upstream, Guard, Ports, UPSTREAM_IP};

const EXIT_DEADLINE: Duration = Duration::from_secs(15);

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sigterm_persists_the_statistics_recorded_since_boot() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");

    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream(upstream));

    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");

    let (child, ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports, upstream_addr)
    })
    .await;
    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();

    let answer = resolve(ports.dns, "allowed.example.com").await;
    assert_eq!(answer.rcode, ResponseCode::NoError);
    assert_eq!(answer.a_records, vec![UPSTREAM_IP]);
    await_stats(&http, &base, &key, |stats| {
        stats["queries_total"].as_u64().unwrap_or(0) >= 1
    })
    .await;

    terminate(&child);
    let status = wait_for_exit(child, config_dir.path()).await;
    assert!(
        status.success(),
        "SIGTERM must end in a clean exit, got {status}"
    );

    let snapshot = read_snapshot(&data_dir.path().join("stats").join("snapshot.json"));
    assert!(
        queries_in(&snapshot["aggregates"]) >= 1,
        "the shutdown flush must persist the query made after boot: {snapshot}"
    );
    assert!(
        snapshot["clients"]["clients"]["127.0.0.1"].is_object(),
        "the shutdown flush must persist the client registry: {snapshot}"
    );
}

#[cfg(unix)]
fn terminate(child: &Guard) {
    let pid = i32::try_from(child.0.id()).expect("pid fits in i32");
    // SAFETY: `kill` only delivers a signal to a pid this harness spawned and still owns; no memory is involved.
    let sent = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(sent, 0, "SIGTERM must be delivered");
}

async fn wait_for_exit(mut child: Guard, config_dir: &Path) -> ExitStatus {
    let deadline = Instant::now() + EXIT_DEADLINE;
    loop {
        if let Some(status) = child.0.try_wait().expect("poll the child") {
            return status;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(config_dir.join("engine.log")).unwrap_or_default();
            panic!(
                "fastadhunter did not exit within {EXIT_DEADLINE:?} of SIGTERM\
                 \n--- engine log ---\n{log}"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn read_snapshot(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn queries_in(value: &Value) -> u64 {
    match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(name, field)| match field {
                Value::Number(count) if name == "queries" => count.as_u64().unwrap_or(0),
                nested => queries_in(nested),
            })
            .sum(),
        Value::Array(items) => items.iter().map(queries_in).sum(),
        _ => 0,
    }
}

fn config_toml(ports: &Ports, upstream: SocketAddr) -> String {
    let dns_port = ports.dns;
    let api_port = ports.api;
    format!(
        r#"
[engine]
mode = "dns"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}

[dns.blocking]
mode = "null_ip"
ttl_seconds = 10

[dns.upstreams]
strategy = "adaptive"
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
