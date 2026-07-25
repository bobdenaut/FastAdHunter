//! Aggregate counters (CONTEXT.md: Statistics — "totals, blocked percentage,
//! top domains, top clients, rolling time buckets"). Everything is windowed
//! to the rolling 24h that API.md's `GET /api/v1/stats` payload declares
//! (`window: "24h"`), and everything is fixed-memory: totals derive from the
//! 24-slot bucket ring ([`HourlyBuckets`]), top-N domains from per-hour
//! capped counters ([`HourlyTopN`]).

use std::sync::Arc;
use std::time::SystemTime;

use fah_model::{QueryType, Verdict};
use serde::{Deserialize, Serialize};

use crate::bucket::{qtype_index, BucketView, HourlyBuckets, HourlyTypeCounts};
use crate::top_n::HourlyTopN;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Aggregates {
    top_blocked: HourlyTopN,
    top_queried: HourlyTopN,
    buckets: HourlyBuckets,
    /// Per-DNS-type rolling counts, kept alongside (not inside) `buckets` so
    /// the per-client buckets stay lean. `#[serde(default)]` so a pre-history
    /// snapshot still loads — the per-type series just starts empty.
    #[serde(default)]
    type_counts: HourlyTypeCounts,
}

impl Aggregates {
    /// Heap owned by the aggregates. `buckets` and `type_counts` are
    /// fixed-size arrays living inline in this struct, so they contribute
    /// their own inline size once here and allocate nothing further; only the
    /// two bounded domain counters own heap.
    pub(crate) fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.top_blocked.heap_bytes() + self.top_queried.heap_bytes()
    }

    pub fn record(
        &mut self,
        domain: &str,
        qtype: &QueryType,
        verdict: &Verdict,
        cache_hit: bool,
        at: SystemTime,
    ) {
        let blocked = matches!(verdict, Verdict::Block(_));

        self.top_queried.record(domain, at);
        if blocked {
            self.top_blocked.record(domain, at);
        }
        self.buckets.record(at, blocked, cache_hit);
        self.type_counts.record(at, qtype_index(qtype));
    }

    /// Completed-hour rollups for the history writer: joins each completed
    /// hour's totals with its per-type breakdown by `hour_epoch`. Off the hot
    /// path — called on the flush cadence.
    pub fn completed_hour_rollups(&self, now: SystemTime) -> Vec<fah_model::HourRollup> {
        self.buckets
            .completed_hours(now)
            .into_iter()
            .map(|hour| fah_model::HourRollup {
                hour_epoch: hour.hour_epoch,
                queries: hour.queries,
                blocked: hour.blocked,
                cache_hits: hour.cache_hits,
                per_type: self.type_counts.per_type_for(hour.hour_epoch),
            })
            .collect()
    }

    pub fn queries_total(&self, now: SystemTime) -> u64 {
        self.buckets.totals(now).queries
    }

    pub fn blocked_total(&self, now: SystemTime) -> u64 {
        self.buckets.totals(now).blocked
    }

    pub fn blocked_percent(&self, now: SystemTime) -> f64 {
        let totals = self.buckets.totals(now);
        percent(totals.blocked, totals.queries)
    }

    pub fn cache_hit_percent(&self, now: SystemTime) -> f64 {
        let totals = self.buckets.totals(now);
        percent(totals.cache_hits, totals.queries)
    }

    pub fn top_blocked(&self, n: usize, now: SystemTime) -> Vec<(Arc<str>, u64)> {
        self.top_blocked.top(n, now)
    }

    pub fn top_queried(&self, n: usize, now: SystemTime) -> Vec<(Arc<str>, u64)> {
        self.top_queried.top(n, now)
    }

    pub fn buckets(&self, now: SystemTime) -> Vec<BucketView> {
        self.buckets.snapshot(now)
    }
}

fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use fah_model::DecisiveRule;

    use super::*;

    fn block() -> Verdict {
        Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^"))
    }

    #[test]
    fn totals_and_percentages_track_recorded_events() {
        let mut aggregates = Aggregates::default();
        let now = SystemTime::now();
        aggregates.record("ads.example.com", &QueryType::A, &block(), false, now);
        aggregates.record("example.com", &QueryType::A, &Verdict::Pass, true, now);

        assert_eq!(aggregates.queries_total(now), 2);
        assert_eq!(aggregates.blocked_total(now), 1);
        assert_eq!(aggregates.blocked_percent(now), 50.0);
        assert_eq!(aggregates.cache_hit_percent(now), 50.0);
    }

    #[test]
    fn percent_on_zero_queries_is_zero_not_nan() {
        let aggregates = Aggregates::default();
        let now = SystemTime::now();
        assert_eq!(aggregates.blocked_percent(now), 0.0);
        assert_eq!(aggregates.cache_hit_percent(now), 0.0);
    }

    #[test]
    fn top_blocked_only_counts_blocked_queries() {
        let mut aggregates = Aggregates::default();
        let now = SystemTime::now();
        aggregates.record("ads.example.com", &QueryType::A, &block(), false, now);
        aggregates.record("example.com", &QueryType::A, &Verdict::Pass, false, now);

        let top_blocked = aggregates.top_blocked(10, now);
        assert_eq!(top_blocked.len(), 1);
        assert_eq!(top_blocked[0].0.as_ref(), "ads.example.com");

        assert_eq!(aggregates.top_queried(10, now).len(), 2);
    }

    #[test]
    fn everything_is_windowed_to_the_last_24h() {
        use std::time::{Duration, UNIX_EPOCH};
        let hours = |n: u64| UNIX_EPOCH + Duration::from_secs(n * 3600);

        let mut aggregates = Aggregates::default();
        aggregates.record(
            "old-ads.example.com",
            &QueryType::A,
            &block(),
            true,
            hours(10),
        );
        aggregates.record(
            "fresh.example.com",
            &QueryType::A,
            &Verdict::Pass,
            false,
            hours(20),
        );

        let now = hours(10 + 24); // the first record just aged out
        assert_eq!(aggregates.queries_total(now), 1);
        assert_eq!(aggregates.blocked_total(now), 0);
        assert_eq!(aggregates.cache_hit_percent(now), 0.0);
        assert!(aggregates.top_blocked(10, now).is_empty());
        assert_eq!(aggregates.top_queried(10, now).len(), 1);
    }

    #[test]
    fn tracked_domain_count_stays_bounded_under_sustained_load() {
        let mut aggregates = Aggregates::default();
        let now = SystemTime::now();
        for i in 0..50_000u64 {
            aggregates.record(
                &format!("domain{i}.example.com"),
                &QueryType::A,
                &Verdict::Pass,
                false,
                now,
            );
        }
        assert!(aggregates.top_queried.tracked_keys() <= 24 * 256);
    }
}
