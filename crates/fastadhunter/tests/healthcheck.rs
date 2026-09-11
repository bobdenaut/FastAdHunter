mod common;

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use common::{boot, free_udp_port, Ports};

const PROBE_BUDGET: Duration = Duration::from_secs(3);

fn healthcheck(config_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastadhunter"))
        .args(["--config", config_path.to_str().unwrap(), "--healthcheck"])
        .output()
        .unwrap()
}

fn timed_healthcheck(config_path: &Path) -> (Output, Duration) {
    let started = Instant::now();
    let output = healthcheck(config_path);
    (output, started.elapsed())
}

fn config_toml(ports: &Ports) -> String {
    let dns_port = ports.dns;
    let api_port = ports.api;
    format!(
        r#"
[engine]
mode = "dns"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}

[dns.upstreams]
strategy = "adaptive"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "127.0.0.1:1"
protocol = "udp"

[rules]
refresh_hours_default = 24
lists = []

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthcheck_passes_while_the_listener_answers() {
    let config_dir = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let (_child, _ports, _base) =
        boot(config_dir.path(), data_dir.path(), &http, config_toml).await;

    let output = healthcheck(&config_dir.path().join("fastadhunter.toml"));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("healthcheck ok"));
}

#[test]
fn healthcheck_fails_when_nothing_answers_on_the_configured_port() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("fastadhunter.toml");
    let dead_port = free_udp_port();
    std::fs::write(
        &config_path,
        format!("[dns.listen]\naddress = \"127.0.0.1\"\nport = {dead_port}\n"),
    )
    .unwrap();

    let no_probe_path = dir.path().join("broken.toml");
    std::fs::write(&no_probe_path, "[dns.cache]\nmax_entrees = 500\n").unwrap();
    let (_, spawn_only) = timed_healthcheck(&no_probe_path);

    let (output, with_probe) = timed_healthcheck(&config_path);

    assert!(!output.status.success(), "a dead listener must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("dns-probe"), "stderr: {stderr}");
    let probe = with_probe.saturating_sub(spawn_only);
    assert!(
        probe < PROBE_BUDGET,
        "the probe took {probe:?} ({with_probe:?} minus {spawn_only:?} of process start), \
         past its {PROBE_BUDGET:?} budget"
    );
}

#[test]
fn healthcheck_names_the_config_check_and_the_offending_key_on_broken_toml() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("fastadhunter.toml");
    std::fs::write(&config_path, "[dns.cache]\nmax_entrees = 500\n").unwrap();

    let output = healthcheck(&config_path);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config:"), "stderr: {stderr}");
    assert!(stderr.contains("max_entrees"), "stderr: {stderr}");
}

#[test]
fn healthcheck_never_creates_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("fastadhunter.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_fastadhunter"))
        .args(["--config", config_path.to_str().unwrap(), "--healthcheck"])
        .env("FAH__DNS__LISTEN__PORT", free_udp_port().to_string())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "no config plus a dead port must fail, not pass"
    );
    assert!(
        !config_path.exists(),
        "healthcheck wrote the config file; it should be read-only"
    );
}
