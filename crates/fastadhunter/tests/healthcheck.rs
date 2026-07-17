use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fastadhunter"))
}

#[test]
fn healthcheck_exits_zero_without_writing_config() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("fastadhunter.toml");

    let output = bin()
        .args(["--config", config_path.to_str().unwrap(), "--healthcheck"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A healthcheck must observe, not mutate: it must not create the config file.
    assert!(
        !config_path.exists(),
        "healthcheck wrote the config file; it should be read-only"
    );
}

#[test]
fn healthcheck_exits_nonzero_and_names_offending_key_on_broken_toml() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("fastadhunter.toml");
    std::fs::write(&config_path, "[dns.cache]\nmax_entrees = 500\n").unwrap();

    let output = bin()
        .args(["--config", config_path.to_str().unwrap(), "--healthcheck"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("max_entrees"), "stderr: {stderr}");
}
