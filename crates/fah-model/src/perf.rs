//! Perf / system / cache sample series (ARCHITECTURE.md L1 — pure data + serde).
//! One [`PerfSample`] per sampling interval, written by `fah-stats` to
//! `/data/history/perf/perf-YYYY-MM-DD.jsonl` (ADR-0002: flat JSONL, no
//! embedded DB) — the router self-hosting the RSS/QPS/latency/cache/upstream
//! series the ~91h soak captured externally with a curl loop. Plain records:
//! no logic, no I/O (root CLAUDE.md hard rule 2).

use crate::memory::MemoryComponents;
use serde::{Deserialize, Serialize};

/// One sampling interval's live figures — everything a dashboard graphs that
/// isn't a per-query aggregate. `qps` and the `*_delta` counters are computed
/// by the binary sampler from the difference between consecutive `Metrics`
/// reads: the underlying counters are process-lifetime cumulative, but a chart
/// wants rates, and per-interval deltas keep every row self-contained (no
/// cross-row subtraction needed to read it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfSample {
    /// Seconds since the Unix epoch at capture (wall clock).
    pub ts: u64,
    /// Process resident set size in bytes at capture.
    pub rss_bytes: u64,
    /// High-water RSS from `getrusage`'s `ru_maxrss` — **monotone within one
    /// container lifetime**, so a drop is a restart, never a reclaim. A
    /// minutes-apart sampler catches the seconds-long compile only through it.
    #[serde(default)]
    pub peak_rss: u64,
    /// Queries per second over the interval since the previous sample
    /// (`queries_delta / interval_seconds`); `0.0` for the first sample after
    /// boot (no prior reading to delta against).
    pub qps: f64,
    /// New queries since the previous sample. `pass` is derivable as
    /// `queries_delta - blocked_delta - allowed_delta`.
    pub queries_delta: u64,
    pub blocked_delta: u64,
    pub allowed_delta: u64,
    pub cache: CacheStatsSample,
    pub latency: LatencySummary,
    pub upstreams: Vec<UpstreamSample>,
    /// Named components at capture (p2-07). No `rss` (that is `rss_bytes`) and
    /// no residual — residual is derived on read via
    /// [`crate::MemoryBreakdown::residual`], so it cannot disagree with its own
    /// inputs. `default`: rows predating p2-07, and the first row after boot if
    /// the telemetry poll has not published yet, read back as all-zero.
    #[serde(default)]
    pub memory: MemoryComponents,
    /// Minor page faults since start, 0 where unavailable. Persisted because
    /// its *derivative* is the purge-thrash signal and a rate needs consecutive
    /// rows; the other allocator figures are monotone ramps and are not.
    /// Outside `memory` so `accounted()` cannot pick up a kernel counter.
    #[serde(default)]
    pub minor_page_faults: u64,
}

/// A range of [`PerfSample`]s plus the decimation applied to fit the caller's
/// point budget. `stride` is `1` for an undecimated range and `n` when only
/// every `n`-th sample was kept — the rows themselves are always verbatim
/// readings, never averages, so a spike that survives decimation is real and
/// one that is dropped is dropped whole rather than smoothed away.
#[derive(Debug, Clone, PartialEq)]
pub struct PerfSeries {
    pub samples: Vec<PerfSample>,
    pub stride: u64,
}

/// Cache usage at sample time — the figures behind `GET /api/v1/cache`, minus
/// the derived percentages (a dashboard divides `entries / capacity` and
/// `bytes / max_bytes` itself) and the slab-inclusive estimate (API.md keeps
/// that to `/debug/memory`). `hits`/`misses`/`evictions` are process-lifetime
/// totals; the entry-state fields (`entries`/`fresh`/`stale`/`expired`) and
/// `bytes` are point-in-time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheStatsSample {
    pub entries: u64,
    pub capacity: u64,
    pub fresh: u64,
    pub stale: u64,
    pub expired: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// The cache's second bound (p1.5-05): what the resident entries hold,
    /// against the ceiling they evict at. `default` because rows written
    /// before the byte cap existed are still valid history and must keep
    /// deserializing — they read back as `0`, which charts as "not recorded".
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub max_bytes: u64,
}

/// Per-stage latency percentiles in **seconds**, estimated over the interval
/// since the previous sample from `fah-metrics`' fixed histogram buckets.
/// `pXX` is the smallest bucket upper bound whose cumulative count reaches the
/// quantile — coarse by construction (bucket granularity), saturating at the
/// top finite bucket when the quantile lands beyond it. A stage with no
/// queries in the interval reports `0.0`. Not exact percentiles: a
/// resolution-bounded estimate for charting (the phase documents the
/// resolution rather than implying precision).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatencySummary {
    pub block_p50: f64,
    pub block_p99: f64,
    pub cache_hit_p50: f64,
    pub cache_hit_p99: f64,
    pub forward_p50: f64,
    pub forward_p99: f64,
}

/// One upstream server's counters at sample time — the persisted mirror of
/// `fah-metrics`' `UpstreamSnapshot`, and the same rows `GET /api/v1/telemetry`
/// publishes live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamSample {
    pub address: String,
    pub protocol: crate::Protocol,
    pub attempts: u64,
    pub failures: u64,
    /// Failures since the last success. **Not a liveness signal**: under
    /// `fallback` a secondary is attempted only when the primary fails, so a
    /// non-zero streak can be hours old. Read it beside `attempts`.
    pub consecutive_failures: u64,
    pub tls_handshakes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_sample_serde_roundtrip() {
        let sample = PerfSample {
            ts: 1_695_600_000,
            rss_bytes: 55_000_000,
            peak_rss: 123_539_456,
            qps: 12.5,
            queries_delta: 750,
            blocked_delta: 210,
            allowed_delta: 5,
            cache: CacheStatsSample {
                entries: 10_000,
                capacity: 16_384,
                fresh: 9_000,
                stale: 800,
                expired: 200,
                hits: 500_000,
                misses: 120_000,
                evictions: 3_400,
                bytes: 21_000_000,
                max_bytes: 67_108_864,
            },
            latency: LatencySummary {
                block_p50: 0.0001,
                block_p99: 0.0005,
                cache_hit_p50: 0.0001,
                cache_hit_p99: 0.00025,
                forward_p50: 0.005,
                forward_p99: 0.05,
            },
            memory: MemoryComponents {
                ruleset: 23_440_198,
                cache: 1_445_728,
                stats: crate::StatsHeap {
                    aggregates: 271_090,
                    clients: 132_352,
                },
            },
            minor_page_faults: 4_211_337,
            upstreams: vec![UpstreamSample {
                address: "1.1.1.1".to_string(),
                protocol: crate::Protocol::Dot,
                attempts: 12_000,
                failures: 3,
                consecutive_failures: 0,
                tls_handshakes: 4,
            }],
        };
        let json = serde_json::to_string(&sample).unwrap();
        let back: PerfSample = serde_json::from_str(&json).unwrap();
        assert_eq!(sample, back);
    }

    #[test]
    fn a_row_written_before_the_byte_cap_still_deserializes() {
        // History on disk outlives the schema: rows persisted by p1.5-02
        // carry no `bytes`/`max_bytes`, and dropping them on the floor would
        // silently truncate the retained series.
        let legacy = r#"{"ts":1,"rss_bytes":2,"qps":0.0,"queries_delta":0,
            "blocked_delta":0,"allowed_delta":0,
            "cache":{"entries":1,"capacity":2,"fresh":1,"stale":0,"expired":0,
                     "hits":0,"misses":0,"evictions":0},
            "latency":{"block_p50":0.0,"block_p99":0.0,"cache_hit_p50":0.0,
                       "cache_hit_p99":0.0,"forward_p50":0.0,"forward_p99":0.0},
            "upstreams":[]}"#;
        let sample: PerfSample = serde_json::from_str(legacy).unwrap();
        assert_eq!(sample.cache.entries, 1);
        assert_eq!(sample.cache.bytes, 0);
        assert_eq!(sample.cache.max_bytes, 0);
    }

    #[test]
    fn a_row_written_before_the_memory_breakdown_still_deserializes() {
        // 30 days of retained rows carry no `memory` and no
        // `minor_page_faults`; rejecting them would orphan the whole series.
        let legacy = r#"{"ts":1,"rss_bytes":40000000,"qps":0.0,"queries_delta":0,
            "blocked_delta":0,"allowed_delta":0,
            "cache":{"entries":1,"capacity":2,"fresh":1,"stale":0,"expired":0,
                     "hits":0,"misses":0,"evictions":0,"bytes":9,"max_bytes":99},
            "latency":{"block_p50":0.0,"block_p99":0.0,"cache_hit_p50":0.0,
                       "cache_hit_p99":0.0,"forward_p50":0.0,"forward_p99":0.0},
            "upstreams":[]}"#;
        let sample: PerfSample = serde_json::from_str(legacy).unwrap();
        assert_eq!(sample.rss_bytes, 40_000_000);
        assert_eq!(sample.memory, MemoryComponents::default());
        assert_eq!(sample.memory.accounted(), 0);
        assert_eq!(sample.minor_page_faults, 0);
    }

    /// The shape a 0.2.13 sampler writes: every p2-07 field present, no
    /// `peak_rss`. Those rows are the retained series on the router right now,
    /// so reading them back has to leave the rest of the row untouched — a 0
    /// here means "not recorded", never "the peak was zero".
    #[test]
    fn a_row_written_before_the_peak_still_deserializes() {
        let legacy = r#"{"ts":1,"rss_bytes":49942528,"qps":1.5,"queries_delta":9,
            "blocked_delta":2,"allowed_delta":0,
            "cache":{"entries":57,"capacity":16384,"fresh":57,"stale":0,"expired":0,
                     "hits":3,"misses":54,"evictions":0,"bytes":76976,"max_bytes":67108864},
            "latency":{"block_p50":0.0,"block_p99":0.0,"cache_hit_p50":0.0,
                       "cache_hit_p99":0.0,"forward_p50":0.0,"forward_p99":0.0},
            "memory":{"ruleset":27064396,"cache":76976,
                      "stats":{"aggregates":494055,"clients":264704}},
            "minor_page_faults":37188,
            "upstreams":[]}"#;
        let sample: PerfSample = serde_json::from_str(legacy).unwrap();
        assert_eq!(sample.peak_rss, 0);
        assert_eq!(sample.rss_bytes, 49_942_528);
        assert_eq!(sample.memory.ruleset, 27_064_396);
        assert_eq!(sample.minor_page_faults, 37_188);
    }
}
