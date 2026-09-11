//! Response bodies, field-for-field as API.md documents them. These types
//! exist so the JSON shape is pinned by something a golden test can assert
//! against, independent of how the crates behind the ports happen to model
//! their data.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use fah_model::{
    AnswerCounters, CacheStatsSample, HistorySeries, LatencySummary, PerfSeries, QueryType,
    TopItems, UpstreamSample,
};
use serde::{Deserialize, Serialize};

use crate::ports::{CacheClean, CacheStats, QueryRecord, StatsOverview};
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
    /// Per-policy activity over the same 24h window (p2-06). Clients under no
    /// assignment are counted under `default`.
    pub policies: Vec<PolicyCountResponse>,
}

#[derive(Debug, Serialize)]
pub struct PolicyCountResponse {
    pub policy: String,
    pub queries: u64,
    pub blocked: u64,
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
            policies: overview
                .policies
                .into_iter()
                .map(|p| PolicyCountResponse {
                    policy: p.policy,
                    queries: p.queries,
                    blocked: p.blocked,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QueryItemResponse {
    /// `dns` | `http` (p2-04). Present on every item so a client never has to
    /// infer which pipeline answered from which fields happen to be null.
    pub kind: &'static str,
    #[serde(serialize_with = "timestamp::serialize")]
    pub ts: std::time::SystemTime,
    pub client: IpAddr,
    pub client_name: Option<String>,
    /// The name the entry is about: the question for DNS, the request host for
    /// HTTP. One field rather than two, because "what was this client
    /// reaching for" is one question.
    pub domain: String,
    /// DNS only — `null` for an HTTP request, which asks no record type.
    pub qtype: Option<String>,
    /// `allow` | `block` | `pass`.
    pub verdict: &'static str,
    pub rule: Option<String>,
    pub list: Option<String>,
    pub duration_ms: f64,
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<u8>,
    /// DNS only. An HTTP request has no cache to hit, so this is `false` for
    /// one rather than pretending it missed.
    pub cached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
    // ── HTTP only; `null` on a DNS item ──
    pub method: Option<String>,
    pub path: Option<String>,
    /// `script` | `image` | … | `unknown` (`fah_model::ResourceType`).
    pub resource_type: Option<String>,
    /// Status returned to the client, a synthesized block's included.
    pub status: Option<u16>,
    /// Response body bytes relayed. `0` on a block — the number that shows
    /// what filtering saved.
    pub bytes: Option<u64>,
}

impl From<QueryRecord> for QueryItemResponse {
    fn from(record: QueryRecord) -> Self {
        let (rule, list) = record.decisive();
        let (rule, list) = (rule.map(str::to_string), list.map(str::to_string));
        let verdict = record.verdict_str();
        let duration_ms = record.duration().as_secs_f64() * 1000.0;
        let kind = record.event.kind().as_str();
        let ts = record.event.timestamp();
        let client = record.event.client_ip();

        let (domain, qtype, cached, endpoint, transport) = match record.as_dns() {
            Some(event) => (
                display_domain(&event.query.domain),
                Some(qtype_name(&event.query.qtype)),
                event.cache_hit,
                event.endpoint,
                Some(event.transport.as_str()),
            ),
            None => (String::new(), None, false, None, None),
        };
        let (domain, method, path, resource_type, status, bytes) = match record.as_http() {
            Some(event) => (
                event.request.host.clone(),
                Some(event.request.method.clone()),
                Some(event.request.path.clone()),
                Some(resource_type_name(event.request.resource_type)),
                Some(event.status),
                Some(event.bytes),
            ),
            None => (domain, None, None, None, None, None),
        };

        Self {
            kind,
            ts,
            client,
            client_name: record.client_name,
            domain,
            qtype,
            verdict,
            rule,
            list,
            duration_ms,
            upstream: None,
            endpoint,
            cached,
            transport,
            method,
            path,
            resource_type,
            status,
            bytes,
        }
    }
}

/// The wire spelling of a resource type, matching the `$option` vocabulary
/// RULE_ENGINE.md documents rather than the Rust variant name.
fn resource_type_name(kind: fah_model::ResourceType) -> String {
    use fah_model::ResourceType as R;
    match kind {
        R::Document => "document",
        R::Subdocument => "subdocument",
        R::Script => "script",
        R::Stylesheet => "stylesheet",
        R::Image => "image",
        R::Font => "font",
        R::Media => "media",
        R::XmlHttpRequest => "xmlhttprequest",
        R::WebSocket => "websocket",
        R::Ping => "ping",
        R::Object => "object",
        R::Other => "other",
        R::Unknown => "unknown",
    }
    .to_string()
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

// ─── History (persisted series) ────────────────────────────────────────

/// Common envelope of the three history endpoints: the range actually served,
/// echoed back so a chart can label its axis without re-deriving the defaults
/// this crate applied.
#[derive(Debug, Serialize)]
pub struct HistorySummaryResponse {
    /// `hour` | `day`.
    pub resolution: &'static str,
    #[serde(serialize_with = "timestamp::serialize")]
    pub from: SystemTime,
    #[serde(serialize_with = "timestamp::serialize")]
    pub to: SystemTime,
    /// `1` when every stored point in the range is present, `n` when only every
    /// `n`-th survived the `max_points` budget. Reported rather than hidden: a
    /// silently sparse chart is a lying chart.
    pub stride: u64,
    pub items: Vec<HistoryPointResponse>,
}

#[derive(Debug, Serialize)]
pub struct HistoryPointResponse {
    /// Start of the bucket (the hour, or the UTC day).
    #[serde(serialize_with = "timestamp::serialize")]
    pub ts: SystemTime,
    pub queries: u64,
    pub blocked: u64,
    /// `blocked / queries` in percent, two decimals — derived here, like the
    /// cache view's `load_percent`, so the stored rows stay pure counters.
    pub blocked_percent: f64,
    pub cache_hits: u64,
    pub per_type: BTreeMap<String, u64>,
}

impl HistorySummaryResponse {
    pub fn new(
        resolution: &'static str,
        from: SystemTime,
        to: SystemTime,
        series: HistorySeries,
    ) -> Self {
        Self {
            resolution,
            from,
            to,
            stride: series.stride,
            items: series
                .points
                .into_iter()
                .map(|point| HistoryPointResponse {
                    ts: instant(point.ts),
                    blocked_percent: percent(point.blocked, point.queries),
                    queries: point.queries,
                    blocked: point.blocked,
                    cache_hits: point.cache_hits,
                    per_type: point.per_type,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HistoryPerfResponse {
    #[serde(serialize_with = "timestamp::serialize")]
    pub from: SystemTime,
    #[serde(serialize_with = "timestamp::serialize")]
    pub to: SystemTime,
    /// As in [`HistorySummaryResponse::stride`] — the perf series is the one
    /// that routinely needs it (a 60 s cadence is 1440 samples per day).
    pub stride: u64,
    pub items: Vec<PerfSampleResponse>,
}

/// One persisted [`fah_model::PerfSample`], with `ts` in the RFC 3339 spelling
/// the rest of the API uses and every other key droppable via `?fields=`.
/// A key the caller did not ask for is **absent**, not null — `fields` exists to
/// shrink the payload, and a null would still cost its name.
#[derive(Debug, Serialize)]
pub struct PerfSampleResponse {
    #[serde(serialize_with = "timestamp::serialize")]
    pub ts: SystemTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queries_delta: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_delta: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_delta: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheStatsSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstreams: Option<Vec<UpstreamSample>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryComponentsResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minor_page_faults: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rss_anon_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rss_file_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers_delta: Option<AnswerCounters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocator_committed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_fetch: Option<fah_model::ListFetchCounters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent_connections: Option<fah_model::ConcurrentConnections>,
}

/// Which [`PerfSampleResponse`] keys `?fields=` kept. Names match the response
/// keys one-for-one, so there is nothing to look up. It trims the *response*;
/// the read costs the same either way (each sample is one JSONL line, parsed
/// whole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfFields {
    pub rss_bytes: bool,
    pub peak_rss: bool,
    pub qps: bool,
    pub queries_delta: bool,
    pub blocked_delta: bool,
    pub allowed_delta: bool,
    pub cache: bool,
    pub latency: bool,
    pub upstreams: bool,
    pub memory: bool,
    pub minor_page_faults: bool,
    pub rss_anon_bytes: bool,
    pub rss_file_bytes: bool,
    pub answers_delta: bool,
    pub allocator_committed_bytes: bool,
    pub list_fetch: bool,
    pub concurrent_connections: bool,
}

impl PerfFields {
    /// The default: `?fields=` omitted serves the whole sample.
    pub const ALL: Self = Self {
        rss_bytes: true,
        peak_rss: true,
        qps: true,
        queries_delta: true,
        blocked_delta: true,
        allowed_delta: true,
        cache: true,
        latency: true,
        upstreams: true,
        memory: true,
        minor_page_faults: true,
        rss_anon_bytes: true,
        rss_file_bytes: true,
        answers_delta: true,
        allocator_committed_bytes: true,
        list_fetch: true,
        concurrent_connections: true,
    };

    pub const NONE: Self = Self {
        rss_bytes: false,
        peak_rss: false,
        qps: false,
        queries_delta: false,
        blocked_delta: false,
        allowed_delta: false,
        cache: false,
        latency: false,
        upstreams: false,
        memory: false,
        minor_page_faults: false,
        rss_anon_bytes: false,
        rss_file_bytes: false,
        answers_delta: false,
        allocator_committed_bytes: false,
        list_fetch: false,
        concurrent_connections: false,
    };

    /// The accepted `?fields=` names, in response order — also what a rejection
    /// message lists back.
    pub const NAMES: [&'static str; 17] = [
        "rss_bytes",
        "peak_rss",
        "qps",
        "queries_delta",
        "blocked_delta",
        "allowed_delta",
        "cache",
        "latency",
        "upstreams",
        "memory",
        "minor_page_faults",
        "rss_anon_bytes",
        "rss_file_bytes",
        "answers_delta",
        "allocator_committed_bytes",
        "list_fetch",
        "concurrent_connections",
    ];

    /// Turns one `?fields=` name on; `false` for a name that is not a key.
    pub fn enable(&mut self, name: &str) -> bool {
        match name {
            "rss_bytes" => self.rss_bytes = true,
            "peak_rss" => self.peak_rss = true,
            "qps" => self.qps = true,
            "queries_delta" => self.queries_delta = true,
            "blocked_delta" => self.blocked_delta = true,
            "allowed_delta" => self.allowed_delta = true,
            "cache" => self.cache = true,
            "latency" => self.latency = true,
            "upstreams" => self.upstreams = true,
            "memory" => self.memory = true,
            "minor_page_faults" => self.minor_page_faults = true,
            "rss_anon_bytes" => self.rss_anon_bytes = true,
            "rss_file_bytes" => self.rss_file_bytes = true,
            "answers_delta" => self.answers_delta = true,
            "allocator_committed_bytes" => self.allocator_committed_bytes = true,
            "list_fetch" => self.list_fetch = true,
            "concurrent_connections" => self.concurrent_connections = true,
            _ => return false,
        }
        true
    }
}

impl HistoryPerfResponse {
    pub fn new(from: SystemTime, to: SystemTime, series: PerfSeries, fields: PerfFields) -> Self {
        Self {
            from,
            to,
            stride: series.stride,
            items: series
                .samples
                .into_iter()
                .map(|sample| PerfSampleResponse {
                    ts: instant(sample.ts),
                    rss_bytes: fields.rss_bytes.then_some(sample.rss_bytes),
                    peak_rss: fields.peak_rss.then_some(sample.peak_rss),
                    qps: fields.qps.then_some(sample.qps),
                    queries_delta: fields.queries_delta.then_some(sample.queries_delta),
                    blocked_delta: fields.blocked_delta.then_some(sample.blocked_delta),
                    allowed_delta: fields.allowed_delta.then_some(sample.allowed_delta),
                    cache: fields.cache.then_some(sample.cache),
                    latency: fields.latency.then_some(sample.latency),
                    // The row's own RSS, so its residual is single-instant and
                    // goes through the same `residual()` the live path uses.
                    memory: fields.memory.then(|| {
                        MemoryComponentsResponse::of(&fah_model::MemoryBreakdown {
                            components: sample.memory,
                            rss: Some(sample.rss_bytes),
                            rss_anon: None,
                            rss_file: None,
                            process: None,
                            allocator: None,
                        })
                    }),
                    minor_page_faults: fields.minor_page_faults.then_some(sample.minor_page_faults),
                    rss_anon_bytes: fields.rss_anon_bytes.then_some(sample.rss_anon_bytes),
                    rss_file_bytes: fields.rss_file_bytes.then_some(sample.rss_file_bytes),
                    answers_delta: fields.answers_delta.then_some(sample.answers_delta),
                    allocator_committed_bytes: fields
                        .allocator_committed_bytes
                        .then_some(sample.allocator_committed_bytes),
                    list_fetch: fields.list_fetch.then_some(sample.list_fetch),
                    concurrent_connections: fields
                        .concurrent_connections
                        .then_some(sample.concurrent_connections),
                    upstreams: fields.upstreams.then_some(sample.upstreams),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HistoryTopResponse {
    /// `blocked` | `queried` | `clients`.
    pub kind: &'static str,
    #[serde(serialize_with = "timestamp::serialize")]
    pub from: SystemTime,
    #[serde(serialize_with = "timestamp::serialize")]
    pub to: SystemTime,
    pub items: Vec<HistoryTopItem>,
}

/// Two shapes, because a client is an IP plus an optional name while a domain
/// is its name alone — untagged, so the JSON is the plain object a chart wants
/// rather than a wrapper it has to unpick.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum HistoryTopItem {
    Domain {
        domain: String,
        count: u64,
    },
    Client {
        ip: IpAddr,
        name: Option<String>,
        count: u64,
    },
}

impl HistoryTopResponse {
    pub fn new(kind: &'static str, from: SystemTime, to: SystemTime, items: TopItems) -> Self {
        let items = match items {
            TopItems::Domains(domains) => domains
                .into_iter()
                .map(|hit| HistoryTopItem::Domain {
                    domain: display_domain(&hit.domain),
                    count: hit.count,
                })
                .collect(),
            TopItems::Clients(clients) => clients
                .into_iter()
                .map(|hit| HistoryTopItem::Client {
                    ip: hit.ip,
                    name: hit.name,
                    count: hit.count,
                })
                .collect(),
        };
        Self {
            kind,
            from,
            to,
            items,
        }
    }
}

/// Epoch seconds → `SystemTime`, for the persisted rows' `ts` fields.
fn instant(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// `part / whole` in percent, two decimals; `0.0` rather than a division error
/// for an empty bucket.
fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        round2(part as f64 * 100.0 / whole as f64)
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
    pub intercepted: InterceptedResponse,
    pub policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_source: Option<&'static str>,
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
    /// Rules in the compiled ruleset — **distinct** rules across every list,
    /// so it is smaller than the sum of the per-list `rules_active_dns` by
    /// exactly `duplicates_removed`.
    pub compiled_rules: usize,
    /// How many rules the merge dropped as duplicates of one already present
    /// (RULE_ENGINE.md §Deduplication). Ruleset-wide, not per list: a
    /// duplicate belongs to a pair of lists, not to one of them.
    pub duplicates_removed: usize,
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
    pub last_status: &'static str,
    pub rules_total: usize,
    pub rules_active_dns: usize,
    /// Rules active in the URL tier — request-level rules the HTTP pipeline
    /// answers from (RULE_ENGINE.md §HTTP matching, p2-03). Split out of
    /// `rules_inactive`, where they used to be counted: they filter, so
    /// reporting them as inactive understated what a list like EasyList does
    /// by ~22k rules.
    pub rules_active_url: usize,
    /// Rules no tier answers yet: cosmetic (Phase 4), `$client` (p2-05), and
    /// patterns no supported syntax expresses.
    pub rules_inactive: usize,
    pub parse_errors: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// `POST /api/v1/lists/refresh` — one pass over every list. Best-effort:
/// `results` lists each list in configuration order, so a dead source shows as
/// `status: "failed"` with its `error` while the others still refresh, and the
/// ruleset recompiles once for the whole batch.
#[derive(Debug, Serialize)]
pub struct RefreshAllResponse {
    pub refreshed: usize,
    pub failed: usize,
    pub results: Vec<ListRefreshResult>,
}

#[derive(Debug, Serialize)]
pub struct ListRefreshResult {
    pub id: String,
    pub status: &'static str,
    /// Present on success: the list's active DNS rules after this refresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules_active_dns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    /// Whose view to test from: an address or a client name. Live since p2-06
    /// — it selects the policy and satisfies `$client` rules. Omitted means the
    /// default policy, which is what an unassigned client gets.
    #[serde(default)]
    pub client: Option<String>,
    /// Test against a named policy directly, ignoring assignments. Useful for
    /// "what would kids see?" without owning a device on that policy.
    #[serde(default)]
    pub policy: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuleTestResponse {
    /// `allow` | `block` | `pass`.
    pub verdict: &'static str,
    pub rule: Option<String>,
    pub list: Option<String>,
    /// Which policy decided, `"default"` when none was assigned (p2-06).
    pub policy: String,
}

// ─── Policies (p2-06) ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PoliciesResponse {
    /// The POSIX TZ every schedule below is read in.
    pub timezone: String,
    pub items: Vec<PolicyResponse>,
    /// Assignments in force at this instant — what a schedule edit changes
    /// without a restart, visible without waiting for a query.
    pub active_assignments: usize,
}

#[derive(Debug, Serialize)]
pub struct PolicyResponse {
    pub id: String,
    pub name: String,
    /// `null` means every enabled list, which is what an omitted `lists` means
    /// in the TOML too.
    pub lists: Option<Vec<String>>,
    pub blocking_mode: Option<String>,
    pub assignments: Vec<AssignmentResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssignmentResponse {
    pub client: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub lists: Option<Vec<String>>,
    #[serde(default)]
    pub blocking_mode: Option<String>,
    #[serde(default)]
    pub assignments: Vec<AssignmentResponse>,
}

/// Absent fields are left alone; `lists: null` clears the subset back to "every
/// enabled list", which is why it needs [`double_option`].
#[derive(Debug, Default, Deserialize)]
pub struct PatchPolicyRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option_lists")]
    pub lists: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "double_option_string")]
    pub blocking_mode: Option<Option<String>>,
    #[serde(default)]
    pub assignments: Option<Vec<AssignmentResponse>>,
}

fn double_option_lists<'de, D>(deserializer: D) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

fn double_option_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// `PUT /api/v1/clients/{ip}/policy` — assigns one address to a policy,
/// optionally only inside a schedule.
#[derive(Debug, Deserialize)]
pub struct ClientPolicyRequest {
    pub policy: String,
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

/// What a client is judged under right now, and why.
#[derive(Debug, Serialize)]
pub struct ClientPolicyResponse {
    pub ip: IpAddr,
    /// The policy in force at this instant — a scheduled assignment that is
    /// not currently open reports the policy that actually applies instead.
    pub policy: String,
    /// The assignment configured for this exact address, if any. Absent when
    /// the client is covered by a subnet or name assignment instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment: Option<AssignmentResponse>,
}

#[derive(Debug, Serialize)]
pub struct CacheStatsResponse {
    pub entries: u64,
    pub capacity: u64,
    pub fresh: u64,
    pub stale: u64,
    pub expired: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// What the resident entries hold, and the ceiling the cache evicts
    /// against — the second of the two bounds (API.md §Cache). Coarse by
    /// construction: the per-entry estimate, not an allocator audit.
    pub bytes: u64,
    pub max_bytes: u64,
    /// `entries / capacity`, in percent, rounded to two decimals.
    pub load_percent: f64,
    /// `bytes / max_bytes`, in percent, rounded to two decimals — whichever
    /// of the two load figures is higher is the one about to evict.
    pub byte_load_percent: f64,
}

impl From<CacheStats> for CacheStatsResponse {
    fn from(stats: CacheStats) -> Self {
        let percent_of = |value: u64, bound: u64| {
            if bound == 0 {
                0.0
            } else {
                round2(value as f64 * 100.0 / bound as f64)
            }
        };
        Self {
            entries: stats.entries,
            capacity: stats.capacity,
            fresh: stats.fresh,
            stale: stats.stale,
            expired: stats.expired,
            hits: stats.hits,
            misses: stats.misses,
            evictions: stats.evictions,
            bytes: stats.bytes,
            max_bytes: stats.max_bytes,
            load_percent: percent_of(stats.entries, stats.capacity),
            byte_load_percent: percent_of(stats.bytes, stats.max_bytes),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CacheCleanResponse {
    pub removed_expired: u64,
    pub removed_stale: u64,
    pub entries_before: u64,
    pub entries_after: u64,
    /// Coarse heap estimate (API.md §Cache) — good for a dashboard bar, not
    /// an allocator audit.
    pub freed_bytes: u64,
    pub duration_ms: f64,
}

impl From<CacheClean> for CacheCleanResponse {
    fn from(clean: CacheClean) -> Self {
        Self {
            removed_expired: clean.removed_expired,
            removed_stale: clean.removed_stale,
            entries_before: clean.entries_before,
            entries_after: clean.entries_after,
            freed_bytes: clean.freed_bytes,
            duration_ms: clean.duration.as_secs_f64() * 1000.0,
        }
    }
}

/// `POST /api/v1/cache/clean`'s query string.
#[derive(Debug, Deserialize)]
pub struct CacheCleanParams {
    /// `?stale=true` also purges the RFC 8767 stale window — an explicit
    /// choice, because stale entries are the outage insurance.
    #[serde(default)]
    pub stale: bool,
}

/// The component figures and their two derived totals. One struct because
/// `/debug/memory` and every `/history/perf` row serve exactly these numbers:
/// flattened into [`MemoryResponse`], nested under `memory` in a perf row.
#[derive(Debug, Serialize)]
pub struct MemoryComponentsResponse {
    pub ruleset_bytes: u64,
    pub cache_estimated_bytes: u64,
    /// 24 h aggregates plus the bounded top-N domain counters.
    pub stats_aggregates_bytes: u64,
    /// Per-client records, capped at 4 096 with LRU eviction.
    pub stats_clients_bytes: u64,
    /// Everything above, summed.
    pub accounted_bytes: u64,
    /// `process_rss − accounted_bytes`: binary pages, thread stacks, the tokio
    /// runtime, and memory the allocator holds but has not returned to the OS.
    /// Growth here while the components stay flat is the leak signal (p2-07).
    /// `null` when RSS is unavailable, since it cannot then be computed.
    ///
    /// The allocator counters cannot refine this — see
    /// `MemoryResponse::allocator_committed_bytes`. Judge it against its own
    /// history.
    pub residual_bytes: Option<u64>,
}

impl MemoryComponentsResponse {
    /// The only place these are derived, so a live read and a persisted row
    /// cannot disagree about `accounted` or `residual`.
    pub fn of(memory: &fah_model::MemoryBreakdown) -> Self {
        Self {
            ruleset_bytes: memory.components.ruleset,
            cache_estimated_bytes: memory.components.cache,
            stats_aggregates_bytes: memory.components.stats.aggregates,
            stats_clients_bytes: memory.components.stats.clients,
            accounted_bytes: memory.accounted(),
            residual_bytes: memory.residual(),
        }
    }
}

/// The memory figures **both** `/api/v1/telemetry` and `/api/v1/debug/memory`
/// serve: the named components, and the kernel's own readings.
///
/// The split from [`DebugMemoryResponse`] is the producer boundary, expressed
/// in types rather than in prose. Everything here comes from FastAdHunter's own
/// accounting or from the kernel (`/proc/self/status`, `getrusage` via
/// `fah_model::ProcessStats`), so it survives an allocator change and belongs
/// on the stable surface. Anything specific to whichever allocator is linked in
/// goes one struct down, where no compatibility is promised — and reads a
/// different `Option`, so dropping it cannot null anything here.
///
/// Because `/debug/memory` embeds this one, the two can never disagree about a
/// field they share — the only way to add a figure to both is to add it here.
#[derive(Debug, Serialize)]
pub struct MemoryResponse {
    #[serde(flatten)]
    pub components: MemoryComponentsResponse,
    pub cache_entries: u64,
    /// `null` off Linux — the deployment target is a Linux container; a dev
    /// box on another OS simply has no `/proc/self/status` to read.
    pub process_rss: Option<u64>,
    /// Peak RSS since start, from `getrusage` — a real kernel high-water mark,
    /// unlike the commit counters. **Process-lifetime monotonic and never
    /// decreasing**, so it answers "did this process ever exceed the memory
    /// budget" rather than describing now.
    ///
    /// Its value over `process_rss` is that a spike between two polls cannot be
    /// missed — the 150.7 MiB startup-compile peak on 0.2.7 fell between two
    /// 2-minute samples and was visible only here.
    pub process_peak_rss: Option<u64>,
    /// Major (disk-backed) page faults since start. Process-lifetime
    /// cumulative, and structurally near-zero: nothing FAH touches is
    /// demand-paged from disk, so a non-zero value means real host memory
    /// pressure. `null` off Unix.
    pub major_page_faults: Option<u64>,
    /// Minor (no disk I/O) page faults since start. Process-lifetime
    /// cumulative, and the counter that actually moves.
    ///
    /// **The purge-thrash detector.** Handing pages back with `MADV_DONTNEED`
    /// and then reallocating costs one minor fault per page faulted in again,
    /// which is precisely the trade-off `MIMALLOC_PURGE_DELAY` tunes. Without
    /// it, an over-aggressive purge setting is invisible — RSS looks healthy
    /// while the process pays a syscall and a fault for memory it is about to
    /// reuse. Read as a rate against query volume, not as an absolute. `null`
    /// off Unix, where `getrusage` does not exist.
    pub minor_page_faults: Option<u64>,
    /// `RssAnon:` — the heap-and-stacks part of `process_rss`, from the same
    /// read. With `process_rss_file` it says what the residual is made of:
    /// memory the allocator holds, or page cache charged for reading `/data`.
    /// `null` on a kernel that reports `VmRSS:` without the split.
    pub process_rss_anon: Option<u64>,
    /// `RssFile:` — the file-backed part of `process_rss`. What the two do not
    /// cover is shared memory, derivable rather than served as a fourth key.
    pub process_rss_file: Option<u64>,
    pub cpu_user_ms: Option<u64>,
    pub cpu_system_ms: Option<u64>,
}

impl MemoryResponse {
    /// Built from the breakdown, never from another endpoint's response — both
    /// surfaces derive independently from the same snapshot, which is what
    /// keeps them from becoming coupled.
    pub fn of(memory: &fah_model::MemoryBreakdown, cache_entries: u64) -> Self {
        Self {
            components: MemoryComponentsResponse::of(memory),
            cache_entries,
            process_rss: memory.rss,
            // `process`, not `allocator`: these three are kernel readings and
            // must not disappear when the allocator's counters do.
            process_peak_rss: memory.process.map(|p| p.peak_rss),
            major_page_faults: memory.process.map(|p| p.major_page_faults),
            minor_page_faults: memory.process.map(|p| p.minor_page_faults),
            process_rss_anon: memory.rss_anon,
            process_rss_file: memory.rss_file,
            cpu_user_ms: memory.process.map(|p| p.cpu_user_ms),
            cpu_system_ms: memory.process.map(|p| p.cpu_system_ms),
        }
    }
}

/// `GET /api/v1/debug/memory`: everything [`MemoryResponse`] carries, plus the
/// figures that describe *this* allocator and would read near zero under
/// another one.
///
/// They live here rather than on `/api/v1/telemetry` so that surface can stay a
/// stable contract while the allocator remains replaceable — a dashboard built
/// on `/telemetry` survives swapping mimalloc out, and a diagnostic built on
/// these knowingly does not.
#[derive(Debug, Serialize)]
pub struct DebugMemoryResponse {
    #[serde(flatten)]
    pub memory: MemoryResponse,
    /// Bytes the allocator has committed, by its own accounting (see
    /// `crates/fastadhunter/src/allocator.rs`) — not a kernel reading. `null`
    /// where unavailable.
    ///
    /// **Read this as a high-water mark, and expect it to exceed
    /// `process_rss`, often several times over.** mimalloc v3 does not
    /// decrement the counter when a purge returns pages to the OS, so it only
    /// ever rises; 318 MB here against 70 MB `process_rss` was the measured
    /// state on the RB5009. The gap is memory committed, touched, and since
    /// reclaimed by the kernel — not memory being held.
    ///
    /// **Do not subtract `accounted_bytes` from this and call it retention.**
    /// 0.2.7 served exactly that as `allocator_retained_bytes`; it reported
    /// 260 MiB of "retention" in a process with 70 MiB resident, which is
    /// impossible for anything resident, and the field has been removed.
    /// `process_rss` is the authority on footprint.
    pub allocator_committed_bytes: Option<u64>,
    /// High-water mark of `allocator_committed_bytes`.
    ///
    /// Currently equal to it at every reading, for the reason above. That
    /// equality is the diagnostic: should the two ever diverge, the allocator
    /// has started accounting purges and `allocator_committed_bytes` has become
    /// a live figure worth reading as one.
    pub allocator_committed_peak_bytes: Option<u64>,
}

impl DebugMemoryResponse {
    pub fn of(memory: &fah_model::MemoryBreakdown, cache_entries: u64) -> Self {
        Self {
            memory: MemoryResponse::of(memory, cache_entries),
            allocator_committed_bytes: memory.allocator.map(|alloc| alloc.current_commit),
            allocator_committed_peak_bytes: memory.allocator.map(|alloc| alloc.peak_commit),
        }
    }
}

/// Two-decimal rounding for percentages — `72.61`, not `72.61000000000001`.
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
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

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
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
            event: fah_model::Event::dns(QueryEvent::new(
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
                None,
                fah_model::ClientTransport::Udp,
            )),
            client_name: Some("liviu-phone".to_string()),
        };

        let json = serde_json::to_value(QueryItemResponse::from(record)).unwrap();
        let object = json.as_object().unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "bytes",
                "cached",
                "client",
                "client_name",
                "domain",
                "duration_ms",
                "kind",
                "list",
                "method",
                "path",
                "qtype",
                "resource_type",
                "rule",
                "status",
                "transport",
                "ts",
                "upstream",
                "verdict",
            ]
        );
        assert_eq!(object["kind"], "dns");
        assert_eq!(object["transport"], "udp");
        assert_eq!(
            object["method"],
            serde_json::Value::Null,
            "HTTP-only fields must be null on a DNS item, not absent — a client              should not have to distinguish 'missing' from 'not applicable'"
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
    fn cache_stats_serialize_with_a_rounded_load_percent() {
        let response = CacheStatsResponse::from(CacheStats {
            entries: 7_261,
            capacity: 10_000,
            fresh: 7_026,
            stale: 52,
            expired: 183,
            hits: 18_639_283,
            misses: 1_543_921,
            evictions: 21_483,
            bytes: 2_000_000,
            max_bytes: 8_000_000,
            estimated_bytes: 2_846_720,
        });
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["entries"], 7_261);
        assert_eq!(json["capacity"], 10_000);
        assert_eq!(json["fresh"], 7_026);
        assert_eq!(json["stale"], 52);
        assert_eq!(json["expired"], 183);
        assert_eq!(json["hits"], 18_639_283u64);
        assert_eq!(json["misses"], 1_543_921);
        assert_eq!(json["evictions"], 21_483);
        assert_eq!(json["load_percent"], 72.61);
        // The enforced byte bound is part of the cache view (p1.5-05): a
        // dashboard graphs it next to the entry load to see which one binds.
        assert_eq!(json["bytes"], 2_000_000);
        assert_eq!(json["max_bytes"], 8_000_000);
        assert_eq!(json["byte_load_percent"], 25.0);
        assert!(
            json.get("estimated_bytes").is_none(),
            "the slab-inclusive estimate stays with /debug/memory"
        );
    }

    #[test]
    fn an_empty_cache_reports_zero_load_not_a_division_error() {
        let response = CacheStatsResponse::from(CacheStats {
            entries: 0,
            capacity: 0,
            fresh: 0,
            stale: 0,
            expired: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            bytes: 0,
            max_bytes: 0,
            estimated_bytes: 0,
        });
        assert_eq!(response.load_percent, 0.0);
        assert_eq!(response.byte_load_percent, 0.0);
    }

    #[test]
    fn cache_clean_reports_duration_in_milliseconds() {
        let response = CacheCleanResponse::from(CacheClean {
            removed_expired: 1_834,
            removed_stale: 0,
            entries_before: 9_095,
            entries_after: 7_261,
            freed_bytes: 2_846_720,
            duration: Duration::from_micros(4_700),
        });
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["removed_expired"], 1_834);
        assert_eq!(json["removed_stale"], 0);
        assert_eq!(json["entries_before"], 9_095);
        assert_eq!(json["entries_after"], 7_261);
        assert_eq!(json["freed_bytes"], 2_846_720);
        assert_eq!(json["duration_ms"], 4.7);
    }

    #[test]
    fn clean_params_default_to_keeping_stale_entries() {
        let params: CacheCleanParams = serde_json::from_str("{}").unwrap();
        assert!(!params.stale);
        let params: CacheCleanParams = serde_json::from_str(r#"{"stale": true}"#).unwrap();
        assert!(params.stale);
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

#[derive(Debug, Serialize)]
pub struct InterceptedResponse {
    pub completed: u64,
    pub rejected: u64,
    #[serde(serialize_with = "timestamp::serialize_option")]
    pub last_completed: Option<SystemTime>,
    #[serde(serialize_with = "timestamp::serialize_option")]
    pub last_rejected: Option<SystemTime>,
}

impl From<crate::ports::InterceptedHandshakes> for InterceptedResponse {
    fn from(intercepted: crate::ports::InterceptedHandshakes) -> Self {
        Self {
            completed: intercepted.completed,
            rejected: intercepted.rejected,
            last_completed: intercepted.last_completed,
            last_rejected: intercepted.last_rejected,
        }
    }
}
