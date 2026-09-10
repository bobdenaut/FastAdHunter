//! The engine's operational state at one instant (ARCHITECTURE.md L1 — pure
//! data, no logic, no I/O).
//!
//! CONTEXT.md §Metrics calls this content "operational telemetry … for
//! operators", distinct from Statistics (for users, `fah-stats`). This is that
//! telemetry as a *value*: what the engine is doing right now.
//!
//! Lives here for the reason [`crate::MemoryBreakdown`] does — three crates
//! need the same shape and none of them may import another. `fah-metrics`
//! produces it off its registry, `fah-api` serves it on
//! `GET /api/v1/telemetry`, and the binary is the only crate that sees both
//! (ARCHITECTURE.md §Dependency Layering — L3 siblings never import each
//! other, so a shared shape belongs at L1).
//!
//! ## These types are the wire shape
//!
//! They carry `Serialize` and are published as-is, rather than being restated
//! as a parallel set of response structs in `fah-api`. A DTO layer earns its
//! place when it *transforms* — [`crate::MemoryComponents`] has one because the
//! response renames every field, flattens a nested struct and adds two derived
//! totals. Here the mapping would have been the identity function across eight
//! structs, which is duplication with a rationalisation attached: two field
//! lists to keep in step, and a second full copy built and thrown away on every
//! request. The two genuine conversions (a `Duration` rendered as seconds, and
//! as microseconds) are `serialize_with` helpers below.
//!
//! The consequence is that **renaming a field here changes the public API**,
//! which the compatibility contract on `/api/v1/telemetry` already forbids
//! doing casually.
//!
//! ## Every counter is cumulative
//!
//! Process-lifetime, so a consumer charting rates deltas consecutive reads —
//! and must watch process uptime, because a restart returns all of them to zero
//! and a delta across that boundary is meaningless rather than merely small.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::perf::UpstreamSample;

/// Everything `GET /api/v1/telemetry` reports about the engine itself.
///
/// Process identity (version, uptime) is deliberately absent: it describes the
/// process rather than the engine, and the crate serving this already holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EngineTelemetry {
    pub ruleset: RulesetInfo,
    pub counters: EngineCounters,
    pub latency: LatencyTotals,
    pub upstreams: Vec<UpstreamSample>,
}

/// The compiled ruleset as the matcher last reported it.
///
/// **No `heap_bytes`.** It is the same `Matcher::heap_bytes()` that
/// [`crate::MemoryComponents::ruleset`] already carries, and memory sizes
/// belong with the other memory figures — two homes for one number is how the
/// two drift.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesetInfo {
    /// Distinct compiled rules, after identical ones were collapsed.
    pub rules: u64,
    /// Rules the last compile dropped as exact duplicates of one already
    /// present. A large number means two lists carry the same corpus.
    pub duplicates_removed: u64,
    #[serde(
        rename = "compile_duration_seconds",
        serialize_with = "as_seconds",
        deserialize_with = "from_seconds"
    )]
    pub compile_duration: Duration,
}

/// Lifetime-cumulative counters, grouped by what produces them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCounters {
    pub dns: DnsCounters,
    pub http: HttpCounters,
    /// Events the fan-out channel shed under backpressure — non-zero means
    /// Statistics and Metrics have both under-counted by this much.
    pub events_dropped: u64,
    pub swr: SwrCounters,
    pub cache_cleanup: CacheCleanupCounters,
    #[serde(default)]
    pub lists: ListFetchCounters,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListFetchCounters {
    pub bodies: u64,
    pub not_modified: u64,
    pub bytes_fetched: u64,
}

/// DNS questions answered, by outcome.
///
/// `cache_hits + cache_misses == pass + allow`, never `+ block`: a blocked
/// query never reaches the cache (ADR-0001). Divide a hit ratio by the resolved
/// queries, not by every query, or the ratio falls as the blocker improves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsCounters {
    pub pass: u64,
    pub allow: u64,
    pub block: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    /// Hits served past their TTL because the upstream was unreachable
    /// (RFC 8767 serve-stale). A subset of `cache_hits`.
    pub cache_stale: u64,
    #[serde(default)]
    pub answers: AnswerCounters,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerCounters {
    pub servfail_synthesized: u64,
    pub servfail_relayed: u64,
    pub refused_relayed: u64,
}

/// HTTP requests handled, by outcome (p2-04).
///
/// Kept apart from [`DnsCounters`] rather than folded in: `queries` has meant
/// "DNS questions" since p1-08, and merging would silently redefine every
/// figure built on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpCounters {
    pub pass: u64,
    pub allow: u64,
    pub block: u64,
    /// Response bytes relayed downstream — what makes "a blocked request ships
    /// nothing" visible as a trend rather than as an assertion.
    pub response_bytes: u64,
    #[serde(default)]
    pub refused: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerCounters {
    pub connections: u64,
    pub requests: u64,
    pub blocked: u64,
    pub refused_claim: u64,
    pub refused_destination: u64,
    pub resolve_failures: u64,
    pub upstream_failures: u64,
    pub upstream_cert_failures: u64,
    #[serde(default)]
    pub client_cert_rejections: u64,
    pub non_http: u64,
    pub non_tls: u64,
    pub hello_timeouts: u64,
    pub dropped_events: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerTelemetry {
    pub http: Option<ListenerCounters>,
    pub https: Option<ListenerCounters>,
}

/// Stale-while-refresh queue counters (ADR-0005).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwrCounters {
    pub enqueued: u64,
    pub deduplicated: u64,
    pub dropped: u64,
    pub completed: u64,
    pub failed: u64,
}

/// Scheduled cache-sweep counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheCleanupCounters {
    pub runs: u64,
    pub entries_removed: u64,
    pub bytes_freed: u64,
    /// **A last-value gauge, not a total** — the most recent sweep only.
    /// Deltaing it against a previous read produces nonsense; the three fields
    /// above are cumulative and delta correctly. JSON carries no equivalent of
    /// Prometheus's `# TYPE`, so this is the only warning a client gets.
    #[serde(
        rename = "last_duration_micros",
        serialize_with = "as_micros",
        deserialize_with = "from_micros"
    )]
    pub last_duration: Duration,
}

/// Per-stage latency, as the two numbers an average is made of.
///
/// Stages match the histograms `fah-metrics` records into: a blocked query
/// contacts no upstream, so it is not comparable with a forward, and a stale
/// serve counts as `forward` because its duration includes the failed forward
/// attempt that preceded it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyTotals {
    pub dns: DnsLatency,
    pub http: HttpLatency,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DnsLatency {
    /// A blocked query contacts no upstream, so this is engine time only.
    pub block: StageTotals,
    pub cache_hit: StageTotals,
    /// End-to-end, including the upstream round trip — and, for a stale serve,
    /// the failed forward attempt that preceded it.
    pub forward: StageTotals,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpLatency {
    pub block: StageTotals,
    pub forward: StageTotals,
}

/// One stage's observation count and total time.
///
/// **Deliberately not an average.** Both figures are lifetime-cumulative, so
/// `sum_seconds / count` is the mean since boot — which flattens within hours
/// of uptime and stops responding to anything. Deltaing two reads and dividing
/// gives the mean over that interval, which is what a chart wants. Percentiles
/// need buckets and are served, windowed, by `GET /api/v1/history/perf`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StageTotals {
    pub count: u64,
    pub sum_seconds: f64,
}

impl StageTotals {
    /// Mean seconds per observation since process start, or `None` when the
    /// stage has recorded nothing. Read the caveat on the type first: this is a
    /// lifetime figure, useful for a one-shot look and misleading as a trend.
    pub fn mean_seconds(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum_seconds / self.count as f64)
    }
}

fn as_seconds<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(duration.as_secs_f64())
}

/// `Duration::from_secs_f64` panics on a negative or non-finite input, and this
/// parses whatever a peer sent — so an out-of-range value is a deserialization
/// error, not a crash.
fn from_seconds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let seconds = f64::deserialize(deserializer)?;
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| serde::de::Error::custom(format!("not a duration in seconds: {seconds}")))
}

fn as_micros<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(duration.as_micros() as u64)
}

fn from_micros<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    Ok(Duration::from_micros(u64::deserialize(deserializer)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_counters_written_before_the_refused_field_still_deserialize() {
        let json = r#"{"pass":4412,"allow":0,"block":918,"response_bytes":148223904}"#;
        let counters: HttpCounters = serde_json::from_str(json).unwrap();
        assert_eq!(counters.refused, 0);
        assert_eq!(counters.block, 918);
    }

    #[test]
    fn an_empty_stage_has_no_mean_rather_than_a_zero_one() {
        assert_eq!(StageTotals::default().mean_seconds(), None);
    }

    #[test]
    fn the_mean_is_the_sum_over_the_count() {
        let stage = StageTotals {
            count: 4,
            sum_seconds: 0.002,
        };
        assert_eq!(stage.mean_seconds(), Some(0.0005));
    }

    /// The shape a consumer sees after a restart: everything back to zero. It
    /// must be representable and must not read as "no data" — which is why the
    /// endpoint pairs it with process uptime.
    #[test]
    fn a_freshly_started_engine_is_all_zero() {
        let fresh = EngineTelemetry::default();
        assert_eq!(fresh.counters.dns.pass, 0);
        assert_eq!(fresh.latency.dns.forward.count, 0);
        assert_eq!(fresh.ruleset.rules, 0);
        assert!(fresh.upstreams.is_empty());
    }

    /// The durations are the only fields whose Rust type and wire type differ,
    /// so they are the only place the serialization can silently drift.
    #[test]
    fn durations_serialize_in_the_unit_their_field_name_promises() {
        let telemetry = EngineTelemetry {
            ruleset: RulesetInfo {
                rules: 3,
                duplicates_removed: 1,
                compile_duration: Duration::from_millis(1500),
            },
            counters: EngineCounters {
                cache_cleanup: CacheCleanupCounters {
                    last_duration: Duration::from_micros(1842),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_value(&telemetry).unwrap();

        assert_eq!(json["ruleset"]["compile_duration_seconds"], 1.5);
        assert_eq!(
            json["counters"]["cache_cleanup"]["last_duration_micros"],
            1842
        );
        assert!(
            json["ruleset"].get("compile_duration").is_none(),
            "the Duration field name must not reach the wire beside its rendered form"
        );
    }

    /// Round-tripping is what lets a consumer — a test, the TUI, a scraper —
    /// hold this as a typed value instead of indexing a `serde_json::Value` by
    /// string, where a renamed field degrades to a silent `None` rather than a
    /// compile error. It also pins the two `serialize_with`/`deserialize_with`
    /// pairs to the same unit.
    #[test]
    fn the_published_shape_round_trips_back_into_the_same_value() {
        let telemetry = EngineTelemetry {
            ruleset: RulesetInfo {
                rules: 1_043_886,
                duplicates_removed: 41_207,
                compile_duration: Duration::from_millis(7_412),
            },
            counters: EngineCounters {
                dns: DnsCounters {
                    pass: 812_044,
                    block: 96_318,
                    ..Default::default()
                },
                cache_cleanup: CacheCleanupCounters {
                    last_duration: Duration::from_micros(1_842),
                    ..Default::default()
                },
                ..Default::default()
            },
            latency: LatencyTotals {
                dns: DnsLatency {
                    forward: StageTotals {
                        count: 269_446,
                        sum_seconds: 6_021.338,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            upstreams: vec![UpstreamSample {
                address: "1.1.1.1:853".to_string(),
                protocol: crate::Protocol::Dot,
                attempts: 201_883,
                failures: 12,
                consecutive_failures: 0,
                tls_handshakes: 41,
                failure_runs: [5, 2, 0, 1],
                state: crate::UpstreamState::Healthy,
                penalty_round: 0,
                penalties: 0,
                penalized_seconds_total: 0,
                probes: 0,
                probe_successes: 0,
                family: Some(crate::AddressFamily::V4),
                rtt: crate::UpstreamRtt::default(),
            }],
        };

        let json = serde_json::to_string(&telemetry).unwrap();
        assert_eq!(
            serde_json::from_str::<EngineTelemetry>(&json).unwrap(),
            telemetry
        );
    }

    /// The durations arrive from a peer, so a value no `Duration` can hold must
    /// be an error rather than the panic `Duration::from_secs_f64` would raise.
    #[test]
    fn an_out_of_range_duration_is_rejected_not_panicked_on() {
        let json = r#"{"rules":0,"duplicates_removed":0,"compile_duration_seconds":-1.0}"#;
        assert!(serde_json::from_str::<RulesetInfo>(json).is_err());
    }

    /// The nesting is the published shape, not an internal arrangement — a
    /// client indexes `latency.dns.forward`, so flattening these would be a
    /// breaking change rather than a refactor.
    #[test]
    fn latency_is_published_nested_by_protocol_then_stage() {
        let json = serde_json::to_value(EngineTelemetry::default()).unwrap();
        for stage in ["block", "cache_hit", "forward"] {
            assert_eq!(json["latency"]["dns"][stage]["count"], 0);
        }
        for stage in ["block", "forward"] {
            assert_eq!(json["latency"]["http"][stage]["sum_seconds"], 0.0);
        }
    }
}
