//! A fixed-bucket histogram, hot-path safe: no locks, no allocation per
//! observation — just atomic increments over a small static bucket array
//! (PERFORMANCE.md: no global locks, allocation-free hot path).
//!
//! Boundaries bracket PERFORMANCE.md's sub-millisecond budgets (verdict+cache
//! hit, blocked query, forwarded-query overhead all target p99 < 1 ms) with
//! enough resolution below 1 ms to tell a healthy p99 from one creeping
//! toward the budget.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Upper bounds in seconds, ascending. Not cumulative — each observation
/// increments exactly one slot ([`Histogram::observe`]); cumulative counts
/// are computed at encode time ([`Histogram::cumulative_counts`]), matching
/// Prometheus's own bucket semantics (`le` = "less than or equal").
pub(crate) const BUCKETS_SECONDS: &[f64] = &[
    0.0001, 0.00025, 0.0005, 0.00075, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1,
];

pub(crate) struct Histogram {
    /// One non-cumulative counter per [`BUCKETS_SECONDS`] entry.
    buckets: [AtomicU64; BUCKETS_SECONDS.len()],
    count: AtomicU64,
    sum_nanos: AtomicU64,
}

impl Histogram {
    pub(crate) fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            count: AtomicU64::new(0),
            sum_nanos: AtomicU64::new(0),
        }
    }

    pub(crate) fn observe(&self, duration: Duration) {
        let secs = duration.as_secs_f64();
        if let Some(idx) = BUCKETS_SECONDS.iter().position(|&bound| secs <= bound) {
            self.buckets[idx].fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        // u128 -> u64: a single duration would need to run ~584 years to
        // overflow nanoseconds as u64; truncation is not a real-world risk.
        self.sum_nanos
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Running counts for each finite boundary, cumulative — bucket `i` is
    /// the count of observations `<= BUCKETS_SECONDS[i]`. The implicit `+Inf`
    /// bucket (every observation) is [`Histogram::count`].
    pub(crate) fn cumulative_counts(&self) -> Vec<u64> {
        let mut running = 0u64;
        self.buckets
            .iter()
            .map(|bucket| {
                running += bucket.load(Ordering::Relaxed);
                running
            })
            .collect()
    }

    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub(crate) fn sum_seconds(&self) -> f64 {
        self.sum_nanos.load(Ordering::Relaxed) as f64 / 1_000_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_lands_in_the_first_bucket_it_fits() {
        let hist = Histogram::new();
        hist.observe(Duration::from_micros(50)); // 0.00005s <= 0.0001 bucket
        hist.observe(Duration::from_millis(500)); // beyond every finite bucket (max 0.1s)

        let cumulative = hist.cumulative_counts();
        assert_eq!(
            cumulative[0], 1,
            "the 0.0001s bucket catches the 50us sample"
        );
        assert_eq!(
            *cumulative.last().unwrap(),
            1,
            "the last finite bucket must not catch the 500ms sample"
        );
        assert_eq!(hist.count(), 2, "+Inf (total count) catches both");
    }

    #[test]
    fn sum_and_count_track_every_observation() {
        let hist = Histogram::new();
        hist.observe(Duration::from_millis(1));
        hist.observe(Duration::from_millis(2));
        assert_eq!(hist.count(), 2);
        assert!((hist.sum_seconds() - 0.003).abs() < 1e-9);
    }
}
