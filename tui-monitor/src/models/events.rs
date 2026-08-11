//! `WS /api/v1/events` — the live tail (API.md §Events).
//!
//! The envelope is an adjacently-tagged enum, so one `from_str` decides both
//! the frame kind and its payload.

use std::net::IpAddr;

use serde::Deserialize;

use crate::models::lan::LanNames;

/// One `{ "type": …, "data": … }` frame this monitor renders.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Boxed: far larger than the other variant, and moved once per query on
    /// a busy resolver.
    Query(Box<QueryItem>),
    Stats(Box<StatsPush>),
}

/// What one frame turned out to be. Skipping and failing are separate outcomes:
/// both leave the socket up, but only one of them means this build has drifted
/// from the server.
#[derive(Debug)]
pub enum Decoded {
    Event(ServerEvent),
    /// A kind this build does not render — `config_changed`, `list_refreshed`,
    /// anything a later release adds. Expected, and not worth counting.
    Unrendered,
    /// Malformed, or a kind we *do* render whose payload no longer parses.
    /// Counted: otherwise a renamed server field empties the feed in silence.
    Undecodable,
}

/// Kinds this build renders. Only their failures are drift.
const RENDERED: [&str; 2] = ["query", "stats"];

/// Decodes one frame. Nothing here may tear down the socket.
///
/// A `#[serde(other)]` fallback variant cannot do this job: on an adjacently
/// tagged enum serde still deserializes `data` into the unit variant, which
/// fails for every frame that carries a payload.
pub fn decode(text: &str) -> Decoded {
    match serde_json::from_str::<ServerEvent>(text) {
        Ok(event) => Decoded::Event(event),
        // Second parse only on the error path, which is meant to be rare: it
        // separates "a kind we skip" from "a kind we render, but no longer can".
        Err(_) => match serde_json::from_str::<Envelope>(text) {
            Ok(envelope) if !RENDERED.contains(&envelope.kind.as_str()) => Decoded::Unrendered,
            _ => Decoded::Undecodable,
        },
    }
}

/// Just the tag, for classifying a frame whose payload would not parse.
#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Dns,
    Http,
}

/// One entry of the live feed.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryItem {
    /// `dns` | `http`.
    pub kind: EventType,
    pub ts: String,
    pub client: IpAddr,
    pub client_name: Option<String>,
    pub domain: String,
    /// DNS only — an HTTP request asks no record type.
    pub qtype: Option<String>,
    pub verdict: Verdict,
    pub rule: Option<String>,
    pub list: Option<String>,
    pub duration_ms: f64,
    pub cached: bool,
    pub method: Option<String>,
    pub path: Option<String>,
    pub status: Option<u16>,
    pub bytes: Option<u64>,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::Dns => write!(f, "dns"),
            EventType::Http => write!(f, "http"),
        }
    }
}

impl QueryItem {
    /// Used to render the cache column in the feed. The appliance does not know the cache status of HTTP requests, so it is always `-` for them.
    /// DNS queries are either a cache hit or miss, and the appliance knows which.
    const CACHE_HIT: &str = "HIT";
    const CACHE_MISS: &str = "MISS";
    const CACHE_NA: &str = "-";

    pub fn cache_label(&self) -> &'static str {
        match (self.kind, self.cached) {
            (EventType::Dns, true) => Self::CACHE_HIT,
            (EventType::Dns, false) => Self::CACHE_MISS,
            (EventType::Http, _) => Self::CACHE_NA,
        }
    }
    /// The appliance's name for the client, else one a provider resolved for
    /// its address. `None` when neither knows it, which is what tells the
    /// caller to fall back to the address.
    ///
    /// The appliance wins because its names are curated; `names` only covers
    /// what it cannot see, which is every IPv6 client. Borrowed either way:
    /// this runs for every row of the feed, every frame.
    pub fn resolved_name<'a>(&'a self, names: &'a LanNames) -> Option<&'a str> {
        self.client_name
            .as_deref()
            .or_else(|| names.get(&self.client))
    }

    /// Which family the client reached the appliance on. Its own column in the
    /// feed: a name does not say it, and scanning one is faster than reading a
    /// suffix on each row.
    pub fn family(&self) -> &'static str {
        if self.client.is_ipv6() {
            "v6"
        } else {
            "v4"
        }
    }

    /// The record type for DNS, the method for an HTTP request.
    pub fn type_label(&self) -> &str {
        self.qtype
            .as_deref()
            .or(self.method.as_deref())
            .unwrap_or("-")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Allow,
    Block,
    Pass,
    /// A verdict this build does not know — rendered neutrally rather than
    /// mis-coloured or dropped.
    #[serde(other)]
    Unknown,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Block => "block",
            Self::Pass => "pass",
            Self::Unknown => "?",
        }
    }
}

/// The periodic push, every ~2 s — the same payload as `GET /api/v1/stats`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StatsPush {
    pub queries_total: u64,
    pub blocked_total: u64,
    pub blocked_percent: f64,
    /// Hits over **every** query, blocked ones included — not the cache's own
    /// hit ratio ([`super::telemetry::CacheStats::lookup_hit_percent`]).
    pub cache_hit_percent: f64,
    pub top_blocked_domains: Vec<DomainCount>,
    pub top_queried_domains: Vec<DomainCount>,
    pub top_clients: Vec<ClientCount>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DomainCount {
    pub domain: String,
    pub count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientCount {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub count: u64,
}

impl ClientCount {
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => self.ip.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    #[test]
    fn a_query_frame_decodes_in_one_pass() {
        let Decoded::Event(ServerEvent::Query(item)) = decode(fixtures::EVENTS_QUERY) else {
            panic!("expected a query frame");
        };

        assert_eq!(item.verdict, Verdict::Pass);
        assert_eq!(
            item.resolved_name(&LanNames::default()),
            Some("liviu-phone")
        );
        assert_eq!(item.type_label(), "A");
        assert!(item.cached);
    }

    /// The two name sources in priority order, plus the gap that means "show
    /// the address".
    #[test]
    fn a_resolved_name_fills_the_gap_the_appliance_leaves_and_never_overrides_it() {
        let Decoded::Event(ServerEvent::Query(named)) = decode(fixtures::EVENTS_QUERY) else {
            panic!("expected a query frame");
        };
        let mut anonymous = named.clone();
        anonymous.client_name = None;

        let mut names = LanNames::default();
        names.replace(std::collections::HashMap::from([(
            anonymous.client,
            std::sync::Arc::from("from-the-router"),
        )]));

        assert_eq!(
            named.resolved_name(&names),
            Some("liviu-phone"),
            "the appliance's own name wins"
        );
        assert_eq!(anonymous.resolved_name(&names), Some("from-the-router"));
        assert_eq!(anonymous.resolved_name(&LanNames::default()), None);
    }

    #[test]
    fn the_family_suffix_follows_the_address_the_client_arrived_on() {
        let Decoded::Event(ServerEvent::Query(mut item)) = decode(fixtures::EVENTS_QUERY) else {
            panic!("expected a query frame");
        };
        item.client = "192.168.10.10".parse().unwrap();
        assert_eq!(item.family(), "v4");
        item.client = "fd6c:7f32:8e91::1".parse().unwrap();
        assert_eq!(item.family(), "v6");
    }

    #[test]
    fn a_stats_frame_carries_the_top_n_lists() {
        let Decoded::Event(ServerEvent::Stats(stats)) = decode(fixtures::EVENTS_STATS) else {
            panic!("expected a stats frame");
        };

        assert_eq!(stats.queries_total, 96_183);
        assert_eq!(stats.top_blocked_domains[0].count, 20_283);
        // Unnamed clients fall back to their address, v6 included.
        assert_eq!(stats.top_clients[0].label(), "192.168.10.17");
        assert_eq!(
            stats.top_clients[1].label(),
            "fd6c:7f32:8e91:0:4000:9341:51b1:ebbc"
        );
    }

    /// Two kinds the server already publishes are not rendered here, and a
    /// later release may add more. None of that is drift — it must not be
    /// counted as such, or the warning cries wolf on a healthy socket.
    #[test]
    fn an_unrendered_frame_kind_is_skipped_without_counting_as_drift() {
        for frame in [
            r#"{"type":"config_changed","data":{"restart_required":true}}"#,
            r#"{"type":"list_refreshed","data":{"id":"oisd-basic","status":"ok"}}"#,
            r#"{"type":"something_from_phase_4","data":{"whatever":1}}"#,
        ] {
            assert!(matches!(decode(frame), Decoded::Unrendered), "{frame}");
        }
    }

    /// The case the counter exists for: a kind this build *does* render, whose
    /// payload it can no longer parse. Silently skipping it empties the feed
    /// while the socket reads ONLINE.
    #[test]
    fn a_rendered_kind_that_no_longer_parses_is_counted_not_skipped() {
        for frame in [
            // A `query` whose `client` stopped being an address.
            r#"{"type":"query","data":{"kind":"dns","client":"not-an-ip"}}"#,
            // A `stats` missing every field.
            r#"{"type":"stats","data":{}}"#,
            "not json at all",
            r#"{"nothing":"resembling the envelope"}"#,
        ] {
            assert!(matches!(decode(frame), Decoded::Undecodable), "{frame}");
        }
    }

    /// An unknown verdict must not fail the frame and drop a row from the
    /// feed — a rendered kind degrades within itself.
    #[test]
    fn an_unknown_verdict_degrades_rather_than_dropping_the_row() {
        let frame = fixtures::EVENTS_QUERY.replace(r#""verdict": "pass""#, r#""verdict": "xyz""#);
        let Decoded::Event(ServerEvent::Query(item)) = decode(&frame) else {
            panic!("expected a query frame");
        };

        assert_eq!(item.verdict, Verdict::Unknown);
    }
}
