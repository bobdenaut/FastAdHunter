//! [`Metrics`]: the one registry instance, held behind an `Arc` and shared
//! with every producer (ARCHITECTURE.md §Dependency Layering — siblings wire
//! through the binary, not through each other). Query counters and the
//! latency histograms update off the same `QueryEvent` channel `fah-stats`
//! consumes ([`Metrics::spawn_collector`]); upstream/ruleset/channel-drop
//! numbers are polled snapshots the binary pushes in (those crates' own
//! counters already exist for exactly this — `fah_dns::Pipeline::dropped_events`,
//! `fah_dns::upstream::UpstreamPool::status`, `fah_rules::Matcher::len`/
//! `heap_bytes` — this crate just never imports their types).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use fah_model::{QueryEvent, Verdict};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::histogram::Histogram;
use crate::ruleset::RulesetSnapshot;
use crate::upstream::UpstreamSnapshot;

pub struct Metrics {
    pub(crate) queries_pass: AtomicU64,
    pub(crate) queries_allow: AtomicU64,
    pub(crate) queries_block: AtomicU64,
    pub(crate) cache_hits: AtomicU64,
    pub(crate) cache_misses: AtomicU64,
    pub(crate) cache_stale: AtomicU64,
    /// In-engine latency, bucketed by the pipeline stage that answered the
    /// query — matches PERFORMANCE.md's three latency budget rows exactly
    /// (verdict+cache hit, blocked query, forwarded-query overhead).
    pub(crate) duration_block: Histogram,
    pub(crate) duration_cache_hit: Histogram,
    pub(crate) duration_forward: Histogram,
    pub(crate) dropped_events: AtomicU64,
    pub(crate) upstreams: ArcSwap<Vec<UpstreamSnapshot>>,
    pub(crate) ruleset: ArcSwap<RulesetSnapshot>,
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
        }
    }

    /// Records one completed query. The hot-path entry point — atomic
    /// increments only, no lock, no allocation (PERFORMANCE.md). Blocked
    /// queries never reach the cache or an upstream (ADR-0001), so their
    /// `cache_hit`/`stale` are always false; the stage split below relies on
    /// that to stay mutually exclusive with the cache/forward paths.
    pub fn record(&self, event: &QueryEvent) {
        match event.verdict {
            Verdict::Pass => self.queries_pass.fetch_add(1, Ordering::Relaxed),
            Verdict::Allow(_) => self.queries_allow.fetch_add(1, Ordering::Relaxed),
            Verdict::Block(_) => self.queries_block.fetch_add(1, Ordering::Relaxed),
        };

        if event.cache_hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            if event.stale {
                self.cache_stale.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        let stage = if matches!(event.verdict, Verdict::Block(_)) {
            &self.duration_block
        } else if event.cache_hit {
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

    /// Consumes `QueryEvent`s until the sender side closes — mirrors
    /// `fah_stats::Stats::spawn_collector`; the same channel feeds both, one
    /// receiver each (`fastadhunter` creates one bounded channel per
    /// consumer, ARCHITECTURE.md §Dependency Layering).
    pub fn spawn_collector(
        self: &Arc<Self>,
        mut events: mpsc::Receiver<QueryEvent>,
    ) -> JoinHandle<()> {
        let metrics = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                metrics.record(&event);
            }
        })
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
        });
        assert_eq!(metrics.ruleset.load().rules, 100);
    }

    #[tokio::test]
    async fn collector_consumes_events_from_the_channel() {
        let metrics = Arc::new(Metrics::new());
        let (tx, rx) = mpsc::channel(8);
        let handle = metrics.spawn_collector(rx);

        tx.send(event(Verdict::Pass, false, true, false))
            .await
            .unwrap();
        drop(tx);
        handle.await.unwrap();

        assert_eq!(metrics.queries_pass.load(Ordering::Relaxed), 1);
    }
}
