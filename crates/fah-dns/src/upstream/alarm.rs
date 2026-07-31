//! Rate limiter for the "every upstream failed" alarm.
//!
//! A total upstream outage is the one failure the operator has to hear about:
//! with encrypted-only upstreams there is no plaintext fallback, so clients are
//! living on whatever the cache can still serve (ADR-0005). Before this, the
//! condition was silent — [`super::UpstreamServer`] attempts log at `debug!`,
//! below the shipped default level, and `forward` returned the error without
//! saying anything.
//!
//! Logging it per query is not an option either: at query rate a sustained
//! outage would bury `/log` and cost more than the outage. Hence one line per
//! interval, carrying the number of failures it stands for — and the whole
//! state is three atomics and a fixed epoch, so remembering that we already
//! complained costs no memory that grows with the outage (CLAUDE.md rule 4).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// One warning per this long while every upstream is down. Long enough that a
/// multi-hour outage stays readable, short enough that the log says the
/// resolver is *still* broken rather than only that it once was.
const WARN_INTERVAL: Duration = Duration::from_secs(30);

/// `last_warn_ms` when no warning has been emitted since the last recovery —
/// distinct from 0, which is a legitimate "warned at the epoch".
const NEVER: u64 = u64::MAX;

pub(super) struct FailureAlarm {
    epoch: Instant,
    interval_ms: u64,
    /// Millis since `epoch` at the last emitted warning, or [`NEVER`].
    last_warn_ms: AtomicU64,
    /// Failures swallowed since that warning — reported and cleared by the
    /// next one, so the log states what the outage actually cost.
    suppressed: AtomicU64,
}

impl FailureAlarm {
    pub(super) fn new() -> Self {
        Self::with_interval(WARN_INTERVAL)
    }

    /// Test seam: the unit tests use a few-millisecond interval so crossing a
    /// window does not mean sleeping for the production one.
    fn with_interval(interval: Duration) -> Self {
        Self {
            epoch: Instant::now(),
            interval_ms: duration_ms(interval),
            last_warn_ms: AtomicU64::new(NEVER),
            suppressed: AtomicU64::new(0),
        }
    }

    /// Claims the right to warn now. `Some(n)` means the caller should log,
    /// standing for `n` failures suppressed since the previous warning;
    /// `None` means another failure already warned inside this window and this
    /// one has been counted instead.
    pub(super) fn claim(&self) -> Option<u64> {
        let now = self.now_ms();
        let last = self.last_warn_ms.load(Ordering::Relaxed);
        if last != NEVER && now.saturating_sub(last) < self.interval_ms {
            self.suppressed.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        // Concurrent failures race for the slot; exactly one wins and the
        // losers count themselves as suppressed, so a burst still yields one
        // line rather than one per worker.
        if self
            .last_warn_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            self.suppressed.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Some(self.suppressed.swap(0, Ordering::Relaxed))
    }

    /// The pool answered again. Returns `true` exactly once per outage — for
    /// the caller to log the recovery — and re-arms the alarm so the next
    /// outage warns immediately instead of waiting out a stale window.
    ///
    /// This runs on every successful forward, so the healthy path is one
    /// relaxed load that returns early.
    pub(super) fn clear(&self) -> bool {
        if self.last_warn_ms.load(Ordering::Relaxed) == NEVER {
            return false;
        }
        self.suppressed.store(0, Ordering::Relaxed);
        // Whoever swaps the non-sentinel value out owns the recovery message.
        self.last_warn_ms.swap(NEVER, Ordering::Relaxed) != NEVER
    }

    fn now_ms(&self) -> u64 {
        duration_ms(self.epoch.elapsed())
    }
}

/// Milliseconds, saturating below the [`NEVER`] sentinel. The clamp is
/// unreachable in practice — it needs ~584 million years of uptime — but it
/// keeps the sentinel unambiguous without an `as` cast.
fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(NEVER - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Short enough to cross inside a test, long enough that a scheduling
    /// hiccup does not cross it by accident.
    const TEST_INTERVAL: Duration = Duration::from_millis(80);

    fn alarm() -> FailureAlarm {
        FailureAlarm::with_interval(TEST_INTERVAL)
    }

    #[test]
    fn first_failure_warns_immediately() {
        // An outage must be visible at once, not one interval late.
        assert_eq!(alarm().claim(), Some(0));
    }

    #[test]
    fn failures_inside_the_window_are_counted_not_logged() {
        let alarm = alarm();
        assert_eq!(alarm.claim(), Some(0));
        for _ in 0..5 {
            assert_eq!(alarm.claim(), None, "must not log again inside the window");
        }
        std::thread::sleep(TEST_INTERVAL * 2);
        assert_eq!(
            alarm.claim(),
            Some(5),
            "the next warning must report what the silence cost"
        );
        std::thread::sleep(TEST_INTERVAL * 2);
        assert_eq!(alarm.claim(), Some(0), "the count clears once reported");
    }

    #[test]
    fn recovery_is_reported_once_then_rearms() {
        let alarm = alarm();
        assert_eq!(alarm.claim(), Some(0));

        assert!(alarm.clear(), "the first success after a warning recovers");
        assert!(!alarm.clear(), "further successes are not news");

        // Re-armed: the next outage warns straight away rather than waiting out
        // the window left over from the previous one.
        assert_eq!(alarm.claim(), Some(0));
    }

    #[test]
    fn a_pool_that_never_warned_reports_no_recovery() {
        // Every successful forward calls this; a healthy resolver must never
        // announce a recovery from an outage that did not happen.
        assert!(!alarm().clear());
    }

    #[test]
    fn suppressed_failures_do_not_survive_a_recovery() {
        let alarm = alarm();
        assert_eq!(alarm.claim(), Some(0));
        alarm.claim();
        alarm.claim();

        assert!(alarm.clear());

        std::thread::sleep(TEST_INTERVAL * 2);
        assert_eq!(
            alarm.claim(),
            Some(0),
            "a new outage must not inherit the previous one's tally"
        );
    }
}
