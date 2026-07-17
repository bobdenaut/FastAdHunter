use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::query::Query;
use crate::verdict::Verdict;

/// The channel DTO carrying a completed query from the hot path to Query Log,
/// Statistics and Metrics: query, verdict, duration, cache/upstream flags
/// (CONTEXT.md: Query Log, Statistics, Metrics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEvent {
    pub query: Query,
    pub verdict: Verdict,
    pub duration: Duration,
    pub cache_hit: bool,
    pub upstream_used: bool,
}

impl QueryEvent {
    pub fn new(
        query: Query,
        verdict: Verdict,
        duration: Duration,
        cache_hit: bool,
        upstream_used: bool,
    ) -> Self {
        Self {
            query,
            verdict,
            duration,
            cache_hit,
            upstream_used,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::time::SystemTime;

    use super::*;
    use crate::query::QueryType;

    #[test]
    fn query_event_serde_roundtrip() {
        let event = QueryEvent::new(
            Query::new(
                "ads.example.com",
                QueryType::A,
                IpAddr::from([192, 168, 1, 10]),
                SystemTime::UNIX_EPOCH,
            ),
            Verdict::Pass,
            Duration::from_micros(250),
            false,
            true,
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: QueryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }
}
