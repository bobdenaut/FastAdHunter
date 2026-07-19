//! Response bodies, field-for-field as API.md documents them. These types
//! exist so the JSON shape is pinned by something a golden test can assert
//! against, independent of how the crates behind the ports happen to model
//! their data.

use std::net::IpAddr;

use fah_model::QueryType;
use serde::{Deserialize, Serialize};

use crate::ports::{ClientEntry, QueryLogPage, QueryRecord, StatsOverview};
use crate::timestamp;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// `ok` | `degraded`.
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub window: &'static str,
    pub queries_total: u64,
    pub blocked_total: u64,
    pub blocked_percent: f64,
    pub cache_hit_percent: f64,
    pub top_blocked_domains: Vec<DomainCountResponse>,
    pub top_queried_domains: Vec<DomainCountResponse>,
    pub top_clients: Vec<ClientCountResponse>,
    pub buckets: Vec<BucketResponse>,
}

#[derive(Debug, Serialize)]
pub struct DomainCountResponse {
    pub domain: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct ClientCountResponse {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct BucketResponse {
    #[serde(serialize_with = "timestamp::serialize")]
    pub start: std::time::SystemTime,
    pub queries: u64,
    pub blocked: u64,
}

impl From<StatsOverview> for StatsResponse {
    fn from(overview: StatsOverview) -> Self {
        Self {
            window: overview.window,
            queries_total: overview.queries_total,
            blocked_total: overview.blocked_total,
            blocked_percent: overview.blocked_percent,
            cache_hit_percent: overview.cache_hit_percent,
            top_blocked_domains: overview
                .top_blocked_domains
                .into_iter()
                .map(|d| DomainCountResponse {
                    domain: display_domain(&d.domain),
                    count: d.count,
                })
                .collect(),
            top_queried_domains: overview
                .top_queried_domains
                .into_iter()
                .map(|d| DomainCountResponse {
                    domain: display_domain(&d.domain),
                    count: d.count,
                })
                .collect(),
            top_clients: overview
                .top_clients
                .into_iter()
                .map(|c| ClientCountResponse {
                    ip: c.ip,
                    name: c.name,
                    count: c.count,
                })
                .collect(),
            buckets: overview
                .buckets
                .into_iter()
                .map(|b| BucketResponse {
                    start: b.start,
                    queries: b.queries,
                    blocked: b.blocked,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QueryPageResponse {
    pub items: Vec<QueryItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QueryItemResponse {
    #[serde(serialize_with = "timestamp::serialize")]
    pub ts: std::time::SystemTime,
    pub client: IpAddr,
    pub client_name: Option<String>,
    pub domain: String,
    pub qtype: String,
    /// `allow` | `block` | `pass`.
    pub verdict: &'static str,
    pub rule: Option<String>,
    pub list: Option<String>,
    pub duration_ms: f64,
    /// Always `null` in Phase 1: the pipeline records *that* an upstream was
    /// used, not which one (`QueryEvent::upstream_used`), and per-query
    /// upstream attribution would cost an allocation on the hot path.
    pub upstream: Option<String>,
    pub cached: bool,
}

impl From<QueryRecord> for QueryItemResponse {
    fn from(record: QueryRecord) -> Self {
        let (rule, list) = record.decisive();
        let (rule, list) = (rule.map(str::to_string), list.map(str::to_string));
        let verdict = record.verdict_str();
        let duration_ms = record.duration().as_secs_f64() * 1000.0;
        Self {
            ts: record.event.query.timestamp,
            client: record.event.query.client_ip,
            client_name: record.client_name,
            domain: display_domain(&record.event.query.domain),
            qtype: qtype_name(&record.event.query.qtype),
            verdict,
            rule,
            list,
            duration_ms,
            upstream: None,
            cached: record.event.cache_hit,
        }
    }
}

impl From<QueryLogPage> for QueryPageResponse {
    fn from(page: QueryLogPage) -> Self {
        Self {
            items: page.items.into_iter().map(Into::into).collect(),
            next_cursor: page.next_cursor,
        }
    }
}

/// Domains as API.md writes them: `"ads.example.com"`, not the wire's
/// fully-qualified `"ads.example.com."`. The DNS pipeline works in wire form
/// (that trailing dot is part of how `fah-dns` decodes and keys a name), so
/// the root label is dropped here, at the presentation boundary, rather than
/// changing what the engine stores.
fn display_domain(domain: &str) -> String {
    domain.strip_suffix('.').unwrap_or(domain).to_string()
}

/// The wire spelling of a record type: `"A"`, `"AAAA"`, or whatever the
/// resolver called it for everything else.
pub fn qtype_name(qtype: &QueryType) -> String {
    match qtype {
        QueryType::A => "A".to_string(),
        QueryType::Aaaa => "AAAA".to_string(),
        QueryType::Other(name) => name.clone(),
    }
}

/// Parses the wire spelling back into a [`QueryType`] for
/// `POST /api/v1/rules/test`.
pub fn parse_qtype(name: &str) -> QueryType {
    match name.to_ascii_uppercase().as_str() {
        "A" => QueryType::A,
        "AAAA" => QueryType::Aaaa,
        other => QueryType::Other(other.to_string()),
    }
}

#[derive(Debug, Serialize)]
pub struct ClientsResponse {
    pub items: Vec<ClientResponse>,
}

#[derive(Debug, Serialize)]
pub struct ClientResponse {
    pub ip: IpAddr,
    pub name: Option<String>,
    #[serde(serialize_with = "timestamp::serialize")]
    pub first_seen: std::time::SystemTime,
    #[serde(serialize_with = "timestamp::serialize")]
    pub last_seen: std::time::SystemTime,
    pub queries_24h: u64,
    pub blocked_24h: u64,
}

impl From<ClientEntry> for ClientResponse {
    fn from(entry: ClientEntry) -> Self {
        Self {
            ip: entry.ip,
            name: entry.name,
            first_seen: entry.first_seen,
            last_seen: entry.last_seen,
            queries_24h: entry.queries_24h,
            blocked_24h: entry.blocked_24h,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ClientNameRequest {
    /// `null` clears the name (API.md: "DELETE of the name: send
    /// `{ "name": null }`").
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListsResponse {
    pub items: Vec<ListResponse>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub id: String,
    pub url: String,
    /// Always `"auto"`: formats are auto-detected per refresh and the
    /// detection result is not retained (RULE_ENGINE.md §Supported formats).
    pub format: &'static str,
    pub enabled: bool,
    pub refresh_hours: u32,
    #[serde(serialize_with = "timestamp::serialize_option")]
    pub last_refresh: Option<std::time::SystemTime>,
    /// `ok` | `failed` | `never`.
    pub last_status: &'static str,
    pub rules_total: usize,
    pub rules_active_dns: usize,
    pub rules_inactive: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateListRequest {
    /// A remote list's URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// A mounted file's path (API.md: `{ "path": "/data/lists/local.txt" }`).
    pub path: Option<String>,
    /// Optional explicit id; defaults to one derived from the source.
    pub id: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub refresh_hours: Option<u32>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct PatchListRequest {
    pub enabled: Option<bool>,
    /// Absent leaves the interval alone; `null` clears the per-list override
    /// back to `[rules] refresh_hours_default`.
    #[serde(default, deserialize_with = "double_option")]
    pub refresh_hours: Option<Option<u32>>,
}

/// Distinguishes "field absent" from "field present and null" — serde folds
/// both into `None` otherwise, which would make a `PATCH` unable to clear a
/// per-list refresh override.
fn double_option<'de, D>(deserializer: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserRulesBody {
    pub rules: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RuleTestRequest {
    pub domain: String,
    #[serde(default)]
    pub qtype: Option<String>,
    /// Accepted for forward compatibility and ignored: client-scoped rules
    /// (`$client`) are parsed but inactive in Phase 1 (ADR-0003). Named
    /// explicitly rather than left to serde's ignore-unknown so the field is
    /// part of the documented request shape the day it starts mattering.
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "reserved: $client rules activate in a later phase"
    )]
    pub client: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuleTestResponse {
    /// `allow` | `block` | `pass`.
    pub verdict: &'static str,
    pub rule: Option<String>,
    pub list: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfigUpdateResponse {
    pub applied: bool,
    pub restart_required: bool,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    /// Returned exactly once, at rotation (SECURITY.md §API access).
    pub api_key: String,
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::{Duration, SystemTime};

    use fah_model::{DecisiveRule, Query, QueryEvent, Verdict};

    use super::*;

    #[test]
    fn query_item_matches_the_documented_field_set() {
        let record = QueryRecord {
            event: QueryEvent::new(
                Query::new(
                    "ads.example.com",
                    QueryType::A,
                    IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15)),
                    SystemTime::UNIX_EPOCH,
                ),
                Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
                Duration::from_micros(300),
                false,
                false,
                false,
            ),
            client_name: Some("liviu-phone".to_string()),
        };

        let json = serde_json::to_value(QueryItemResponse::from(record)).unwrap();
        let object = json.as_object().unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "cached",
                "client",
                "client_name",
                "domain",
                "duration_ms",
                "list",
                "qtype",
                "rule",
                "ts",
                "upstream",
                "verdict",
            ]
        );
        assert_eq!(object["ts"], "1970-01-01T00:00:00Z");
        assert_eq!(object["verdict"], "block");
        assert_eq!(object["rule"], "||ads.example.com^");
        assert_eq!(object["list"], "oisd-basic");
        assert_eq!(object["duration_ms"], 0.3);
        assert!(object["upstream"].is_null());
        assert_eq!(object["cached"], false);
    }

    #[test]
    fn the_wire_form_root_label_is_dropped_for_display() {
        // The pipeline stores names in wire form; API.md documents them
        // without the trailing root label.
        assert_eq!(display_domain("ads.example.com."), "ads.example.com");
        assert_eq!(display_domain("ads.example.com"), "ads.example.com");
        // The root zone itself must not become an empty string.
        assert_eq!(display_domain("."), "");
    }

    #[test]
    fn qtype_round_trips_through_the_wire_spelling() {
        for qtype in [
            QueryType::A,
            QueryType::Aaaa,
            QueryType::Other("TXT".to_string()),
        ] {
            assert_eq!(parse_qtype(&qtype_name(&qtype)), qtype);
        }
        assert_eq!(parse_qtype("aaaa"), QueryType::Aaaa);
    }

    #[test]
    fn patch_distinguishes_absent_from_explicit_null() {
        let absent: PatchListRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.refresh_hours, None);

        let cleared: PatchListRequest = serde_json::from_str(r#"{"refresh_hours": null}"#).unwrap();
        assert_eq!(cleared.refresh_hours, Some(None));

        let set: PatchListRequest = serde_json::from_str(r#"{"refresh_hours": 6}"#).unwrap();
        assert_eq!(set.refresh_hours, Some(Some(6)));
    }
}
