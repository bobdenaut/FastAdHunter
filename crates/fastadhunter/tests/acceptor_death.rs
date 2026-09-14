mod common;

use std::ffi::OsStr;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::net::{TcpStream, UdpSocket};

use common::{
    boot_with, get_json, resolve, run_mock_upstream_answering, ApiScheme, Ports, UPSTREAM_IP,
};

const KILL_ACCEPTOR_ENV: &str = "FAH_TEST_KILL_ACCEPTOR";
const KILL_WHEN_ENV: &str = "FAH_TEST_KILL_ACCEPTOR_WHEN";
const RESOLVED_HOST: &str = "alive.example.com";
const OBSERVATION_BUDGET: Duration = Duration::from_secs(35);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const SUPERVISION_TICK: Duration = Duration::from_secs(10);

fn config(ports: &Ports, upstream: SocketAddr) -> String {
    let dns_port = ports.dns;
    let dot_port = ports.dot;
    let api_port = ports.api;
    let http_port = ports.http();
    let https_port = ports.https();
    format!(
        r#"
[engine]
mode = "dns+http+https"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}
dot_port = {dot_port}

[dns.upstreams]
strategy = "adaptive"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "{upstream}"
protocol = "udp"

[http.listen]
address = "127.0.0.1"
port = {http_port}

[https]
hello_timeout_ms = 5000
idle_timeout_ms = 10000

[https.listen]
address = "127.0.0.1"
port = {https_port}

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
level = "info"
format = "text"
"#
    )
}

async fn poke(port: u16) {
    let _ = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await;
}

async fn alive(dns_port: u16, when: &str) {
    let answer = resolve(dns_port, RESOLVED_HOST).await;
    assert_eq!(
        answer.a_records,
        vec![UPSTREAM_IP],
        "the resolver must answer {when}"
    );
}

fn engine_log(config_dir: &Path) -> String {
    std::fs::read_to_string(config_dir.join("engine.log")).unwrap_or_default()
}

fn names_a_death(log: &str, task: &str) -> bool {
    log.lines()
        .any(|line| line.contains(&format!("task=\"{task}\"")) && line.contains("cause=returned"))
}

async fn boot(
    config_dir: &tempfile::TempDir,
    data_dir: &tempfile::TempDir,
    acceptors: &str,
) -> (common::Guard, Ports, String, String, reqwest::Client) {
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream_answering(upstream, UPSTREAM_IP));

    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");
    let sentinel = config_dir.path().join("kill-acceptor");
    assert!(
        !sentinel.exists(),
        "the sentinel must not exist before the test trips it"
    );
    let envs: Vec<(&str, &OsStr)> = vec![
        (KILL_ACCEPTOR_ENV, OsStr::new(acceptors)),
        (KILL_WHEN_ENV, sentinel.as_os_str()),
    ];
    let (child, ports, base) = boot_with(
        config_dir.path(),
        data_dir.path(),
        &http,
        ApiScheme::Https,
        &envs,
        |ports| config(ports, upstream_addr),
    )
    .await;
    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();
    (child, ports, base, key, http)
}

fn trip(config_dir: &tempfile::TempDir) {
    std::fs::write(config_dir.path().join("kill-acceptor"), b"die\n").expect("trip the sentinel");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_http_or_https_acceptor_is_observed_and_dns_keeps_answering() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let (_child, ports, base, key, http) = boot(&config_dir, &data_dir, "http,https").await;
    let http_port = ports.http();
    let https_port = ports.https();

    alive(ports.dns, "before any acceptor dies").await;

    trip(&config_dir);

    let deadline = Instant::now() + OBSERVATION_BUDGET;
    let counted = loop {
        poke(http_port).await;
        poke(https_port).await;
        let telemetry = get_json(&http, &base, &key, "/api/v1/telemetry").await;
        let died = telemetry["counters"]["tasks_died"]
            .as_u64()
            .unwrap_or_default();
        if died >= 2 {
            break died;
        }
        assert!(
            Instant::now() < deadline,
            "both acceptors must be counted within {OBSERVATION_BUDGET:?}; tasks_died = {died}. \
             A build made without --all-features compiles the test-harness kill seam out, so \
             nothing ever asked an acceptor to die and this is what that looks like\
             \n--- engine log ---\n{}",
            engine_log(config_dir.path())
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    };
    assert_eq!(
        counted,
        2,
        "only the two acceptors may have died; a third death means something else broke\
         \n--- engine log ---\n{}",
        engine_log(config_dir.path())
    );

    tokio::time::sleep(SUPERVISION_TICK + POLL_INTERVAL).await;
    let settled = get_json(&http, &base, &key, "/api/v1/telemetry").await["counters"]["tasks_died"]
        .as_u64()
        .unwrap_or_default();
    assert_eq!(
        settled,
        counted,
        "a death is handed over once, not on every supervision tick. The poll above stops the \
         moment the figure reaches two, which cannot tell two acceptors counted once each \
         apart from one acceptor re-counted every tick; a full tick later the figure must not \
         have moved\n--- engine log ---\n{}",
        engine_log(config_dir.path())
    );

    let log = engine_log(config_dir.path());
    for task in ["HTTP acceptor", "HTTPS acceptor"] {
        assert!(
            names_a_death(&log, task),
            "the log must name {task} with cause=returned — a cancelled loop would be an \
             intentional stop, which is the opposite of the thing under test\
             \n--- engine log ---\n{log}"
        );
    }

    alive(
        ports.dns,
        "after both acceptors died. This is probably a cache hit: it proves the UDP listener, \
         the pipeline and the runtime are still alive, not that the upstream path was \
         re-exercised",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_api_acceptor_is_observed_and_dns_keeps_answering() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let (_child, ports, _base, _key, _http) = boot(&config_dir, &data_dir, "api").await;
    let api_port = ports.api;

    alive(ports.dns, "before the API acceptor dies").await;

    trip(&config_dir);

    let deadline = Instant::now() + OBSERVATION_BUDGET;
    loop {
        poke(api_port).await;
        let log = engine_log(config_dir.path());
        if names_a_death(&log, "API acceptor") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the API acceptor's death must be logged within {OBSERVATION_BUDGET:?}. A build \
             made without --all-features compiles the test-harness kill seam out, so nothing \
             ever asked an acceptor to die and this is what that looks like\
             \n--- engine log ---\n{log}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    let refused = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, api_port))).await;
    assert!(
        refused.is_err(),
        "the API port must stop accepting once its loop ended — a logged death with a live \
         socket would mean the wrong thing died"
    );

    alive(
        ports.dns,
        "after the API acceptor died, taking DoH with it. This is probably a cache hit: it \
         proves the UDP listener, the pipeline and the runtime are still alive, not that the \
         upstream path was re-exercised",
    )
    .await;
}
