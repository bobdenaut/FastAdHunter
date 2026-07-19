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
        "In-engine query latency, by the pipeline stage that answered (PERFORMANCE.md budgets).",
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
        "QueryEvents dropped because a consumer channel (stats/metrics) was full.",
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
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn write_histogram(out: &mut String, name: &str, stage: &str, hist: &Histogram) {
    let cumulative = hist.cumulative_counts();
    for (bound, count) in BUCKETS_SECONDS.iter().zip(cumulative.iter()) {
        writeln_metric(
            out,
            &format!("{name}_bucket"),
            &[("stage", stage), ("le", &format_bound(*bound))],
            *count as f64,
        );
    }
    writeln_metric(
        out,
        &format!("{name}_bucket"),
        &[("stage", stage), ("le", "+Inf")],
        hist.count() as f64,
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
        hist.count() as f64,
    );
}

fn format_bound(bound: f64) -> String {
    format!("{bound}")
}

fn writeln_metric(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    if labels.is_empty() {
        let _ = writeln!(out, "{name} {}", format_value(value));
        return;
    }
    let rendered: Vec<String> = labels
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", escape_label_value(v)))
        .collect();
    let _ = writeln!(
        out,
        "{name}{{{}}} {}",
        rendered.join(","),
        format_value(value)
    );
}

fn format_value(value: f64) -> String {
    format!("{value}")
}

/// Prometheus text format label-value escaping: backslash, double-quote and
/// newline are the only characters that need it.
fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
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
        });
        let text = encode(&metrics);
        assert!(text.contains("fastadhunter_ruleset_rules 1000000"));
        assert!(text.contains("fastadhunter_ruleset_heap_bytes 40000000"));
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
        assert_eq!(escape_label_value("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }
}
