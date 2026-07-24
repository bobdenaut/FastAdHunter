//! A point-in-time read of the whole registry ([`Metrics::snapshot`]) plus the
//! per-stage latency histograms in a form a sampler can turn into a persisted
//! `fah_model::PerfSample` (p1.5-02). This is an in-process DTO, not a
//! Prometheus rendering (that stays in [`crate::encode`]) and not persisted
//! itself — the binary maps it onto the `fah-model` record it writes.
//!
//! The counters are lifetime-cumulative (as every `Metrics` counter is), so
//! the sampler deltas consecutive snapshots to get per-interval rates. For the
//! latency histograms that means diffing the cumulative bucket counts
//! ([`StageHistogram::delta`]) before estimating a percentile
//! ([`StageHistogram::quantile`]) — otherwise the percentile would reflect the
//! whole process lifetime and barely move between samples.

use crate::histogram::BUCKETS_SECONDS;
use crate::upstream::UpstreamSnapshot;

/// A whole-registry read at one instant.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub queries_pass: u64,
    pub queries_allow: u64,
    pub queries_block: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_stale: u64,
    pub dropped_events: u64,
    /// In-engine blocked-query latency (PERFORMANCE.md <1 ms p99 budget).
    pub block: StageHistogram,
    /// In-engine cache-hit latency (PERFORMANCE.md <1 ms p99 budget).
    pub cache_hit: StageHistogram,
    /// End-to-end forwarded-query latency, including the upstream round trip.
    pub forward: StageHistogram,
    pub upstreams: Vec<UpstreamSnapshot>,
}

/// One latency stage captured at an instant: cumulative bucket counts (bucket
/// `i` = observations `<= BUCKETS_SECONDS[i]`), the total `count` (including
/// the implicit `+Inf` bucket), and the running sum. Cheap to diff into an
/// interval histogram so a sampler reports per-interval percentiles rather
/// than lifetime-cumulative ones.
#[derive(Debug, Clone)]
pub struct StageHistogram {
    /// Cumulative counts aligned to [`BUCKETS_SECONDS`]; `cumulative.len()`
    /// equals `BUCKETS_SECONDS.len()`.
    pub cumulative: Vec<u64>,
    pub count: u64,
    pub sum_seconds: f64,
}

impl StageHistogram {
    /// Interval histogram = `self − prev`, elementwise. Both are cumulative
    /// counts from the same monotonic histogram, so bucket `i` of the result
    /// (`cur_i − prev_i`) is exactly "observations in `(prev, cur]` that are
    /// `<= BUCKETS_SECONDS[i]`" — itself a valid cumulative histogram for the
    /// window. `saturating_sub` guards the (racy-read) corner where a counter
    /// appears to have gone backwards.
    pub fn delta(&self, prev: &StageHistogram) -> StageHistogram {
        let cumulative = self
            .cumulative
            .iter()
            .zip(prev.cumulative.iter())
            .map(|(cur, prev)| cur.saturating_sub(*prev))
            .collect();
        StageHistogram {
            cumulative,
            count: self.count.saturating_sub(prev.count),
            sum_seconds: (self.sum_seconds - prev.sum_seconds).max(0.0),
        }
    }

    /// Quantile estimate in **seconds**: the smallest bucket upper bound whose
    /// cumulative count reaches the `q`-th observation. Coarse by design
    /// (bucket granularity), saturating at the top finite bound when the
    /// quantile falls in the `+Inf` bucket. `0.0` when empty. `q` is a
    /// fraction in `[0, 1]`.
    pub fn quantile(&self, q: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        // The q-th observation, 1-based: p99 of 100 samples is the 99th.
        let target = ((q * self.count as f64).ceil() as u64).max(1);
        for (idx, &cumulative) in self.cumulative.iter().enumerate() {
            if cumulative >= target {
                return BUCKETS_SECONDS[idx];
            }
        }
        // Beyond the largest finite bucket (the `+Inf` region): report the top
        // finite bound as a floor rather than inventing a value.
        *BUCKETS_SECONDS.last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(cumulative: Vec<u64>, count: u64) -> StageHistogram {
        StageHistogram {
            cumulative,
            count,
            sum_seconds: 0.0,
        }
    }

    #[test]
    fn empty_stage_reports_zero_quantiles() {
        let hist = stage(vec![0; BUCKETS_SECONDS.len()], 0);
        assert_eq!(hist.quantile(0.5), 0.0);
        assert_eq!(hist.quantile(0.99), 0.0);
    }

    #[test]
    fn quantile_returns_the_bucket_bound_reaching_the_target() {
        // 10 observations, all in the first bucket (<= 0.0001s).
        let mut cumulative = vec![10u64; BUCKETS_SECONDS.len()];
        cumulative[0] = 10;
        let hist = stage(cumulative, 10);
        assert_eq!(hist.quantile(0.5), BUCKETS_SECONDS[0]);
        assert_eq!(hist.quantile(0.99), BUCKETS_SECONDS[0]);
    }

    #[test]
    fn quantile_saturates_at_the_top_bound_when_target_is_in_plus_inf() {
        // 10 observations total, but only 4 fell into finite buckets — the
        // rest are in +Inf, so p99 saturates at the last finite bound.
        let mut cumulative = vec![4u64; BUCKETS_SECONDS.len()];
        cumulative[0] = 1;
        cumulative[1] = 2;
        cumulative[2] = 4;
        let hist = stage(cumulative, 10);
        assert_eq!(hist.quantile(0.99), *BUCKETS_SECONDS.last().unwrap());
    }

    #[test]
    fn quantile_picks_the_median_bucket() {
        // Cumulative: 2 <= b0, 5 <= b1, 10 <= b2. Median (5th of 10) lands at b1.
        let mut cumulative = vec![10u64; BUCKETS_SECONDS.len()];
        cumulative[0] = 2;
        cumulative[1] = 5;
        let hist = stage(cumulative, 10);
        assert_eq!(hist.quantile(0.5), BUCKETS_SECONDS[1]);
        assert_eq!(hist.quantile(0.99), BUCKETS_SECONDS[2]);
    }

    #[test]
    fn delta_subtracts_cumulative_and_count() {
        let prev = stage(vec![1, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3], 5);
        let cur = stage(vec![4, 6, 8, 8, 8, 8, 8, 8, 8, 8, 8], 12);
        let interval = cur.delta(&prev);
        assert_eq!(interval.cumulative[0], 3);
        assert_eq!(interval.cumulative[1], 4);
        assert_eq!(interval.cumulative[2], 5);
        assert_eq!(interval.count, 7);
        // Interval: 3 <= b0, 4 <= b1, 5 <= b2, then 2 obs in +Inf (count 7).
        // p50 (4th of 7) lands at b1; p99 (7th of 7) is in +Inf → top bound.
        assert_eq!(interval.quantile(0.5), BUCKETS_SECONDS[1]);
        assert_eq!(interval.quantile(0.99), *BUCKETS_SECONDS.last().unwrap());
    }
}
