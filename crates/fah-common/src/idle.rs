use std::time::{Duration, SystemTime};

pub fn older_than(last_seen: SystemTime, now: SystemTime, limit: Duration) -> bool {
    now.duration_since(last_seen).is_ok_and(|age| age > limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    const DAY: Duration = Duration::from_secs(86_400);

    #[test]
    fn an_age_equal_to_the_limit_is_not_older() {
        assert!(!older_than(now() - DAY, now(), DAY));
        assert!(!older_than(now(), now(), DAY));
    }

    #[test]
    fn one_second_past_the_limit_is_older() {
        assert!(older_than(now() - DAY - Duration::from_secs(1), now(), DAY));
    }

    #[test]
    fn a_backwards_clock_step_is_never_older() {
        assert!(!older_than(now() + DAY, now(), DAY));
        assert!(!older_than(
            now() + Duration::from_secs(1),
            now(),
            Duration::ZERO
        ));
    }
}
