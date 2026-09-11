//! The handles this crate reaches its L3 siblings through.
//!
//! `fah-stats`, `fah-metrics` and `fah-dns` sit on the same layer as
//! `fah-api`, and siblings never import each other (ARCHITECTURE.md
//! §Dependency Layering) — the binary wires them "via channels and handles".
//! These traits are those handles: `fah-api` states what it needs, and
//! `fastadhunter` (L4, the one crate that may see both sides) implements them
//! over `Arc<fah_stats::Stats>` and `Arc<fah_metrics::Metrics>`. Same pattern
//! `fah-metrics` already uses for its `RulesetSnapshot` DTO.
//!
//! The DTOs here carry `SystemTime` and `fah_model` types — L1, legal for
//! everyone. RFC 3339 formatting is the wire layer's job ([`crate::wire`]).

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use fah_model::{
    HistoryRange, HistoryResolution, HistorySeries, PerfSeries, QueryEvent, TopItems, TopKind,
    Verdict,
};

/// Product data: aggregates and the client registry
/// (API.md §Statistics, §Clients).
pub trait StatsSource: Send + Sync + 'static {
    fn overview(&self, now: SystemTime) -> StatsOverview;
    fn clients(&self, now: SystemTime) -> Vec<ClientEntry>;
    /// `None` when the IP has never been seen — the API answers `404`.
    fn set_client_name(&self, ip: IpAddr, name: Option<String>) -> Option<ClientEntry>;
    /// One client's name, for decorating live WS query events.
    fn client_name(&self, ip: IpAddr) -> Option<String>;
    /// Every named client, for resolving name assignments after a policy edit
    /// (p2-06). Bounded by the registry's capacity; never on a query path.
    fn named_clients(&self) -> Vec<(IpAddr, std::sync::Arc<str>)>;
    /// Live-applies the runtime-class `[history]` fields after a
    /// `POST /api/v1/config`. Retention moves the next prune's cut-off via an
    /// atomic shared with both history writers; `enabled` toggles the flush —
    /// neither reconstructs anything (hard rule 3). `sample_interval_seconds` is
    /// boot-class and so is deliberately absent here.
    fn apply_history_config(&self, enabled: bool, retention_days: u32);
    /// This crate's contribution to the p2-07 memory breakdown, for
    /// `GET /debug/memory`. Cheap bounded walks on a read path, never the
    /// query path.
    fn heap(&self) -> fah_model::StatsHeap;
}

/// The persisted history on `/data/history` (API.md §History) — the same
/// crossing as [`StatsSource`], but a separate port because the contract is
/// different in kind: these are **fallible, blocking** file scans, not the
/// cheap infallible in-RAM snapshots the rest of the stats surface serves.
///
/// Every method blocks on `std::fs`. That is deliberate — a multi-day read is
/// one `spawn_blocking` hop for the whole scan, where an async-per-file API
/// would pay a hop per file. Handlers must therefore call these from a blocking
/// task, never straight off a runtime worker.
///
/// Types come from `fah_model` (L1) so the reader and this port describe the
/// same contract rather than two structurally identical ones that can drift.
pub trait HistorySource: Send + Sync + 'static {
    fn summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> io::Result<HistorySeries>;
    fn perf(
        &self,
        range: HistoryRange,
        max_points: usize,
        include_upstreams: bool,
    ) -> io::Result<PerfSeries>;
    fn top(&self, range: HistoryRange, kind: TopKind, limit: usize) -> io::Result<TopItems>;
}

/// Ops telemetry (API.md §Health & telemetry).
pub trait TelemetrySource: Send + Sync + 'static {
    /// True when every configured upstream is currently failing — API.md's
    /// `"degraded"` health status ("e.g. all upstreams failing — serve-stale
    /// active").
    fn degraded(&self) -> bool;
    /// Process-allocator figures for `GET /api/v1/debug/memory`, or `None`
    /// where unavailable.
    ///
    /// Routed through this port rather than read directly, so which allocator
    /// is in use stays a fact about the binary (see `crates/fastadhunter/src/allocator.rs`) and no L3 crate takes
    /// an FFI dependency to publish its numbers. Read per request, matching how
    /// that endpoint samples every other field, since the breakdown's residual
    /// is only meaningful when its inputs share an instant.
    fn allocator(&self) -> Option<fah_model::AllocatorStats>;
    /// Kernel readings (`getrusage`) for `GET /api/v1/telemetry`, or `None` off
    /// Unix. A separate method from [`Self::allocator`] because the two carry
    /// different promises: these survive replacing the allocator, which is why
    /// the stable surface may publish them.
    fn process(&self) -> Option<fah_model::ProcessStats>;
    /// The engine's operational state for `GET /api/v1/telemetry` — counters,
    /// per-stage latency totals, the compiled ruleset and the upstream pool.
    ///
    /// An L1 value rather than a port-local DTO: `EngineTelemetry` is a domain
    /// concept ("what the engine is doing right now"), not a wire shape, so it
    /// lives beside [`fah_model::MemoryBreakdown`] for the same reason — three
    /// crates need it and none of them may import another.
    fn engine(&self) -> fah_model::EngineTelemetry;
    fn listeners(&self) -> fah_model::ListenerTelemetry;
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

pub type WireResolving = Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>>;

pub trait DnsWireSource: Send + Sync + 'static {
    fn resolve(&self, message: Vec<u8>, client: IpAddr) -> WireResolving;
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
    /// What the resident entries hold right now — the figure `max_bytes`
    /// bounds (p1.5-05's byte-aware cap).
    pub bytes: u64,
    /// The enforced byte ceiling, which can round slightly below
    /// `dns.cache.max_bytes` the way `capacity` does below `max_entries`.
    pub max_bytes: u64,
    /// Coarse heap estimate — documented as such wherever it is served.
    /// Larger than `bytes`: it also counts the hash-table and queue slabs,
    /// which the entry bound governs rather than the byte bound.
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
    /// Per-policy activity (p2-06).
    pub policies: Vec<PolicyCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCount {
    pub policy: String,
    pub queries: u64,
    pub blocked: u64,
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
    pub intercepted: InterceptedHandshakes,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InterceptedHandshakes {
    pub completed: u64,
    pub rejected: u64,
    pub last_completed: Option<SystemTime>,
    pub last_rejected: Option<SystemTime>,
}

/// One query-log row. Wraps the L1 [`QueryEvent`] rather than restating its
/// fields, plus the client name resolved when the row was recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRecord {
    pub event: fah_model::Event,
    pub client_name: Option<String>,
}

impl QueryRecord {
    /// The decisive rule and its list, or `(None, None)` for a `Pass` —
    /// API.md's `rule`/`list` fields are null when nothing matched.
    pub fn decisive(&self) -> (Option<&str>, Option<&str>) {
        match self.event.verdict() {
            Verdict::Allow(rule) | Verdict::Block(rule) => {
                (Some(rule.rule.as_ref()), Some(rule.list.as_ref()))
            }
            Verdict::Pass => (None, None),
        }
    }

    pub fn verdict_str(&self) -> &'static str {
        match self.event.verdict() {
            Verdict::Allow(_) => "allow",
            Verdict::Block(_) => "block",
            Verdict::Pass => "pass",
        }
    }

    pub fn duration(&self) -> Duration {
        match &self.event {
            fah_model::Event::Dns(event) => event.duration,
            fah_model::Event::Http(event)
            | fah_model::Event::HttpsSni(event)
            | fah_model::Event::Https(event) => event.duration,
        }
    }

    /// The DNS half, when this record is one. `None` for an HTTP request —
    /// which is what makes the qtype/cached fields honestly absent rather than
    /// defaulted to something that looks like a DNS answer.
    pub fn as_dns(&self) -> Option<&QueryEvent> {
        match &self.event {
            fah_model::Event::Dns(event) => Some(event),
            fah_model::Event::Http(_)
            | fah_model::Event::HttpsSni(_)
            | fah_model::Event::Https(_) => None,
        }
    }

    pub fn as_http(&self) -> Option<&fah_model::RequestEvent> {
        match &self.event {
            fah_model::Event::Http(event)
            | fah_model::Event::HttpsSni(event)
            | fah_model::Event::Https(event) => Some(event),
            fah_model::Event::Dns(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use fah_model::{DecisiveRule, Query, QueryType};

    use super::*;

    fn record(verdict: Verdict) -> QueryRecord {
        QueryRecord {
            event: fah_model::Event::dns(QueryEvent::new(
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
                None,
                fah_model::ClientTransport::Udp,
            )),
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
