//! [`Metrics`]: the one registry instance, held behind an `Arc` and shared
//! with every producer (ARCHITECTURE.md §Dependency Layering — siblings wire
//! through the binary, not through each other). Query counters and the
//! latency histograms update via [`Metrics::record`], called by the binary's
//! event fan-out task — the single consumer of the pipeline's `QueryEvent`
//! channel, which also feeds `fah-stats` and the WS hub; upstream/ruleset/
//! channel-drop numbers are polled snapshots the binary pushes in (those
//! crates' own counters already exist for exactly this —
//! `fah_dns::Pipeline::dropped_events`,
//! `fah_dns::upstream::UpstreamPool::status`, `fah_rules::Matcher::len`/
//! `heap_bytes` — this crate just never imports their types).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use fah_model::{QueryEvent, Verdict};

use crate::histogram::Histogram;
use crate::ruleset::RulesetSnapshot;
use crate::snapshot::{MetricsSnapshot, StageHistogram};
use crate::upstream::UpstreamSnapshot;

pub struct Metrics {
    pub(crate) queries_pass: AtomicU64,
    pub(crate) queries_allow: AtomicU64,
    pub(crate) queries_block: AtomicU64,
    pub(crate) cache_hits: AtomicU64,
    pub(crate) cache_misses: AtomicU64,
    pub(crate) cache_stale: AtomicU64,
    /// Total in-pipeline latency, bucketed by the path that answered the
    /// query. `block` and `cache_hit` compare directly against
    /// PERFORMANCE.md's <1 ms p99 rows; `forward` measures end-to-end
    /// including the upstream round trip (and, for serve-stale, the failed
    /// forward attempt that preceded it), so it is NOT comparable to the
    /// "overhead added by engine" budget row — that would need a
    /// pipeline-side timer around the upstream await.
    pub(crate) duration_block: Histogram,
    pub(crate) duration_cache_hit: Histogram,
    pub(crate) duration_forward: Histogram,
    pub(crate) dropped_events: AtomicU64,
    pub(crate) upstreams: ArcSwap<Vec<UpstreamSnapshot>>,
    pub(crate) ruleset: ArcSwap<RulesetSnapshot>,
    pub(crate) memory: ArcSwap<fah_model::MemoryBreakdown>,
    /// **Temporary** (post-p2-07): wall time the last memory-accounting pass
    /// took, in microseconds. Deliberately *not* part of
    /// [`fah_model::MemoryBreakdown`] — it is metadata about the measurement,
    /// not a memory figure, and that type is permanent while this is not.
    ///
    /// Exists because the accounting is paid every 10 s forever, partly under
    /// `fah-stats`'s mutexes, and the only figure so far is 43 µs on an x86 dev
    /// box — where the RSS read never even runs (no procfs), so the `/proc`
    /// open+parse that the RB5009 actually pays was never in it. Remove once a
    /// soak shows the on-device number is stable.
    pub(crate) memory_collection_micros: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            queries_pass: AtomicU64::new(0),
            queries_allow: AtomicU64::new(0),
            queries_block: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            cache_stale: AtomicU64::new(0),
            duration_block: Histogram::new(),
            duration_cache_hit: Histogram::new(),
            duration_forward: Histogram::new(),
            dropped_events: AtomicU64::new(0),
            upstreams: ArcSwap::new(Arc::new(Vec::new())),
            ruleset: ArcSwap::new(Arc::new(RulesetSnapshot::default())),
            memory: ArcSwap::new(Arc::new(fah_model::MemoryBreakdown::default())),
            memory_collection_micros: AtomicU64::new(0),
        }
    }

    /// Records one completed query. The hot-path entry point — atomic
    /// increments only, no lock, no allocation (PERFORMANCE.md). Blocked
    /// queries never reach the cache or an upstream (ADR-0001), so their
    /// `cache_hit`/`stale` are always false. A stale serve has
    /// `cache_hit == true` but only happens after a forward attempt failed
    /// (its duration includes that upstream timeout), so it belongs to the
    /// `forward` stage — routing it to `cache_hit` would blow that
    /// histogram's <1 ms budget signal during exactly the outages it should
    /// stay clean through.
    pub fn record(&self, event: &QueryEvent) {
        match event.verdict {
            Verdict::Pass => self.queries_pass.fetch_add(1, Ordering::Relaxed),
            Verdict::Allow(_) => self.queries_allow.fetch_add(1, Ordering::Relaxed),
            Verdict::Block(_) => self.queries_block.fetch_add(1, Ordering::Relaxed),
        };

        // Only resolved queries have a cache outcome to record. A blocked query
        // never reaches the cache (ADR-0001), so it carries `cache_hit == false`
        // by construction — counting that as a *miss* inflated
        // `cache_misses_total` by exactly the block count and understated the
        // hit rate by more the better the blocker worked. Mirrors what
        // `DnsCache::note_lookup` already does on the pipeline side, where the
        // call sits inside the non-blocked branch. The stage selection below has
        // always tested `Block` first; this branch was the one that did not.
        if !matches!(event.verdict, Verdict::Block(_)) {
            if event.cache_hit {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                if event.stale {
                    self.cache_stale.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                self.cache_misses.fetch_add(1, Ordering::Relaxed);
            }
        }

        let stage = if matches!(event.verdict, Verdict::Block(_)) {
            &self.duration_block
        } else if event.cache_hit && !event.stale {
            &self.duration_cache_hit
        } else {
            &self.duration_forward
        };
        stage.observe(event.duration);
    }

    /// Channel-drop count off `fah_dns::Pipeline::dropped_events()` — a
    /// monotonic counter read fresh on each poll (not incremented here), so
    /// this just replaces the last-known value rather than adding to it.
    pub fn set_dropped_events(&self, count: u64) {
        self.dropped_events.store(count, Ordering::Relaxed);
    }

    pub fn set_upstreams(&self, snapshot: Vec<UpstreamSnapshot>) {
        self.upstreams.store(Arc::new(snapshot));
    }

    pub fn set_ruleset(&self, snapshot: RulesetSnapshot) {
        self.ruleset.store(Arc::new(snapshot));
    }

    /// Where the process's memory is, as measured by the binary in one pass
    /// (p2-07). This crate never reads the structures itself — it is an L3
    /// sibling of the crates that own them.
    pub fn set_memory(&self, snapshot: fah_model::MemoryBreakdown) {
        self.memory.store(Arc::new(snapshot));
    }

    /// **Temporary** — cost of assembling the whole breakdown, not of any one
    /// component: the caller must time the entire pass it hands to
    /// [`Self::set_memory`], RSS read included, because that is what is
    /// actually paid every 10 s. See [`Self::memory_collection_micros`].
    pub fn set_memory_collection_micros(&self, micros: u64) {
        self.memory_collection_micros
            .store(micros, Ordering::Relaxed);
    }

    /// A point-in-time read of the whole registry for the perf sampler
    /// (p1.5-02) — off the hot path, on the sample cadence. Every field is a
    /// lifetime-cumulative counter; the sampler deltas consecutive snapshots
    /// for per-interval rates and percentiles (see [`MetricsSnapshot`]).
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            queries_pass: self.queries_pass.load(Ordering::Relaxed),
            queries_allow: self.queries_allow.load(Ordering::Relaxed),
            queries_block: self.queries_block.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            cache_stale: self.cache_stale.load(Ordering::Relaxed),
            dropped_events: self.dropped_events.load(Ordering::Relaxed),
            block: stage_histogram(&self.duration_block),
            cache_hit: stage_histogram(&self.duration_cache_hit),
            forward: stage_histogram(&self.duration_forward),
            upstreams: self.upstreams.load().as_ref().clone(),
        }
    }
}

/// Reads one latency [`Histogram`] into the sampler-facing [`StageHistogram`]
/// (cumulative bucket counts + total + sum).
fn stage_histogram(hist: &Histogram) -> StageHistogram {
    StageHistogram {
        cumulative: hist.cumulative_counts(),
        count: hist.count(),
        sum_seconds: hist.sum_seconds(),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, SystemTime};

    use fah_model::{DecisiveRule, Query, QueryType};

    use super::*;

    fn event(verdict: Verdict, cache_hit: bool, upstream_used: bool, stale: bool) -> QueryEvent {
        QueryEvent::new(
            Query::new(
                "example.com",
                QueryType::A,
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                SystemTime::now(),
            ),
            verdict,
            Duration::from_micros(100),
            cache_hit,
            upstream_used,
            stale,
        )
    }

    #[test]
    fn counts_queries_by_verdict() {
        let metrics = Metrics::new();
        metrics.record(&event(Verdict::Pass, false, true, false));
        metrics.record(&event(
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            false,
            false,
            false,
        ));
        metrics.record(&event(
            Verdict::Allow(DecisiveRule::new("allow", "@@||example.com^")),
            false,
            true,
            false,
        ));

        assert_eq!(metrics.queries_pass.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.queries_block.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.queries_allow.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cache_hit_miss_and_stale_are_counted_independently() {
        let metrics = Metrics::new();
        metrics.record(&event(Verdict::Pass, true, false, false));
        metrics.record(&event(Verdict::Pass, true, false, true));
        metrics.record(&event(Verdict::Pass, false, true, false));

        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 1);
    }

    /// A blocked query never reaches the cache (ADR-0001), so it must not land
    /// in either cache counter. Counting it as a miss inflated
    /// `cache_misses_total` by exactly the block count — measured on the RB5009
    /// as `hits + misses == pass + allow + block` when it should equal
    /// `pass + allow` — which understates the reported hit rate by more the more
    /// the blocker blocks (~23 points at a 30 % block rate).
    #[test]
    fn a_blocked_query_is_not_counted_as_a_cache_miss() {
        let metrics = Metrics::new();
        let blocked = || {
            event(
                Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
                false,
                false,
                false,
            )
        };
        for _ in 0..5 {
            metrics.record(&blocked());
        }
        metrics.record(&event(Verdict::Pass, true, false, false)); // resolved, hit
        metrics.record(&event(Verdict::Pass, false, true, false)); // resolved, miss

        assert_eq!(metrics.queries_block.load(Ordering::Relaxed), 5);
        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 1);
        assert_eq!(
            metrics.cache_misses.load(Ordering::Relaxed),
            1,
            "only the resolved miss counts; the 5 blocks never touched the cache"
        );

        // The invariant the RB5009 data violated: the two cache counters must
        // sum to the queries that actually reached the cache, not to every query.
        let snap = metrics.snapshot();
        assert_eq!(
            snap.cache_hits + snap.cache_misses,
            snap.queries_pass + snap.queries_allow,
            "cache outcomes must account for exactly the resolved queries"
        );
    }

    #[test]
    fn duration_lands_in_the_stage_matching_the_event() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            false,
            false,
            false,
        ));
        metrics.record(&event(Verdict::Pass, true, false, false));
        metrics.record(&event(Verdict::Pass, false, true, false));

        assert_eq!(metrics.duration_block.count(), 1);
        assert_eq!(metrics.duration_cache_hit.count(), 1);
        assert_eq!(metrics.duration_forward.count(), 1);
    }

    /// A stale serve is a cache hit whose duration includes the failed
    /// forward attempt — it must land in `forward`, or an upstream outage
    /// reads as a cache-latency regression.
    #[test]
    fn stale_serve_records_into_the_forward_stage() {
        let metrics = Metrics::new();
        metrics.record(&event(Verdict::Pass, true, false, true));

        assert_eq!(metrics.duration_cache_hit.count(), 0);
        assert_eq!(metrics.duration_forward.count(), 1);
        assert_eq!(
            metrics.cache_hits.load(Ordering::Relaxed),
            1,
            "hit/stale counters are unaffected by the stage routing"
        );
        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn snapshot_reflects_recorded_counters_and_histograms() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            false,
            false,
            false,
        ));
        metrics.record(&event(Verdict::Pass, true, false, false)); // cache hit
        metrics.set_dropped_events(4);
        metrics.set_upstreams(vec![UpstreamSnapshot {
            address: "1.1.1.1".to_string(),
            protocol: "udp",
            attempts: 10,
            failures: 1,
            consecutive_failures: 0,
            tls_handshakes: 0,
        }]);

        let snap = metrics.snapshot();
        assert_eq!(snap.queries_block, 1);
        assert_eq!(snap.queries_pass, 1);
        assert_eq!(snap.cache_hits, 1);
        assert_eq!(snap.dropped_events, 4);
        assert_eq!(snap.block.count, 1);
        assert_eq!(snap.cache_hit.count, 1);
        assert_eq!(snap.forward.count, 0);
        assert_eq!(
            snap.block.cumulative.len(),
            crate::histogram::BUCKETS_SECONDS.len()
        );
        assert_eq!(snap.upstreams.len(), 1);
        // The 100 µs observations land in the 0.0001 s (first) bucket → p99 there.
        assert_eq!(
            snap.block.quantile(0.99),
            crate::histogram::BUCKETS_SECONDS[0]
        );
    }

    #[test]
    fn snapshots_replace_rather_than_accumulate() {
        let metrics = Metrics::new();
        metrics.set_dropped_events(5);
        metrics.set_dropped_events(3);
        assert_eq!(metrics.dropped_events.load(Ordering::Relaxed), 3);

        metrics.set_ruleset(RulesetSnapshot {
            rules: 100,
            heap_bytes: 4096,
            compile_duration: Duration::from_millis(50),
            duplicates_removed: 7,
        });
        assert_eq!(metrics.ruleset.load().rules, 100);
    }
}
