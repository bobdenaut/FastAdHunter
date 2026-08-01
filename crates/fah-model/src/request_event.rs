use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::http::ResourceType;
use crate::query_event::QueryEvent;
use crate::verdict::Verdict;

/// One HTTP request received from a client, as the pipeline reports it
/// (CONTEXT.md: HTTP Request).
///
/// The counterpart of [`crate::Query`], and deliberately its own type rather
/// than a widened one: a request has no record type and a query has no method,
/// path, status or byte count. Owned `String`s because this crosses a channel
/// and outlives the connection — the borrowed [`crate::HttpRequest`] the
/// matcher takes is the hot-path shape, this is the reporting shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Host the request was addressed to, **without port** — the same
    /// normalization the matcher requires, so the log and the verdict agree
    /// about what was asked for.
    pub host: String,
    /// Path and query string, as sent.
    pub path: String,
    pub method: String,
    pub resource_type: ResourceType,
    pub client_ip: IpAddr,
    pub timestamp: SystemTime,
}

/// The channel DTO carrying a completed HTTP request from the proxy to Query
/// Log, Statistics and Metrics — the HTTP half of what [`QueryEvent`] does for
/// DNS (CONTEXT.md: Query Log, Statistics, Metrics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEvent {
    pub request: Request,
    pub verdict: Verdict,
    pub duration: Duration,
    /// Status returned to the client — a synthesized block's status as much as
    /// an origin's, so "what did the client actually get" is answerable
    /// without re-deriving it from the verdict.
    pub status: u16,
    /// Response body bytes relayed downstream. Zero for a block, which is the
    /// number that makes "blocked requests die cheaply" measurable rather than
    /// merely asserted.
    pub bytes: u64,
}

impl RequestEvent {
    pub fn new(
        request: Request,
        verdict: Verdict,
        duration: Duration,
        status: u16,
        bytes: u64,
    ) -> Self {
        Self {
            request,
            verdict,
            duration,
            status,
            bytes,
        }
    }
}

/// What the pipeline's event channel carries.
///
/// **One channel, not two.** The alternative — a second mpsc for HTTP — has a
/// smaller blast radius, and forfeits the property the p1.5 metrics work
/// existed to establish: *one* bounded queue, *one* `dropped_events` counter,
/// one number for "we shed N events". Two independent drop counters cannot be
/// added together into that number, because they answer different questions
/// about different queues. The shed figure is an observability primitive, so
/// the cost of widening the item is paid here instead.
///
/// Boxing is deliberate: [`QueryEvent`] and [`RequestEvent`] differ enough in
/// size that an unboxed enum would make every DNS event pay for the larger
/// variant, on the channel the DNS hot path writes to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Event {
    Dns(Box<QueryEvent>),
    Http(Box<RequestEvent>),
}

impl Event {
    pub fn dns(event: QueryEvent) -> Self {
        Self::Dns(Box::new(event))
    }

    pub fn http(event: RequestEvent) -> Self {
        Self::Http(Box::new(event))
    }

    /// The stable `dns` / `http` discriminator the query log stores and the
    /// API filters on. A method rather than a derived string so the two can
    /// never disagree about spelling.
    pub fn kind(&self) -> EventKind {
        match self {
            Event::Dns(_) => EventKind::Dns,
            Event::Http(_) => EventKind::Http,
        }
    }

    pub fn client_ip(&self) -> IpAddr {
        match self {
            Event::Dns(event) => event.query.client_ip,
            Event::Http(event) => event.request.client_ip,
        }
    }

    pub fn timestamp(&self) -> SystemTime {
        match self {
            Event::Dns(event) => event.query.timestamp,
            Event::Http(event) => event.request.timestamp,
        }
    }

    pub fn verdict(&self) -> &Verdict {
        match self {
            Event::Dns(event) => &event.verdict,
            Event::Http(event) => &event.verdict,
        }
    }
}

/// Which pipeline produced an event. Persisted in the query log and accepted as
/// a `GET /api/v1/queries` filter (API.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Dns,
    Http,
}

impl EventKind {
    /// Stable wire/label spelling — used by the API filter, the query-log
    /// record and the metric label alike.
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Dns => "dns",
            EventKind::Http => "http",
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EventKind {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "dns" => Ok(EventKind::Dns),
            "http" => Ok(EventKind::Http),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Query, QueryType};

    fn request() -> Request {
        Request {
            host: "ads.example.com".to_string(),
            path: "/pixel.gif?id=1".to_string(),
            method: "GET".to_string(),
            resource_type: ResourceType::Image,
            client_ip: IpAddr::from([192, 168, 1, 10]),
            timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn request_event_serde_roundtrip() {
        let event = RequestEvent::new(
            request(),
            Verdict::Pass,
            Duration::from_micros(900),
            200,
            4096,
        );
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<RequestEvent>(&json).unwrap(), event);
    }

    #[test]
    fn event_serde_roundtrip_for_both_variants() {
        let dns = Event::dns(QueryEvent::new(
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
            false,
        ));
        let http = Event::http(RequestEvent::new(
            request(),
            Verdict::Pass,
            Duration::from_micros(900),
            200,
            0,
        ));
        for event in [dns, http] {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event);
        }
    }

    /// The discriminator reaches the query log, the API filter and a metric
    /// label. One spelling, asserted, so those three cannot drift.
    #[test]
    fn event_kind_spelling_is_stable_and_round_trips() {
        for (kind, text) in [(EventKind::Dns, "dns"), (EventKind::Http, "http")] {
            assert_eq!(kind.as_str(), text);
            assert_eq!(kind.to_string(), text);
            assert_eq!(text.parse::<EventKind>(), Ok(kind));
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{text}\""));
        }
        assert_eq!("dnssec".parse::<EventKind>(), Err(()));
    }

    #[test]
    fn event_exposes_the_common_fields_without_matching() {
        let event = Event::http(RequestEvent::new(
            request(),
            Verdict::Pass,
            Duration::ZERO,
            200,
            0,
        ));
        assert_eq!(event.kind(), EventKind::Http);
        assert_eq!(event.client_ip(), IpAddr::from([192, 168, 1, 10]));
        assert_eq!(event.timestamp(), SystemTime::UNIX_EPOCH);
        assert_eq!(event.verdict(), &Verdict::Pass);
    }
}
