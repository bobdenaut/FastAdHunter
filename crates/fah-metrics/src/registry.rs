//! [`Metrics`]: the one registry instance, held behind an `Arc` and shared
//! with every producer (ARCHITECTURE.md §Dependency Layering — siblings wire
//! through the binary, not through each other). Query counters and the
//! latency histograms update via [`Metrics::record`], called by the binary's
//! event fan-out task — the single consumer of the pipeline's `QueryEvent`
//! channel, which also feeds `fah-stats` and the WS hub; upstream/ruleset/
//! channel-drop numbers are polled snapshots the binary pushes in (those
//! crates' own counters already exist for exactly this —
//! `fah_dns::Pipeline::dropped_events`,
//! `fah_dns::upstream::UpstreamPool::status`, `fah_rules::Matcher::len` —
//! this crate just never imports their types).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use fah_model::{QueryEvent, RequestEvent, StaleServe, Verdict};

use crate::histogram::Histogram;
use crate::ruleset::RulesetSnapshot;
use crate::snapshot::{CleanupSnapshot, MetricsSnapshot, StageHistogram, SwrSnapshot};
use fah_model::UpstreamSample;

pub struct Metrics {
    pub(crate) queries_pass: AtomicU64,
    pub(crate) queries_allow: AtomicU64,
    pub(crate) queries_block: AtomicU64,
    pub(crate) cache_hits: AtomicU64,
    pub(crate) cache_misses: AtomicU64,
    pub(crate) cache_stale: AtomicU64,
    /// Total in-pipeline latency, bucketed by the path that answered the
    /// query. `block` and `cache_hit` compare directly against
    /// PERFORMANCE.md's <1 ms p99 rows — `cache_hit` covers every serve that
    /// came out of the cache without waiting on the network, SWR stale serves
    /// included. `forward` measures end-to-end including the upstream round
    /// trip (and, for the RFC 8767 fallback, the *failed* attempt that
    /// preceded it), so it is NOT comparable to the "overhead added by
    /// engine" budget row — that would need a pipeline-side timer around the
    /// upstream await.
    pub(crate) duration_block: Histogram,
    pub(crate) duration_cache_hit: Histogram,
    pub(crate) duration_forward: Histogram,
    /// HTTP request counters (p2-04). Separate from the DNS ones rather than
    /// shared: `fastadhunter_queries_total` has meant "DNS questions answered"
    /// since p1-08, and folding requests into it would silently redefine every
    /// existing dashboard and alert built on it.
    pub(crate) requests_pass: AtomicU64,
    pub(crate) requests_allow: AtomicU64,
    pub(crate) requests_block: AtomicU64,
    /// Response bytes relayed downstream — the figure that makes "a blocked
    /// request ships nothing" visible as a trend rather than as an assertion.
    pub(crate) response_bytes: AtomicU64,
    /// Request latency, bucketed the way the DNS one is: a block never touches
    /// the network, so mixing it with a forward would hide the very budget row
    /// (`< 1 ms` for a synthesized block) it exists to prove.
    pub(crate) request_duration_block: Histogram,
    pub(crate) request_duration_forward: Histogram,
    pub(crate) dropped_events: AtomicU64,
    /// Stale-while-refresh counters (ADR-0005). Stored as one value rather than
    /// five atomics because they are read together, replaced together off
    /// `fah_dns::Pipeline::swr_stats()`, and only ever compared with each other
    /// — five independently-updated atomics could be scraped mid-update and
    /// show `enqueued` behind `completed`.
    pub(crate) swr: ArcSwap<SwrSnapshot>,
    /// Scheduled cache-sweep counters, stored as one value for the same reason
    /// [`Metrics::swr`] is: they are read together, replaced together off
    /// `fah_dns::Pipeline::cache_cleanup_stats()`, and a scrape landing
    /// mid-update could otherwise show bytes freed by a run that has not been
    /// counted yet.
    pub(crate) cleanup: ArcSwap<CleanupSnapshot>,
    pub(crate) upstreams: ArcSwap<Vec<UpstreamSample>>,
    pub(crate) ruleset: ArcSwap<RulesetSnapshot>,
    pub(crate) memory: ArcSwap<fah_model::MemoryBreakdown>,
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
            requests_pass: AtomicU64::new(0),
            requests_allow: AtomicU64::new(0),
            requests_block: AtomicU64::new(0),
            response_bytes: AtomicU64::new(0),
            request_duration_block: Histogram::new(),
            request_duration_forward: Histogram::new(),
            dropped_events: AtomicU64::new(0),
            swr: ArcSwap::new(Arc::new(SwrSnapshot::default())),
            cleanup: ArcSwap::new(Arc::new(CleanupSnapshot::default())),
            upstreams: ArcSwap::new(Arc::new(Vec::new())),
            ruleset: ArcSwap::new(Arc::new(RulesetSnapshot::default())),
            memory: ArcSwap::new(Arc::new(fah_model::MemoryBreakdown::default())),
        }
    }

    /// Records one completed query. The hot-path entry point — atomic
    /// increments only, no lock, no allocation (PERFORMANCE.md). Blocked
    /// queries never reach the cache or an upstream (ADR-0001), so their
    /// `cache_hit`/`stale` are always unset. A stale serve has
    /// `cache_hit == true`, and which of the two stale paths produced it
    /// decides the stage: [`StaleServe::AfterForwardFailure`] carries an
    /// upstream timeout and belongs to `forward` — routing it to `cache_hit`
    /// would blow that histogram's <1 ms budget signal during exactly the
    /// outages it should stay clean through — while
    /// [`StaleServe::FromSwr`] never touched the network (ADR-0005) and is a
    /// cache read like any other. Pooling them under one bool put 84 % of the
    /// RB5009's `forward` samples in the wrong histogram and reported the
    /// forward mean as 2.40 ms when it was 15.06 ms.
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
                if event.is_stale() {
                    self.cache_stale.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                self.cache_misses.fetch_add(1, Ordering::Relaxed);
            }
        }

        let stage = if matches!(event.verdict, Verdict::Block(_)) {
            &self.duration_block
        } else if event.cache_hit && event.stale != Some(StaleServe::AfterForwardFailure) {
            &self.duration_cache_hit
        } else {
            &self.duration_forward
        };
        stage.observe(event.duration);
    }

    /// Records one completed HTTP request (p2-04). Same contract as
    /// [`Metrics::record`]: atomic increments only, no lock, no allocation.
    pub fn record_http(&self, event: &RequestEvent) {
        match event.verdict {
            Verdict::Pass => self.requests_pass.fetch_add(1, Ordering::Relaxed),
            Verdict::Allow(_) => self.requests_allow.fetch_add(1, Ordering::Relaxed),
            Verdict::Block(_) => self.requests_block.fetch_add(1, Ordering::Relaxed),
        };
        self.response_bytes
            .fetch_add(event.bytes, Ordering::Relaxed);
        let stage = if matches!(event.verdict, Verdict::Block(_)) {
            &self.request_duration_block
        } else {
            &self.request_duration_forward
        };
        stage.observe(event.duration);
    }

    /// Channel-drop count off `fah_dns::Pipeline::dropped_events()` — a
    /// monotonic counter read fresh on each poll (not incremented here), so
    /// this just replaces the last-known value rather than adding to it.
    pub fn set_dropped_events(&self, count: u64) {
        self.dropped_events.store(count, Ordering::Relaxed);
    }

    /// Stale-while-refresh counters off `fah_dns::Pipeline::swr_stats()`
    /// (ADR-0005) — like `set_dropped_events`, these are monotonic counters
    /// read fresh each poll, so this replaces the last-known value rather than
    /// adding to it.
    pub fn set_swr(&self, snapshot: SwrSnapshot) {
        self.swr.store(Arc::new(snapshot));
    }

    /// Cache-cleanup counters off `fah_dns::Pipeline::cache_cleanup_stats()`,
    /// on the same read-fresh-and-replace contract as [`Self::set_swr`].
    pub fn set_cleanup(&self, snapshot: CleanupSnapshot) {
        self.cleanup.store(Arc::new(snapshot));
    }

    pub fn set_upstreams(&self, snapshot: Vec<UpstreamSample>) {
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

    /// The last published breakdown, for the perf sampler to persist (p2-07).
    /// Reading it here rather than re-collecting is what keeps a row's
    /// components and its RSS on one instant — and off a second pass costing
    /// up to 4.9 ms on-device.
    pub fn memory(&self) -> fah_model::MemoryBreakdown {
        **self.memory.load()
    }

    /// The engine's operational state for `GET /api/v1/telemetry`, as the L1
    /// value three crates share (`fah_model::EngineTelemetry`).
    ///
    /// Deliberately **not** built on [`Self::snapshot`]: that one allocates a
    /// `Vec<u64>` of cumulative bucket counts per stage — five heap
    /// allocations — and the telemetry surface reads none of them, only each
    /// histogram's count and sum. The one allocation left is the upstream
    /// vector, which is real data rather than a discarded intermediate.
    pub fn engine_telemetry(&self) -> fah_model::EngineTelemetry {
        let ruleset = self.ruleset.load();
        let swr = **self.swr.load();
        let cleanup = **self.cleanup.load();
        fah_model::EngineTelemetry {
            ruleset: fah_model::RulesetInfo {
                rules: ruleset.rules as u64,
                duplicates_removed: ruleset.duplicates_removed as u64,
                compile_duration: ruleset.compile_duration,
            },
            counters: fah_model::EngineCounters {
                dns: fah_model::DnsCounters {
                    pass: self.queries_pass.load(Ordering::Relaxed),
                    allow: self.queries_allow.load(Ordering::Relaxed),
                    block: self.queries_block.load(Ordering::Relaxed),
                    cache_hits: self.cache_hits.load(Ordering::Relaxed),
                    cache_misses: self.cache_misses.load(Ordering::Relaxed),
                    cache_stale: self.cache_stale.load(Ordering::Relaxed),
                },
                http: fah_model::HttpCounters {
                    pass: self.requests_pass.load(Ordering::Relaxed),
                    allow: self.requests_allow.load(Ordering::Relaxed),
                    block: self.requests_block.load(Ordering::Relaxed),
                    response_bytes: self.response_bytes.load(Ordering::Relaxed),
                },
                events_dropped: self.dropped_events.load(Ordering::Relaxed),
                swr: fah_model::SwrCounters {
                    enqueued: swr.enqueued,
                    deduplicated: swr.deduplicated,
                    dropped: swr.dropped,
                    completed: swr.completed,
                    failed: swr.failed,
                },
                cache_cleanup: fah_model::CacheCleanupCounters {
                    runs: cleanup.runs,
                    entries_removed: cleanup.entries_removed,
                    bytes_freed: cleanup.bytes_freed,
                    last_duration: std::time::Duration::from_micros(cleanup.last_duration_micros),
                },
            },
            latency: fah_model::LatencyTotals {
                dns: fah_model::DnsLatency {
                    block: stage_totals(&self.duration_block),
                    cache_hit: stage_totals(&self.duration_cache_hit),
                    forward: stage_totals(&self.duration_forward),
                },
                http: fah_model::HttpLatency {
                    block: stage_totals(&self.request_duration_block),
                    forward: stage_totals(&self.request_duration_forward),
                },
            },
            upstreams: self.upstreams.load().as_ref().clone(),
        }
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
            requests_pass: self.requests_pass.load(Ordering::Relaxed),
            requests_allow: self.requests_allow.load(Ordering::Relaxed),
            requests_block: self.requests_block.load(Ordering::Relaxed),
            response_bytes: self.response_bytes.load(Ordering::Relaxed),
            swr: **self.swr.load(),
            cleanup: **self.cleanup.load(),
            block: stage_histogram(&self.duration_block),
            cache_hit: stage_histogram(&self.duration_cache_hit),
            forward: stage_histogram(&self.duration_forward),
            request_block: stage_histogram(&self.request_duration_block),
            request_forward: stage_histogram(&self.request_duration_forward),
            upstreams: self.upstreams.load().as_ref().clone(),
        }
    }
}

/// Reads one latency [`Histogram`] into the two numbers an average is made of,
/// skipping the bucket vector [`stage_histogram`] builds.
fn stage_totals(hist: &Histogram) -> fah_model::StageTotals {
    fah_model::StageTotals {
        count: hist.count(),
        sum_seconds: hist.sum_seconds(),
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

    fn event(
        verdict: Verdict,
        cache_hit: bool,
        upstream_used: bool,
        stale: Option<StaleServe>,
    ) -> QueryEvent {
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
        metrics.record(&event(Verdict::Pass, false, true, None));
        metrics.record(&event(
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            false,
            false,
            None,
        ));
        metrics.record(&event(
            Verdict::Allow(DecisiveRule::new("allow", "@@||example.com^")),
            false,
            true,
            None,
        ));

        assert_eq!(metrics.queries_pass.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.queries_block.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.queries_allow.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cache_hit_miss_and_stale_are_counted_independently() {
        let metrics = Metrics::new();
        metrics.record(&event(Verdict::Pass, true, false, None));
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::AfterForwardFailure),
        ));
        metrics.record(&event(Verdict::Pass, false, true, None));

        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 1);
    }

    /// `cache_stale` counts stale serves, not stale *fallbacks* — both paths
    /// answered from an expired entry, and an operator watching the counter
    /// is asking how often that happened at all.
    #[test]
    fn both_stale_paths_count_as_stale_serves() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::FromSwr),
        ));
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::AfterForwardFailure),
        ));

        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.cache_hits.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.cache_misses.load(Ordering::Relaxed), 0);
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
                None,
            )
        };
        for _ in 0..5 {
            metrics.record(&blocked());
        }
        metrics.record(&event(Verdict::Pass, true, false, None)); // resolved, hit
        metrics.record(&event(Verdict::Pass, false, true, None)); // resolved, miss

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
            None,
        ));
        metrics.record(&event(Verdict::Pass, true, false, None));
        metrics.record(&event(Verdict::Pass, false, true, None));

        assert_eq!(metrics.duration_block.count(), 1);
        assert_eq!(metrics.duration_cache_hit.count(), 1);
        assert_eq!(metrics.duration_forward.count(), 1);
    }

    /// The RFC 8767 fallback is a cache hit whose duration includes the failed
    /// forward attempt — it must land in `forward`, or an upstream outage
    /// reads as a cache-latency regression.
    #[test]
    fn stale_after_forward_failure_records_into_the_forward_stage() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::AfterForwardFailure),
        ));

        assert_eq!(metrics.duration_cache_hit.count(), 0);
        assert_eq!(metrics.duration_forward.count(), 1);
        assert_eq!(
            metrics.cache_hits.load(Ordering::Relaxed),
            1,
            "hit/stale counters are unaffected by the stage routing"
        );
        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 1);
    }

    /// An SWR stale serve never reached the forwarder (ADR-0005), so timing it
    /// as a forward is what made the RB5009 report a 2.40 ms forward mean when
    /// the 1 061 real forwards averaged 15.06 ms — 84 % of that histogram's
    /// samples were sub-100 µs cache reads.
    #[test]
    fn stale_from_swr_records_into_the_cache_hit_stage() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::FromSwr),
        ));

        assert_eq!(metrics.duration_cache_hit.count(), 1);
        assert_eq!(
            metrics.duration_forward.count(),
            0,
            "a serve that never touched the network is not a forward"
        );
        assert_eq!(metrics.cache_stale.load(Ordering::Relaxed), 1);
    }

    /// The identity a snapshot reader relies on: every resolved query lands in
    /// exactly one stage, and `forward` holds the misses plus the outage
    /// fallbacks — nothing else.
    #[test]
    fn stages_partition_resolved_queries() {
        let metrics = Metrics::new();
        metrics.record(&event(Verdict::Pass, true, false, None)); // fresh hit
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::FromSwr),
        ));
        metrics.record(&event(Verdict::Pass, false, true, None)); // miss
        metrics.record(&event(
            Verdict::Pass,
            true,
            false,
            Some(StaleServe::AfterForwardFailure),
        ));

        let snap = metrics.snapshot();
        assert_eq!(snap.cache_hit.count, 2, "fresh hit + SWR stale serve");
        assert_eq!(snap.forward.count, 2, "miss + RFC 8767 fallback");
        assert_eq!(
            snap.cache_hit.count + snap.forward.count,
            snap.queries_pass + snap.queries_allow,
            "the two resolved stages must account for every resolved query"
        );
    }

    #[test]
    fn snapshot_reflects_recorded_counters_and_histograms() {
        let metrics = Metrics::new();
        metrics.record(&event(
            Verdict::Block(DecisiveRule::new("oisd", "||ads.example.com^")),
            false,
            false,
            None,
        ));
        metrics.record(&event(Verdict::Pass, true, false, None)); // cache hit
        metrics.set_dropped_events(4);
        metrics.set_upstreams(vec![UpstreamSample {
            address: "1.1.1.1".to_string(),
            protocol: fah_model::Protocol::Udp,
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
            compile_duration: Duration::from_millis(50),
            duplicates_removed: 7,
        });
        assert_eq!(metrics.ruleset.load().rules, 100);
    }
}
