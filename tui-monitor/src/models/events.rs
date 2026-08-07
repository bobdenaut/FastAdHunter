//! `WS /api/v1/events` — the live tail (API.md §Events).
//!
//! The envelope is an adjacently-tagged enum, so one `from_str` decides both
//! the frame kind and its payload.

use std::net::IpAddr;

use serde::Deserialize;

/// One `{ "type": …, "data": … }` frame this monitor renders.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Boxed: far larger than the other variant, and moved once per query on
    /// a busy resolver.
    Query(Box<QueryItem>),
    Stats(Box<StatsPush>),
}

/// Decodes one frame, or `None` for a kind this build does not render —
/// `config_changed`, `list_refreshed`, anything a later release adds — and for
/// anything malformed. Neither may tear down the socket.
///
/// A `#[serde(other)]` fallback variant cannot do this job: on an adjacently
/// tagged enum serde still deserializes `data` into the unit variant, which
/// fails for every frame that carries a payload.
pub fn decode(text: &str) -> Option<ServerEvent> {
    serde_json::from_str(text).ok()
}

/// One entry of the live feed.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryItem {
    /// `dns` | `http`.
    pub kind: String,
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

impl QueryItem {
    /// The assigned name if the client has one, else its address.
    pub fn client_label(&self) -> String {
        match &self.client_name {
            Some(name) => name.clone(),
            None => self.client.to_string(),
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
    pub fn as_str(&self) -> &'static str {
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
        let Some(ServerEvent::Query(item)) = decode(fixtures::EVENTS_QUERY) else {
            panic!("expected a query frame");
        };

        assert_eq!(item.verdict, Verdict::Pass);
        assert_eq!(item.client_label(), "liviu-phone");
        assert_eq!(item.type_label(), "A");
        assert!(item.cached);
    }

    #[test]
    fn a_stats_frame_carries_the_top_n_lists() {
        let Some(ServerEvent::Stats(stats)) = decode(fixtures::EVENTS_STATS) else {
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

    /// Two kinds the server already publishes are not rendered here; neither
    /// may end the socket.
    #[test]
    fn an_unrendered_frame_kind_is_skipped_rather_than_failing() {
        for frame in [
            r#"{"type":"config_changed","data":{"restart_required":true}}"#,
            r#"{"type":"list_refreshed","data":{"id":"oisd-basic","status":"ok"}}"#,
            r#"{"type":"something_from_phase_4","data":{"whatever":1}}"#,
            "not json at all",
        ] {
            assert!(decode(frame).is_none(), "{frame}");
        }
    }

    /// An unknown verdict must not fail the frame and drop a row from the
    /// feed — a rendered kind degrades within itself.
    #[test]
    fn an_unknown_verdict_degrades_rather_than_dropping_the_row() {
        let frame = fixtures::EVENTS_QUERY.replace(r#""verdict": "pass""#, r#""verdict": "xyz""#);
        let Some(ServerEvent::Query(item)) = decode(&frame) else {
            panic!("expected a query frame");
        };

        assert_eq!(item.verdict, Verdict::Unknown);
    }
}
