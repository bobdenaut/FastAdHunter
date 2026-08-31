//! A fixed-bucket histogram, hot-path safe: no locks, no allocation per
//! observation — just atomic increments over a small static bucket array
//! (PERFORMANCE.md: no global locks, allocation-free hot path).
//!
//! Boundaries bracket PERFORMANCE.md's sub-millisecond budgets (verdict+cache
//! hit, blocked query, forwarded-query overhead all target p99 < 1 ms) with
//! enough resolution below 1 ms to tell a healthy p99 from one creeping
//! toward the budget.

use std::time::Duration;

use fah_common::histogram::AtomicHistogram;

pub(crate) const BUCKETS_SECONDS: [f64; 11] = [
    0.0001, 0.00025, 0.0005, 0.00075, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1,
];

pub(crate) struct Histogram(AtomicHistogram<{ BUCKETS_SECONDS.len() }>);

impl Histogram {
    pub(crate) fn new() -> Self {
        Self(AtomicHistogram::new(&BUCKETS_SECONDS))
    }

    pub(crate) fn observe(&self, duration: Duration) {
        self.0.observe(duration);
    }

    pub(crate) fn cumulative_counts(&self) -> Vec<u64> {
        self.0.cumulative().to_vec()
    }

    pub(crate) fn count(&self) -> u64 {
        self.0.count()
    }

    pub(crate) fn sum_seconds(&self) -> f64 {
        self.0.sum_seconds()
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
