use std::time::Duration;

pub(crate) const BACKOFF_INITIAL: Duration = Duration::from_millis(10);
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(1);
pub(crate) const FATAL_CONSECUTIVE_ERRORS: u32 = 40;

pub(crate) enum RetryDecision {
    Sleep(Duration),
    Fatal,
}

#[derive(Default)]
pub(crate) struct RetryPolicy {
    consecutive: u32,
}

impl RetryPolicy {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn on_error(&mut self) -> RetryDecision {
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive >= FATAL_CONSECUTIVE_ERRORS {
            return RetryDecision::Fatal;
        }
        let factor = 1u32.checked_shl(self.consecutive - 1).unwrap_or(u32::MAX);
        RetryDecision::Sleep(BACKOFF_INITIAL.saturating_mul(factor).min(BACKOFF_MAX))
    }

    pub(crate) fn on_success(&mut self) {
        self.consecutive = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sleep_millis(decision: RetryDecision) -> u128 {
        match decision {
            RetryDecision::Sleep(delay) => delay.as_millis(),
            RetryDecision::Fatal => panic!("expected a retry, got a fatal escalation"),
        }
    }

    #[test]
    fn the_delay_doubles_from_ten_milliseconds_and_holds_at_one_second() {
        let mut policy = RetryPolicy::new();
        for expected in [10, 20, 40, 80, 160, 320, 640, 1000, 1000, 1000] {
            assert_eq!(sleep_millis(policy.on_error()), expected);
        }
    }

    #[test]
    fn every_error_below_the_threshold_retries_and_the_threshold_itself_is_fatal() {
        let mut policy = RetryPolicy::new();
        for attempt in 1..FATAL_CONSECUTIVE_ERRORS {
            assert!(
                matches!(policy.on_error(), RetryDecision::Sleep(_)),
                "error {attempt} must retry"
            );
        }
        assert!(matches!(policy.on_error(), RetryDecision::Fatal));
    }

    #[test]
    fn a_successful_receive_resets_the_progression() {
        let mut policy = RetryPolicy::new();
        for _ in 1..FATAL_CONSECUTIVE_ERRORS {
            policy.on_error();
        }
        policy.on_success();
        assert_eq!(sleep_millis(policy.on_error()), BACKOFF_INITIAL.as_millis());
    }

    #[test]
    fn a_capped_delay_never_exceeds_the_maximum() {
        let mut policy = RetryPolicy::new();
        for _ in 1..FATAL_CONSECUTIVE_ERRORS {
            assert!(sleep_millis(policy.on_error()) <= BACKOFF_MAX.as_millis());
        }
    }
}
