mod common;

use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{binary_under_test, boot, free_tcp_port, free_udp_port, get_json, run_mock_upstream};
use serde_json::{json, Value};
use tokio::net::UdpSocket;

const DOCUMENT_FILE: &str = "interception.json";
const CONFIG_FILE: &str = "fastadhunter.toml";
const MIGRATED_LINE: &str = "migrated [https.interception] into interception.json";
const IGNORED_LINE: &str = "[https.interception] ignored; interception.json is the source of truth";
const REFUSAL_DEADLINE: Duration = Duration::from_secs(30);

const LEGACY_CLIENTS: &str = r#"["10.0.0.5", "192.168.88.0/24"]"#;
const LEGACY_EXCLUDE: &str = r#"["bank.example"]"#;

fn config_toml(
    dns_port: u16,
    dot_port: u16,
    api_port: u16,
    upstream: SocketAddr,
    interception: &str,
) -> String {
    format!(
        r#"
[engine]
mode = "dns"

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

[rules]
refresh_hours_default = 24
lists = []

[stats]
snapshot_interval_seconds = 300

[api]
address = "127.0.0.1"
port = {api_port}
tls = true

{interception}
"#
    )
}

fn legacy_block(clients: &str, exclude_domains: &str) -> String {
    format!("[https.interception]\nclients = {clients}\nexclude_domains = {exclude_domains}\n")
}

async fn mock_upstream() -> SocketAddr {
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream(upstream));
    addr
}

fn insecure_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn api_key(config_dir: &Path) -> String {
    read(&config_dir.join("apikey")).trim().to_string()
}

fn expected_document() -> Value {
    json!({
        "clients": ["10.0.0.5", "192.168.88.0/24"],
        "exclude_domains": ["bank.example"],
    })
}

async fn assert_document_authoritative(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    config_dir: &Path,
) {
    let on_disk: Value = serde_json::from_str(&read(&config_dir.join(DOCUMENT_FILE)))
        .expect("the document on disk is JSON");
    assert_eq!(on_disk, expected_document(), "the document on disk");
    assert_eq!(
        get_json(client, base, key, "/api/v1/interception").await,
        expected_document(),
        "GET /api/v1/interception serves the migrated lists"
    );
    assert!(
        get_json(client, base, key, "/api/v1/config").await["https"]["interception"].is_null(),
        "GET /api/v1/config omits https.interception"
    );
    let toml = read(&config_dir.join(CONFIG_FILE));
    assert!(
        !toml.contains("interception"),
        "the TOML is re-saved without the legacy keys:\n{toml}"
    );
}

#[tokio::test]
async fn a_legacy_toml_block_is_migrated_once_and_ignored_after() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = mock_upstream().await;
    let http = insecure_client();
    let legacy = legacy_block(LEGACY_CLIENTS, LEGACY_EXCLUDE);
    let log_path = config_dir.path().join("engine.log");

    assert!(
        !config_dir.path().join(DOCUMENT_FILE).exists(),
        "the first boot starts without a document"
    );
    let (child, _ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports.dns, ports.dot, ports.api, upstream, &legacy)
    })
    .await;
    let key = api_key(config_dir.path());
    assert_document_authoritative(&http, &base, &key, config_dir.path()).await;
    let first_log = read(&log_path);
    assert!(
        first_log.contains(MIGRATED_LINE),
        "the first boot logs the migration:\n{first_log}"
    );
    assert!(
        !first_log.contains(IGNORED_LINE),
        "the first boot has nothing to ignore:\n{first_log}"
    );
    drop(child);

    let (_child, _ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports.dns, ports.dot, ports.api, upstream, &legacy)
    })
    .await;
    assert_document_authoritative(&http, &base, &key, config_dir.path()).await;
    let second_log = read(&log_path);
    assert!(
        second_log.contains(IGNORED_LINE),
        "a legacy block beside an existing document is ignored with one warning:\n{second_log}"
    );
    assert!(
        !second_log.contains(MIGRATED_LINE),
        "the second boot migrates nothing:\n{second_log}"
    );
}

#[tokio::test]
async fn a_legacy_block_beside_an_existing_document_never_overwrites_it() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = mock_upstream().await;
    let http = insecure_client();
    let stored = json!({"clients": ["10.9.9.9"], "exclude_domains": []});
    let document_path = config_dir.path().join(DOCUMENT_FILE);
    let seeded_bytes = format!("{}\n  \n", serde_json::to_string_pretty(&stored).unwrap());
    std::fs::write(&document_path, &seeded_bytes).expect("seed the document");
    let legacy = legacy_block(r#"["10.0.0.300"]"#, LEGACY_EXCLUDE);

    let (_child, _ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports.dns, ports.dot, ports.api, upstream, &legacy)
    })
    .await;
    let key = api_key(config_dir.path());
    assert_eq!(
        get_json(&http, &base, &key, "/api/v1/interception").await,
        stored,
        "the existing document wins; the legacy entries are not even validated"
    );
    assert_eq!(
        read(&document_path),
        seeded_bytes,
        "the existing document is never rewritten — not even its whitespace"
    );
    let toml = read(&config_dir.path().join(CONFIG_FILE));
    assert!(
        !toml.contains("interception"),
        "the legacy keys are stripped from the TOML again:\n{toml}"
    );
    let log = read(&config_dir.path().join("engine.log"));
    assert!(log.contains(IGNORED_LINE), "{log}");
}

fn refused_boot(config_dir: &Path, data_dir: &Path, config: String) -> String {
    let config_path = config_dir.join(CONFIG_FILE);
    std::fs::write(&config_path, config).expect("write config");
    let log_path = config_dir.join("engine.log");
    let log = std::fs::File::create(&log_path).expect("engine log");
    let mut child = Command::new(binary_under_test())
        .arg("--config")
        .arg(&config_path)
        .arg("--data")
        .arg(data_dir)
        .stdout(Stdio::from(log.try_clone().expect("clone log handle")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("spawn fastadhunter");
    let deadline = Instant::now() + REFUSAL_DEADLINE;
    loop {
        match child.try_wait().expect("poll the child") {
            Some(status) => {
                assert!(
                    !status.success(),
                    "the boot must be refused, but the binary exited cleanly"
                );
                return read(&log_path);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "the binary did not refuse the boot within {REFUSAL_DEADLINE:?}:\n{}",
                    read(&log_path)
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn refusal_config(upstream: SocketAddr, interception: &str) -> String {
    config_toml(
        free_udp_port(),
        free_tcp_port(),
        free_tcp_port(),
        upstream,
        interception,
    )
}

#[tokio::test]
async fn an_invalid_legacy_entry_refuses_the_first_boot_naming_list_index_and_entry() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = mock_upstream().await;
    let legacy = legacy_block(r#"["10.0.0.5", "10.0.0.300"]"#, LEGACY_EXCLUDE);
    let config = refusal_config(upstream, &legacy);

    let log = refused_boot(config_dir.path(), data_dir.path(), config);
    assert!(
        log.contains(r#"clients[1]: "10.0.0.300" is not an IP address or CIDR block"#),
        "the refusal names list, index and entry:\n{log}"
    );
    assert!(
        !config_dir.path().join(DOCUMENT_FILE).exists(),
        "no document is written on a refused migration"
    );
    let toml = read(&config_dir.path().join(CONFIG_FILE));
    assert!(
        toml.contains("[https.interception]"),
        "the TOML is untouched on a refused migration:\n{toml}"
    );
}

#[tokio::test]
async fn an_over_cap_legacy_list_refuses_the_first_boot_naming_list_len_and_cap() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = mock_upstream().await;
    let clients: Vec<String> = (0..257)
        .map(|i| format!("\"10.{}.{}.1\"", i / 256, i % 256))
        .collect();
    let legacy = legacy_block(&format!("[{}]", clients.join(", ")), "[]");
    let config = refusal_config(upstream, &legacy);

    let log = refused_boot(config_dir.path(), data_dir.path(), config);
    assert!(
        log.contains("clients: 257 entries exceed the cap of 256 by 1"),
        "the refusal names list, len and cap:\n{log}"
    );
    assert!(!config_dir.path().join(DOCUMENT_FILE).exists());
}

#[tokio::test]
async fn an_unreadable_document_refuses_the_boot_and_is_never_overwritten() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = mock_upstream().await;
    let document_path = config_dir.path().join(DOCUMENT_FILE);
    std::fs::write(&document_path, "{ not json\n").expect("seed a malformed document");
    let config = refusal_config(upstream, "");

    let log = refused_boot(config_dir.path(), data_dir.path(), config);
    assert!(
        log.contains(DOCUMENT_FILE),
        "the refusal names the document file:\n{log}"
    );
    assert_eq!(
        read(&document_path),
        "{ not json\n",
        "the malformed document is left exactly as found"
    );
}
