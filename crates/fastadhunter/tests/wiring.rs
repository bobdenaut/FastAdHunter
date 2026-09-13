use std::fs;
use std::path::{Path, PathBuf};

fn main_source() -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    match text.find("\n#[cfg(test)]\nmod tests {") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

fn body_of(source: &str, signature: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is still in main.rs"));
    let rest = &source[start + signature.len()..];
    let end = rest
        .find("\n    async fn ")
        .into_iter()
        .chain(rest.find("\n    fn "))
        .min()
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn the_run_loop_still_reaps_supervised_tasks() {
    let body = body_of(&main_source(), "async fn run(&mut self)");
    assert!(
        body.contains("self.reap_dead_tasks().await"),
        "the run loop dropped its supervision arm. Without it every supervised task dies \
         silently — the six supervisor.rs tests call reap() directly and stay green \
         regardless (p3-10 A4, F11):\n{body}"
    );
}

#[test]
fn reaping_covers_both_supervised_collections() {
    let body = body_of(&main_source(), "async fn reap_dead_tasks(&mut self)");
    for collection in ["self.tasks", "self.stats_schedulers"] {
        assert!(
            body.contains(&format!("reap(&mut {collection})")),
            "reap_dead_tasks no longer reaps `{collection}`. Keeping one collection and losing \
             the other leaves those tasks unsupervised with the whole suite green \
             (p3-10 A4, F11):\n{body}"
        );
    }
    assert!(
        body.contains("record_task_death"),
        "a reaped death is no longer counted, so it exists only in the log (p3-10 A4):\n{body}"
    );
}

#[test]
fn the_telemetry_poll_still_publishes_the_refusal_split() {
    let source = main_source();
    assert!(
        source.contains("metrics.set_requests_refused(refusals)"),
        "the telemetry poll stopped publishing refusals, so `engine.http.refused_*` and \
         `listeners.*.refused_*` would both freeze at zero. main.rs's own unit test calls \
         set_requests_refused directly and does not cover this call site (p3-10 A4, 28c751d)"
    );
    for listener in ["proxies.http.as_ref()", "proxies.https.as_ref()"] {
        assert!(
            source.contains(listener),
            "the refusal split stopped reading `{listener}`, so one listener's refusals vanish \
             from the sum (p3-10 A4, 28c751d)"
        );
    }
}
