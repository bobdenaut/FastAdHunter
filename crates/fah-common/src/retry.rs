use std::time::Duration;

pub const BACKOFF_INITIAL: Duration = Duration::from_millis(10);
pub const BACKOFF_MAX: Duration = Duration::from_secs(1);
pub const FATAL_CONSECUTIVE_ERRORS: u32 = 40;

#[derive(Debug)]
pub enum RetryDecision {
    Sleep(Duration),
    Fatal,
}

#[derive(Debug)]
pub struct RetryPolicy {
    consecutive: u32,
    fatal_after: Option<u32>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryPolicy {
    pub fn new() -> Self {
        Self {
            consecutive: 0,
            fatal_after: Some(FATAL_CONSECUTIVE_ERRORS),
        }
    }

    pub fn never_fatal() -> Self {
        Self {
            consecutive: 0,
            fatal_after: None,
        }
    }

    pub fn on_error(&mut self) -> RetryDecision {
        self.consecutive = self.consecutive.saturating_add(1);
        if self
            .fatal_after
            .is_some_and(|limit| self.consecutive >= limit)
        {
            return RetryDecision::Fatal;
        }
        let factor = 1u32.checked_shl(self.consecutive - 1).unwrap_or(u32::MAX);
        RetryDecision::Sleep(BACKOFF_INITIAL.saturating_mul(factor).min(BACKOFF_MAX))
    }

    pub fn on_success(&mut self) {
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

    #[test]
    fn a_never_fatal_policy_keeps_sleeping_past_the_threshold() {
        let mut policy = RetryPolicy::never_fatal();
        for _ in 0..FATAL_CONSECUTIVE_ERRORS * 4 {
            assert!(matches!(policy.on_error(), RetryDecision::Sleep(_)));
        }
        assert_eq!(sleep_millis(policy.on_error()), BACKOFF_MAX.as_millis());
    }

    #[test]
    fn a_never_fatal_policy_follows_the_same_progression_as_a_fatal_one() {
        let mut fatal = RetryPolicy::new();
        let mut forgiving = RetryPolicy::never_fatal();
        for _ in 1..FATAL_CONSECUTIVE_ERRORS {
            assert_eq!(
                sleep_millis(fatal.on_error()),
                sleep_millis(forgiving.on_error())
            );
        }
    }
}
