//! The handles this crate reaches its L3 siblings through.
//!
//! `fah-stats`, `fah-metrics` and `fah-dns` sit on the same layer as
//! `fah-api`, and siblings never import each other (ARCHITECTURE.md
//! §Dependency Layering) — the binary wires them "via channels and handles".
//! These traits are those handles: `fah-api` states what it needs, and
//! `fastadhunter` (L4, the one crate that may see both sides) implements them
//! over `Arc<fah_stats::Stats>` and `Arc<fah_metrics::Metrics>`. Same pattern
//! `fah-metrics` already uses for its `UpstreamSnapshot`/`RulesetSnapshot`
//! DTOs.
//!
//! The DTOs here carry `SystemTime` and `fah_model` types — L1, legal for
//! everyone. RFC 3339 formatting is the wire layer's job ([`crate::wire`]).

use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use fah_model::{QueryEvent, Verdict};

/// Product data: aggregates, query log and the client registry
/// (API.md §Statistics & query log, §Clients).
pub trait StatsSource: Send + Sync + 'static {
    fn overview(&self, now: SystemTime) -> StatsOverview;
    fn queries(&self, request: &QueryLogRequest) -> QueryLogPage;
    fn clients(&self, now: SystemTime) -> Vec<ClientEntry>;
    /// `None` when the IP has never been seen — the API answers `404`.
    fn set_client_name(&self, ip: IpAddr, name: Option<String>) -> Option<ClientEntry>;
    /// One client's name, for decorating live WS query events.
    fn client_name(&self, ip: IpAddr) -> Option<String>;
}

/// Ops telemetry (API.md §Health & telemetry).
pub trait TelemetrySource: Send + Sync + 'static {
    /// The `/metrics` body, already in Prometheus text exposition format.
    fn prometheus_text(&self) -> String;
    /// True when every configured upstream is currently failing — API.md's
    /// `"degraded"` health status ("e.g. all upstreams failing — serve-stale
    /// active").
    fn degraded(&self) -> bool;
}

/// The DNS cache's admin plane (API.md §Cache) — implemented by the binary
/// over the `fah-dns` pipeline, the same crossing as the other ports.
pub trait CacheSource: Send + Sync + 'static {
    /// Usage snapshot for `GET /api/v1/cache`.
    fn stats(&self) -> CacheStats;
    /// `POST /api/v1/cache/clean`: removes expired entries — and, when
    /// `purge_stale`, the RFC 8767 stale-window entries too.
    fn clean(&self, purge_stale: bool) -> CacheClean;
}

/// Cache usage at one point in time. Counter fields (`hits`, `misses`,
/// `evictions`) are process-lifetime totals; the rest describe current
/// entries by lifetime stage (CONTEXT.md §Cache: fresh / stale / expired).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: u64,
    pub capacity: u64,
    pub fresh: u64,
    pub stale: u64,
    pub expired: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// Coarse heap estimate — documented as such wherever it is served.
    pub estimated_bytes: u64,
}

/// The outcome of one cache clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheClean {
    pub removed_expired: u64,
    pub removed_stale: u64,
    pub entries_before: u64,
    pub entries_after: u64,
    pub freed_bytes: u64,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsOverview {
    pub window: &'static str,
    pub queries_total: u64,
    pub blocked_total: u64,
    pub blocked_percent: f64,
    pub cache_hit_percent: f64,
    pub top_blocked_domains: Vec<DomainCount>,
    pub top_queried_domains: Vec<DomainCount>,
    pub top_clients: Vec<ClientCount>,
    pub buckets: Vec<BucketCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainCount {
    pub domain: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCount {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketCount {
    pub start: SystemTime,
    pub queries: u64,
    pub blocked: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientEntry {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub first_seen: SystemTime,
    pub last_seen: SystemTime,
    pub queries_24h: u64,
    pub blocked_24h: u64,
}

/// The parsed query string of `GET /api/v1/queries`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryLogRequest {
    pub limit: usize,
    pub cursor: Option<String>,
    pub client: Option<IpAddr>,
    pub domain: Option<String>,
    pub verdict: Option<VerdictFilter>,
    pub from: Option<SystemTime>,
    pub to: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictFilter {
    Allow,
    Block,
    Pass,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryLogPage {
    pub items: Vec<QueryRecord>,
    pub next_cursor: Option<String>,
}

/// One query-log row. Wraps the L1 [`QueryEvent`] rather than restating its
/// fields, plus the client name resolved when the row was recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRecord {
    pub event: QueryEvent,
    pub client_name: Option<String>,
}

impl QueryRecord {
    /// The decisive rule and its list, or `(None, None)` for a `Pass` —
    /// API.md's `rule`/`list` fields are null when nothing matched.
    pub fn decisive(&self) -> (Option<&str>, Option<&str>) {
        match &self.event.verdict {
            Verdict::Allow(rule) | Verdict::Block(rule) => {
                (Some(rule.rule.as_ref()), Some(rule.list.as_ref()))
            }
            Verdict::Pass => (None, None),
        }
    }

    pub fn verdict_str(&self) -> &'static str {
        match self.event.verdict {
            Verdict::Allow(_) => "allow",
            Verdict::Block(_) => "block",
            Verdict::Pass => "pass",
        }
    }

    pub fn duration(&self) -> Duration {
        self.event.duration
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use fah_model::{DecisiveRule, Query, QueryType};

    use super::*;

    fn record(verdict: Verdict) -> QueryRecord {
        QueryRecord {
            event: QueryEvent::new(
                Query::new(
                    "ads.example.com",
                    QueryType::A,
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                    SystemTime::UNIX_EPOCH,
                ),
                verdict,
                Duration::from_micros(300),
                false,
                false,
                false,
            ),
            client_name: None,
        }
    }

    #[test]
    fn decisive_rule_is_reported_for_block_and_allow_but_null_for_pass() {
        let blocked = record(Verdict::Block(DecisiveRule::new(
            "oisd-basic",
            "||ads.example.com^",
        )));
        assert_eq!(
            blocked.decisive(),
            (Some("||ads.example.com^"), Some("oisd-basic"))
        );
        assert_eq!(blocked.verdict_str(), "block");

        let passed = record(Verdict::Pass);
        assert_eq!(passed.decisive(), (None, None));
        assert_eq!(passed.verdict_str(), "pass");
    }
}
