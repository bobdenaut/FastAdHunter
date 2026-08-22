use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::query::Query;
use crate::verdict::Verdict;

/// Why an expired cache entry answered a query. The two paths share only the
/// entry's age: one is a pure cache read, the other carries a failed upstream
/// round trip, so pooling their latencies hides the difference that matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleServe {
    /// Stale-while-refresh (ADR-0005): answered from cache at cache-read
    /// latency while a detached refresh runs. The client never waited on the
    /// network, so this belongs with the cache hits.
    FromSwr,
    /// RFC 8767 §4 minimal serve-stale: the forward attempt failed and the
    /// expired entry is the fallback, so the duration includes the upstream
    /// timeout and belongs with the forwards.
    AfterForwardFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerOutcome {
    #[default]
    Answered,
    ServfailSynthesized,
    ServfailRelayed,
    RefusedRelayed,
}

impl AnswerOutcome {
    pub fn is_answered(&self) -> bool {
        matches!(self, Self::Answered)
    }
}

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
    /// Set when an expired cache entry answered, naming which path produced
    /// it ([`StaleServe`]). Implies `cache_hit`. A bool here pooled the SWR
    /// serve — a sub-100 µs cache read — with the outage fallback that
    /// carries an upstream timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<StaleServe>,
    /// Policy that judged the query, `None` for the default (p2-06). Absent
    /// from the JSON rather than null, so a deployment with no policies logs
    /// exactly what it logged before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<Arc<str>>,
    #[serde(default, skip_serializing_if = "AnswerOutcome::is_answered")]
    pub answer: AnswerOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<u8>,
}

impl QueryEvent {
    pub fn new(
        query: Query,
        verdict: Verdict,
        duration: Duration,
        cache_hit: bool,
        upstream_used: bool,
        stale: Option<StaleServe>,
    ) -> Self {
        Self {
            query,
            verdict,
            duration,
            cache_hit,
            upstream_used,
            stale,
            policy: None,
            answer: AnswerOutcome::Answered,
            endpoint: None,
        }
    }

    pub fn with_outcome(mut self, answer: AnswerOutcome, endpoint: Option<u8>) -> Self {
        self.answer = answer;
        self.endpoint = endpoint;
        self
    }

    /// Records which policy decided this — a refcount bump, never an
    /// allocation, since it runs on the query path.
    pub fn under_policy(mut self, policy: Option<Arc<str>>) -> Self {
        self.policy = policy;
        self
    }

    /// Whether an expired entry answered, either way — for consumers that
    /// count stale serves without caring which path produced them.
    pub fn is_stale(&self) -> bool {
        self.stale.is_some()
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
            None,
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: QueryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    /// The two stale paths must survive the channel as distinct values —
    /// collapsing them is exactly what the bool did.
    #[test]
    fn stale_reason_roundtrips_and_stays_distinct() {
        let with = |stale| {
            QueryEvent::new(
                Query::new(
                    "example.com",
                    QueryType::A,
                    IpAddr::from([192, 168, 1, 10]),
                    SystemTime::UNIX_EPOCH,
                ),
                Verdict::Pass,
                Duration::from_micros(250),
                true,
                false,
                stale,
            )
        };
        for stale in [
            None,
            Some(StaleServe::FromSwr),
            Some(StaleServe::AfterForwardFailure),
        ] {
            let event = with(stale);
            let back: QueryEvent = serde_json::from_str(&serde_json::to_string(&event).unwrap())
                .expect("stale reason must survive the channel");
            assert_eq!(event, back);
            assert_eq!(back.is_stale(), stale.is_some());
        }
        assert_ne!(
            with(Some(StaleServe::FromSwr)),
            with(Some(StaleServe::AfterForwardFailure))
        );
    }

    #[test]
    fn answer_outcome_and_endpoint_survive_the_channel() {
        let base = QueryEvent::new(
            Query::new(
                "example.com",
                QueryType::A,
                IpAddr::from([192, 168, 1, 10]),
                SystemTime::UNIX_EPOCH,
            ),
            Verdict::Pass,
            Duration::from_micros(250),
            false,
            true,
            None,
        );
        for (answer, endpoint, name) in [
            (AnswerOutcome::Answered, Some(0), "answered"),
            (
                AnswerOutcome::ServfailSynthesized,
                None,
                "servfail_synthesized",
            ),
            (AnswerOutcome::ServfailRelayed, Some(1), "servfail_relayed"),
            (AnswerOutcome::RefusedRelayed, Some(2), "refused_relayed"),
        ] {
            let event = base.clone().with_outcome(answer, endpoint);
            let json = serde_json::to_string(&event).unwrap();
            let back: QueryEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, back);
            assert_eq!(back.answer, answer);
            assert_eq!(back.endpoint, endpoint);
            assert_eq!(json.contains(name), !answer.is_answered());
        }
        assert!(!serde_json::to_string(&base).unwrap().contains("answer"));
    }

    #[test]
    fn pre_change_event_json_deserializes_to_answered_without_an_endpoint() {
        let json = r#"{
            "query": {
                "domain": "ads.example.com",
                "qtype": "A",
                "client_ip": "192.168.1.10",
                "timestamp": {"secs_since_epoch": 0, "nanos_since_epoch": 0}
            },
            "verdict": "Pass",
            "duration": {"secs": 0, "nanos": 250000},
            "cache_hit": false,
            "upstream_used": true
        }"#;
        let event: QueryEvent = serde_json::from_str(json).expect("old events must still parse");
        assert_eq!(event.answer, AnswerOutcome::Answered);
        assert_eq!(event.endpoint, None);
        assert!(event.upstream_used);
    }
}
