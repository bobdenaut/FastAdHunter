use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct AtomicHistogram<const N: usize> {
    bounds: &'static [f64; N],
    buckets: [AtomicU64; N],
    count: AtomicU64,
    sum_nanos: AtomicU64,
}

impl<const N: usize> AtomicHistogram<N> {
    pub fn new(bounds: &'static [f64; N]) -> Self {
        Self {
            bounds,
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            count: AtomicU64::new(0),
            sum_nanos: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, duration: Duration) {
        let seconds = duration.as_secs_f64();
        if let Some(index) = self.bounds.iter().position(|&bound| seconds <= bound) {
            self.buckets[index].fetch_add(1, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_nanos
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn cumulative(&self) -> [u64; N] {
        let mut running = 0u64;
        std::array::from_fn(|index| {
            running += self.buckets[index].load(Ordering::Relaxed);
            running
        })
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn sum_seconds(&self) -> f64 {
        self.sum_nanos.load(Ordering::Relaxed) as f64 / 1_000_000_000.0
    }
}

pub fn quantile(bounds: &[f64], cumulative: &[u64], count: u64, q: f64) -> f64 {
    if count == 0 {
        return 0.0;
    }
    let target = ((q * count as f64).ceil() as u64).max(1);
    for (bound, reached) in bounds.iter().zip(cumulative.iter()) {
        if *reached >= target {
            return *bound;
        }
    }
    bounds.last().copied().unwrap_or(0.0)
}

pub fn saturating_delta<'a>(cur: &'a [u64], prev: &'a [u64]) -> impl Iterator<Item = u64> + 'a {
    cur.iter()
        .zip(prev.iter())
        .map(|(cur, prev)| cur.saturating_sub(*prev))
}

#[cfg(test)]
mod tests {
    use super::*;

    static BOUNDS: [f64; 3] = [0.001, 0.01, 0.1];

    #[test]
    fn an_observation_lands_in_the_smallest_bucket_that_bounds_it() {
        let histogram = AtomicHistogram::new(&BOUNDS);
        histogram.observe(Duration::from_micros(500));
        histogram.observe(Duration::from_millis(5));
        let cumulative = histogram.cumulative();
        assert_eq!(cumulative, [1, 2, 2]);
        assert_eq!(histogram.count(), 2);
    }

    #[test]
    fn an_observation_past_the_top_bound_counts_without_a_bucket() {
        let histogram = AtomicHistogram::new(&BOUNDS);
        histogram.observe(Duration::from_secs(5));
        assert_eq!(histogram.cumulative(), [0, 0, 0]);
        assert_eq!(histogram.count(), 1);
        assert!((histogram.sum_seconds() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn sum_and_count_track_every_observation() {
        let histogram = AtomicHistogram::new(&BOUNDS);
        histogram.observe(Duration::from_millis(1));
        histogram.observe(Duration::from_millis(2));
        assert_eq!(histogram.count(), 2);
        assert!((histogram.sum_seconds() - 0.003).abs() < 1e-9);
    }

    #[test]
    fn an_empty_histogram_reports_zero_quantiles() {
        assert_eq!(quantile(&BOUNDS, &[0, 0, 0], 0, 0.5), 0.0);
        assert_eq!(quantile(&BOUNDS, &[0, 0, 0], 0, 0.99), 0.0);
    }

    #[test]
    fn a_quantile_is_the_smallest_bound_reaching_the_target() {
        assert_eq!(quantile(&BOUNDS, &[2, 5, 10], 10, 0.5), BOUNDS[1]);
        assert_eq!(quantile(&BOUNDS, &[2, 5, 10], 10, 0.99), BOUNDS[2]);
        assert_eq!(quantile(&BOUNDS, &[10, 10, 10], 10, 0.99), BOUNDS[0]);
    }

    #[test]
    fn a_target_in_the_implicit_inf_bucket_saturates_at_the_top_bound() {
        assert_eq!(quantile(&BOUNDS, &[1, 2, 4], 10, 0.99), BOUNDS[2]);
    }

    #[test]
    fn the_target_is_one_based_so_a_tiny_quantile_still_reads_a_bucket() {
        assert_eq!(quantile(&BOUNDS, &[1, 1, 1], 1, 0.01), BOUNDS[0]);
    }

    #[test]
    fn delta_subtracts_elementwise_and_saturates() {
        let out: Vec<u64> = saturating_delta(&[5, 10, 3], &[2, 4, 9]).collect();
        assert_eq!(out, vec![3, 6, 0]);
    }
}
