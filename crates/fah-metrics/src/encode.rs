//! Prometheus text exposition format (0.0.4) encoder, hand-written rather
//! than pulling in the `prometheus` crate: this registry is a small, fixed
//! set of instruments known entirely at compile time, so a ~150-line encoder
//! covers it exactly, with no protobuf/quantile machinery the crate doesn't
//! use (PERFORMANCE.md: container image size budget; "every feature
//! justifies its runtime cost").

use std::fmt::Write as _;

use crate::histogram::{Histogram, BUCKETS_SECONDS};
use crate::process;
use crate::registry::Metrics;

/// Renders the full registry as Prometheus text exposition format — what
/// `fah-api`'s `GET /metrics` (p1-09) writes verbatim as the response body.
pub fn encode(metrics: &Metrics) -> String {
    let mut out = String::new();

    write_help_type(
        &mut out,
        "fastadhunter_queries_total",
        "counter",
        "Total DNS queries processed, by verdict.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_queries_total",
        &[("verdict", "pass")],
        metrics
            .queries_pass
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );
    writeln_metric(
        &mut out,
        "fastadhunter_queries_total",
        &[("verdict", "allow")],
        metrics
            .queries_allow
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );
    writeln_metric(
        &mut out,
        "fastadhunter_queries_total",
        &[("verdict", "block")],
        metrics
            .queries_block
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );

    write_help_type(
        &mut out,
        "fastadhunter_cache_hits_total",
        "counter",
        "Queries answered from the DNS cache.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_cache_hits_total",
        &[],
        metrics
            .cache_hits
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );

    write_help_type(
        &mut out,
        "fastadhunter_cache_misses_total",
        "counter",
        "Queries not found in the DNS cache.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_cache_misses_total",
        &[],
        metrics
            .cache_misses
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );

    write_help_type(
        &mut out,
        "fastadhunter_cache_stale_total",
        "counter",
        "Queries served from an expired cache entry (RFC 8767 serve-stale).",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_cache_stale_total",
        &[],
        metrics
            .cache_stale
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );

    write_help_type(
        &mut out,
        "fastadhunter_query_duration_seconds",
        "histogram",
        "Query latency by answering path: block/cache_hit are in-engine (PERFORMANCE.md <1ms p99 budgets); forward is end-to-end including the upstream round trip.",
    );
    write_histogram(
        &mut out,
        "fastadhunter_query_duration_seconds",
        "block",
        &metrics.duration_block,
    );
    write_histogram(
        &mut out,
        "fastadhunter_query_duration_seconds",
        "cache_hit",
        &metrics.duration_cache_hit,
    );
    write_histogram(
        &mut out,
        "fastadhunter_query_duration_seconds",
        "forward",
        &metrics.duration_forward,
    );

    write_help_type(
        &mut out,
        "fastadhunter_events_dropped_total",
        "counter",
        "QueryEvents dropped because the observers' event channel was full.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_events_dropped_total",
        &[],
        metrics
            .dropped_events
            .load(std::sync::atomic::Ordering::Relaxed) as f64,
    );

    let upstreams = metrics.upstreams.load();
    if !upstreams.is_empty() {
        write_help_type(
            &mut out,
            "fastadhunter_upstream_attempts_total",
            "counter",
            "Upstream resolution attempts, by server.",
        );
        for u in upstreams.iter() {
            writeln_metric(
                &mut out,
                "fastadhunter_upstream_attempts_total",
                &[("address", &u.address), ("protocol", u.protocol)],
                u.attempts as f64,
            );
        }
        write_help_type(
            &mut out,
            "fastadhunter_upstream_failures_total",
            "counter",
            "Upstream resolution failures, by server.",
        );
        for u in upstreams.iter() {
            writeln_metric(
                &mut out,
                "fastadhunter_upstream_failures_total",
                &[("address", &u.address), ("protocol", u.protocol)],
                u.failures as f64,
            );
        }
        write_help_type(
            &mut out,
            "fastadhunter_upstream_consecutive_failures",
            "gauge",
            "Failures since the last success, by server (0 = healthy).",
        );
        for u in upstreams.iter() {
            writeln_metric(
                &mut out,
                "fastadhunter_upstream_consecutive_failures",
                &[("address", &u.address), ("protocol", u.protocol)],
                u.consecutive_failures as f64,
            );
        }
        write_help_type(
            &mut out,
            "fastadhunter_upstream_tls_handshakes_total",
            "counter",
            "TLS handshakes performed, by server (flat under steady load = connection reuse).",
        );
        for u in upstreams.iter() {
            writeln_metric(
                &mut out,
                "fastadhunter_upstream_tls_handshakes_total",
                &[("address", &u.address), ("protocol", u.protocol)],
                u.tls_handshakes as f64,
            );
        }
    }

    let ruleset = metrics.ruleset.load();
    write_help_type(
        &mut out,
        "fastadhunter_ruleset_rules",
        "gauge",
        "Compiled rule count in the active ruleset.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_ruleset_rules",
        &[],
        ruleset.rules as f64,
    );
    write_help_type(
        &mut out,
        "fastadhunter_ruleset_heap_bytes",
        "gauge",
        "Heap memory held by the compiled ruleset.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_ruleset_heap_bytes",
        &[],
        ruleset.heap_bytes as f64,
    );
    write_help_type(
        &mut out,
        "fastadhunter_ruleset_duplicates_removed",
        "gauge",
        "Rules dropped by the last compile as duplicates of one already present.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_ruleset_duplicates_removed",
        &[],
        ruleset.duplicates_removed as f64,
    );
    write_help_type(
        &mut out,
        "fastadhunter_ruleset_compile_duration_seconds",
        "gauge",
        "Wall time of the most recent ruleset compile.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_ruleset_compile_duration_seconds",
        &[],
        ruleset.compile_duration.as_secs_f64(),
    );

    // Memory breakdown (p2-07). Sampled together by the binary, so
    // `component + residual` reconciles to the `rss` in the same snapshot —
    // which would not hold if this re-read RSS here, at a different instant.
    let memory = metrics.memory.load();
    write_help_type(
        &mut out,
        "fastadhunter_memory_component_bytes",
        "gauge",
        "Heap attributed to each bounded component, at the last telemetry poll.",
    );
    for (component, bytes) in [
        ("ruleset", memory.ruleset),
        ("cache", memory.cache),
        ("stats_aggregates", memory.stats.aggregates),
        ("stats_clients", memory.stats.clients),
        ("query_log_ring", memory.stats.ring),
        ("query_log_pending", memory.stats.pending_log),
    ] {
        writeln_metric(
            &mut out,
            "fastadhunter_memory_component_bytes",
            &[("component", component)],
            bytes as f64,
        );
    }
    write_help_type(
        &mut out,
        "fastadhunter_memory_residual_bytes",
        "gauge",
        "RSS minus every accounted component: binary pages, stacks, runtime and \
         allocator retention. Growth here while components stay flat is the leak signal.",
    );
    writeln_metric(
        &mut out,
        "fastadhunter_memory_residual_bytes",
        &[],
        memory.residual().unwrap_or(0) as f64,
    );

    write_help_type(
        &mut out,
        "process_resident_memory_bytes",
        "gauge",
        "Resident memory size in bytes.",
    );
    writeln_metric(
        &mut out,
        "process_resident_memory_bytes",
        &[],
        process::resident_memory_bytes() as f64,
    );

    out
}

fn write_help_type(out: &mut String, name: &str, kind: &str, help: &str) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    escape_help_into(out, help);
    out.push('\n');
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn write_histogram(out: &mut String, name: &str, stage: &str, hist: &Histogram) {
    let cumulative = hist.cumulative_counts();
    // One count read shared by `+Inf` and `_count` (the spec requires them
    // equal), clamped to the last finite bucket: the buckets and the count
    // are separate relaxed atomics, so a scrape racing `observe` could
    // otherwise print a `+Inf` below a finite bucket — invalid exposition.
    let count = hist
        .count()
        .max(cumulative.last().copied().unwrap_or_default());
    let bucket_name = format!("{name}_bucket");
    let mut bound_str = String::with_capacity(8);
    for (bound, cumulative_count) in BUCKETS_SECONDS.iter().zip(cumulative.iter()) {
        bound_str.clear();
        let _ = write!(bound_str, "{bound}");
        writeln_metric(
            out,
            &bucket_name,
            &[("stage", stage), ("le", &bound_str)],
            *cumulative_count as f64,
        );
    }
    writeln_metric(
        out,
        &bucket_name,
        &[("stage", stage), ("le", "+Inf")],
        count as f64,
    );
    writeln_metric(
        out,
        &format!("{name}_sum"),
        &[("stage", stage)],
        hist.sum_seconds(),
    );
    writeln_metric(
        out,
        &format!("{name}_count"),
        &[("stage", stage)],
        count as f64,
    );
}

/// Writes one sample line straight into `out` — no intermediate strings; the
/// only per-line work beyond the value formatting is pushing slices.
fn writeln_metric(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    out.push_str(name);
    if let Some(((first_key, first_value), rest)) = labels.split_first() {
        out.push('{');
        push_label(out, first_key, first_value);
        for (key, label_value) in rest {
            out.push(',');
            push_label(out, key, label_value);
        }
        out.push('}');
    }
    let _ = writeln!(out, " {value}");
}

fn push_label(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str("=\"");
    escape_label_into(out, value);
    out.push('"');
}

/// Prometheus text format label-value escaping: backslash, double-quote and
/// newline are the only characters that need it. Escape-free values (the
/// overwhelmingly common case) are appended as one slice.
fn escape_label_into(out: &mut String, value: &str) {
    if !value.contains(['\\', '"', '\n']) {
        out.push_str(value);
        return;
    }
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
}

/// HELP-line escaping per the exposition format: only backslash and newline
/// (double quotes are legal in HELP text). All current help strings are
/// escape-free literals; this keeps the next one honest.
fn escape_help_into(out: &mut String, help: &str) {
    if !help.contains(['\\', '\n']) {
        out.push_str(help);
        return;
    }
    for c in help.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fah_model::{DecisiveRule, Query, QueryEvent, QueryType, Verdict};
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::SystemTime;

    use crate::ruleset::RulesetSnapshot;
    use crate::upstream::UpstreamSnapshot;

    use super::*;

    fn client_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))
    }

    #[test]
    fn every_declared_family_has_a_matching_help_and_type_line() {
        let metrics = Metrics::new();
        let text = encode(&metrics);
        for name in [
            "fastadhunter_queries_total",
            "fastadhunter_cache_hits_total",
            "fastadhunter_cache_misses_total",
            "fastadhunter_cache_stale_total",
            "fastadhunter_query_duration_seconds",
            "fastadhunter_events_dropped_total",
            "fastadhunter_ruleset_rules",
            "fastadhunter_ruleset_heap_bytes",
            "fastadhunter_ruleset_duplicates_removed",
            "fastadhunter_ruleset_compile_duration_seconds",
            "process_resident_memory_bytes",
        ] {
            assert!(
                text.contains(&format!("# HELP {name} ")),
                "missing HELP line for {name}"
            );
            assert!(
                text.contains(&format!("# TYPE {name} ")),
                "missing TYPE line for {name}"
            );
        }
    }

    #[test]
    fn counters_reflect_recorded_events() {
        let metrics = Metrics::new();
        metrics.record(&QueryEvent::new(
            Query::new(
                "ads.example.com",
                QueryType::A,
                client_ip(),
                SystemTime::now(),
            ),
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            Duration::from_micros(50),
            false,
            false,
            false,
        ));

        let text = encode(&metrics);
        assert!(text.contains("fastadhunter_queries_total{verdict=\"block\"} 1"));
        assert!(text.contains("fastadhunter_queries_total{verdict=\"pass\"} 0"));
    }

    #[test]
    fn upstream_family_is_omitted_until_a_snapshot_is_set() {
        let metrics = Metrics::new();
        assert!(!encode(&metrics).contains("fastadhunter_upstream_attempts_total"));

        metrics.set_upstreams(vec![UpstreamSnapshot {
            address: "1.1.1.1".to_string(),
            protocol: "udp",
            attempts: 10,
            failures: 2,
            consecutive_failures: 0,
            tls_handshakes: 0,
        }]);
        let text = encode(&metrics);
        assert!(text.contains(
            "fastadhunter_upstream_attempts_total{address=\"1.1.1.1\",protocol=\"udp\"} 10"
        ));
    }

    #[test]
    fn ruleset_gauges_reflect_the_last_snapshot() {
        let metrics = Metrics::new();
        metrics.set_ruleset(RulesetSnapshot {
            rules: 1_000_000,
            heap_bytes: 40_000_000,
            compile_duration: Duration::from_millis(1500),
            duplicates_removed: 213_000,
        });
        let text = encode(&metrics);
        assert!(text.contains("fastadhunter_ruleset_rules 1000000"));
        assert!(text.contains("fastadhunter_ruleset_heap_bytes 40000000"));
        assert!(text.contains("fastadhunter_ruleset_duplicates_removed 213000"));
        assert!(text.contains("fastadhunter_ruleset_compile_duration_seconds 1.5"));
    }

    #[test]
    fn histogram_buckets_are_cumulative_and_end_in_inf() {
        let metrics = Metrics::new();
        metrics.record(&QueryEvent::new(
            Query::new("example.com", QueryType::A, client_ip(), SystemTime::now()),
            Verdict::Pass,
            Duration::from_micros(50),
            true,
            false,
            false,
        ));
        let text = encode(&metrics);
        assert!(text.contains(
            "fastadhunter_query_duration_seconds_bucket{stage=\"cache_hit\",le=\"+Inf\"} 1"
        ));
        assert!(text.contains("fastadhunter_query_duration_seconds_sum{stage=\"cache_hit\"}"));
        assert!(text.contains("fastadhunter_query_duration_seconds_count{stage=\"cache_hit\"} 1"));
    }

    #[test]
    fn label_values_are_escaped() {
        let mut out = String::new();
        escape_label_into(&mut out, "a\"b\\c\nd");
        assert_eq!(out, "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn help_text_is_escaped() {
        let mut out = String::new();
        write_help_type(&mut out, "m", "counter", "line\\one\nline two");
        assert!(out.starts_with("# HELP m line\\\\one\\nline two\n"));
    }
}
