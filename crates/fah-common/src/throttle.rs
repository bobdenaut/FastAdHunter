use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct LogThrottle {
    origin: Instant,
    last_millis: AtomicU64,
    total: AtomicU64,
    interval_millis: u64,
}

impl LogThrottle {
    pub fn new(interval: Duration) -> Self {
        Self {
            origin: Instant::now(),
            last_millis: AtomicU64::new(0),
            total: AtomicU64::new(0),
            interval_millis: u64::try_from(interval.as_millis()).unwrap_or(u64::MAX),
        }
    }

    pub fn note(&self, now: Instant) -> Option<u64> {
        let total = self.total.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let stamp = self.stamp(now);
        let mut last = self.last_millis.load(Ordering::Relaxed);
        loop {
            if last != 0 && stamp.saturating_sub(last) < self.interval_millis {
                return None;
            }
            match self.last_millis.compare_exchange_weak(
                last,
                stamp,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(total),
                Err(current) => last = current,
            }
        }
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    fn stamp(&self, now: Instant) -> u64 {
        let millis = now.saturating_duration_since(self.origin).as_millis();
        u64::try_from(millis).unwrap_or(u64::MAX).saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(60);

    fn throttle() -> (LogThrottle, Instant) {
        let throttle = LogThrottle::new(INTERVAL);
        let origin = throttle.origin;
        (throttle, origin)
    }

    #[test]
    fn the_first_event_is_always_logged_and_carries_its_own_count() {
        let (throttle, now) = throttle();
        assert_eq!(throttle.note(now), Some(1));
    }

    #[test]
    fn events_inside_the_interval_are_suppressed() {
        let (throttle, now) = throttle();
        assert_eq!(throttle.note(now), Some(1));
        assert_eq!(throttle.note(now + Duration::from_secs(1)), None);
        assert_eq!(
            throttle.note(now + INTERVAL - Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn the_interval_boundary_logs_again_with_the_cumulative_total() {
        let (throttle, now) = throttle();
        assert_eq!(throttle.note(now), Some(1));
        for _ in 0..98 {
            assert_eq!(throttle.note(now), None);
        }
        assert_eq!(throttle.note(now + INTERVAL), Some(100));
    }

    #[test]
    fn the_total_is_never_reset_so_successive_lines_only_grow() {
        let (throttle, now) = throttle();
        let first = throttle.note(now).expect("the first event logs");
        let second = throttle
            .note(now + INTERVAL)
            .expect("the interval has elapsed");
        let third = throttle
            .note(now + INTERVAL * 2)
            .expect("the interval has elapsed again");
        assert!(first < second && second < third);
        assert_eq!(throttle.total(), 3);
    }

    #[test]
    fn a_suppressed_event_still_counts_toward_the_next_line() {
        let (throttle, now) = throttle();
        throttle.note(now);
        throttle.note(now);
        throttle.note(now);
        assert_eq!(throttle.note(now + INTERVAL), Some(4));
    }

    #[test]
    fn a_zero_interval_logs_every_event() {
        let throttle = LogThrottle::new(Duration::ZERO);
        let now = throttle.origin;
        assert_eq!(throttle.note(now), Some(1));
        assert_eq!(throttle.note(now), Some(2));
    }

    #[test]
    fn real_time_zero_is_not_mistaken_for_never_logged() {
        let (throttle, now) = throttle();
        assert_eq!(throttle.note(now), Some(1));
        assert_ne!(throttle.last_millis.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn concurrent_events_produce_one_line_per_interval_and_lose_no_count() {
        const THREADS: usize = 8;
        const PER_THREAD: usize = 250;

        let throttle = LogThrottle::new(INTERVAL);
        let now = throttle.origin;
        let shared = std::sync::Arc::new(throttle);
        let lines = std::sync::Arc::new(AtomicU64::new(0));

        let workers: Vec<_> = (0..THREADS)
            .map(|_| {
                let shared = std::sync::Arc::clone(&shared);
                let lines = std::sync::Arc::clone(&lines);
                std::thread::spawn(move || {
                    for _ in 0..PER_THREAD {
                        if shared.note(now).is_some() {
                            lines.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("no worker panics");
        }

        assert_eq!(lines.load(Ordering::Relaxed), 1);
        assert_eq!(
            shared.total(),
            u64::try_from(THREADS * PER_THREAD).expect("the product fits a u64")
        );
    }
}
