//! Where the L3 siblings meet.
//!
//! `fah-api` declares what it needs as traits and never imports `fah-stats`,
//! `fah-metrics` or `fah-dns` (ARCHITECTURE.md §Dependency Layering: siblings
//! never import each other). This module — inside the binary, the one crate
//! allowed to see both sides — is the translation: field-for-field copies
//! from each sibling's own types into `fah-api`'s port DTOs.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::SystemTime;

use fah_api::{
    BucketCount, ClientCount, ClientEntry, DomainCount, HistorySource, PolicyCount, StatsOverview,
    StatsSource, TelemetrySource,
};
use fah_dns::{UpstreamPool, UpstreamStatus};
use fah_metrics::Metrics;
use fah_model::{HistoryRange, HistoryResolution, HistorySeries, PerfSeries, TopItems, TopKind};
use fah_stats::Stats;

/// Lets the Rule Engine's list fetcher (L2) resolve download hosts through the
/// DNS engine's upstreams (L3) — the same crossing as above, in the other
/// direction: `fah-rules` declares [`fah_rules::HostResolver`] and never
/// imports `fah-dns`.
///
/// Without this the fetcher uses the system resolver, and the container has no
/// working one: RouterOS ships `/etc/resolv.conf` empty (p1-11 defect 5). A
/// process that *is* a DNS resolver should not need a second one to fetch its
/// own lists.
pub struct UpstreamResolver {
    upstreams: UpstreamPool,
}

impl UpstreamResolver {
    pub fn new(upstreams: UpstreamPool) -> Self {
        Self { upstreams }
    }
}

impl fah_rules::HostResolver for UpstreamResolver {
    fn resolve(&self, host: String) -> fah_rules::Resolving {
        // `UpstreamPool` is a cheap `Arc` clone, which is what lets this hand
        // back the owned `'static` future the port requires.
        let upstreams = self.upstreams.clone();
        Box::pin(async move { upstreams.resolve_host(&host).await })
    }
}

pub struct StatsAdapter {
    stats: Arc<Stats>,
}

impl StatsAdapter {
    pub fn new(stats: Arc<Stats>) -> Self {
        Self { stats }
    }
}

impl StatsSource for StatsAdapter {
    fn overview(&self, now: SystemTime) -> StatsOverview {
        let snapshot = self.stats.snapshot(now);
        StatsOverview {
            window: snapshot.window,
            queries_total: snapshot.queries_total,
            blocked_total: snapshot.blocked_total,
            blocked_percent: snapshot.blocked_percent,
            cache_hit_percent: snapshot.cache_hit_percent,
            top_blocked_domains: snapshot
                .top_blocked_domains
                .into_iter()
                .map(|d| DomainCount {
                    domain: d.domain,
                    count: d.count,
                })
                .collect(),
            top_queried_domains: snapshot
                .top_queried_domains
                .into_iter()
                .map(|d| DomainCount {
                    domain: d.domain,
                    count: d.count,
                })
                .collect(),
            top_clients: snapshot
                .top_clients
                .into_iter()
                .map(|c| ClientCount {
                    ip: c.ip,
                    name: c.name,
                    count: c.count,
                })
                .collect(),
            buckets: snapshot
                .buckets
                .into_iter()
                .map(|b| BucketCount {
                    start: b.start,
                    queries: b.queries,
                    blocked: b.blocked,
                })
                .collect(),
            policies: snapshot
                .policies
                .into_iter()
                .map(|p| PolicyCount {
                    policy: p.policy.to_string(),
                    queries: p.queries,
                    blocked: p.blocked,
                })
                .collect(),
        }
    }

    fn clients(&self, now: SystemTime) -> Vec<ClientEntry> {
        self.stats.clients(now).into_iter().map(client).collect()
    }

    fn set_client_name(&self, ip: IpAddr, name: Option<String>) -> Option<ClientEntry> {
        self.stats.set_client_name(ip, name).map(client)
    }

    fn client_name(&self, ip: IpAddr) -> Option<String> {
        self.stats.client_name(ip)
    }

    fn named_clients(&self) -> Vec<(IpAddr, Arc<str>)> {
        self.stats.named_clients()
    }

    fn apply_history_config(&self, enabled: bool, retention_days: u32) {
        self.stats.set_history_enabled(enabled);
        self.stats.set_history_retention_days(retention_days);
    }

    /// Pass-through: both sides speak `fah_model::StatsHeap`, so there is
    /// nothing to translate (p2-07).
    fn heap(&self) -> fah_model::StatsHeap {
        self.stats.heap()
    }
}

/// The history reads, on the same `Arc<Stats>` handle. A pass-through rather
/// than a translation: both sides speak the `fah_model` (L1) history contract,
/// so there is nothing here that could drift from what the reader returns.
impl HistorySource for StatsAdapter {
    fn summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> std::io::Result<HistorySeries> {
        self.stats.history_summary(range, resolution, max_points)
    }

    fn perf(
        &self,
        range: HistoryRange,
        max_points: usize,
        include_upstreams: bool,
    ) -> std::io::Result<PerfSeries> {
        self.stats
            .history_perf(range, max_points, include_upstreams)
    }

    fn top(&self, range: HistoryRange, kind: TopKind, limit: usize) -> std::io::Result<TopItems> {
        self.stats.history_top(range, kind, limit)
    }
}

fn client(view: fah_stats::ClientView) -> ClientEntry {
    ClientEntry {
        ip: view.ip,
        name: view.name,
        first_seen: view.first_seen,
        last_seen: view.last_seen,
        queries_24h: view.queries_24h,
        blocked_24h: view.blocked_24h,
        intercepted: fah_api::InterceptedHandshakes {
            completed: view.intercepted.completed,
            rejected: view.intercepted.rejected,
            last_completed: view.intercepted.last_completed,
            last_rejected: view.intercepted.last_rejected,
        },
    }
}

pub struct DnsWireAdapter {
    pipeline: Arc<fah_dns::Pipeline<UpstreamPool>>,
}

impl DnsWireAdapter {
    pub fn new(pipeline: Arc<fah_dns::Pipeline<UpstreamPool>>) -> Self {
        Self { pipeline }
    }
}

impl fah_api::DnsWireSource for DnsWireAdapter {
    fn resolve(&self, message: Vec<u8>, client: IpAddr) -> fah_api::WireResolving {
        let pipeline = Arc::clone(&self.pipeline);
        Box::pin(async move {
            pipeline
                .handle(&message, client, fah_dns::Transport::Doh)
                .await
        })
    }
}

pub struct CacheAdapter {
    pipeline: Arc<fah_dns::Pipeline<UpstreamPool>>,
}

impl CacheAdapter {
    pub fn new(pipeline: Arc<fah_dns::Pipeline<UpstreamPool>>) -> Self {
        Self { pipeline }
    }
}

impl fah_api::CacheSource for CacheAdapter {
    fn stats(&self) -> fah_api::CacheStats {
        let stats = self.pipeline.cache_stats();
        fah_api::CacheStats {
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
            estimated_bytes: stats.estimated_bytes,
        }
    }

    fn clean(&self, purge_stale: bool) -> fah_api::CacheClean {
        let clean = self.pipeline.cache_clean(purge_stale);
        fah_api::CacheClean {
            removed_expired: clean.removed_expired,
            removed_stale: clean.removed_stale,
            entries_before: clean.entries_before,
            entries_after: clean.entries_after,
            freed_bytes: clean.freed_bytes,
            duration: clean.duration,
        }
    }
}

pub struct TelemetryAdapter {
    metrics: Arc<Metrics>,
    upstreams: UpstreamPool,
    http: Option<Arc<fah_http::ProxyCounters>>,
    https: Option<Arc<fah_http::ProxyCounters>>,
}

impl TelemetryAdapter {
    pub fn new(
        metrics: Arc<Metrics>,
        upstreams: UpstreamPool,
        http: Option<Arc<fah_http::ProxyCounters>>,
        https: Option<Arc<fah_http::ProxyCounters>>,
    ) -> Self {
        Self {
            metrics,
            upstreams,
            http,
            https,
        }
    }
}

impl TelemetrySource for TelemetryAdapter {
    /// API.md's `degraded`: "all upstreams failing — serve-stale active".
    /// Read straight off the pool rather than the metrics snapshot so health
    /// never lags behind the poller's interval. A pool that has answered
    /// nothing yet (every server at zero attempts) is not degraded — it is
    /// simply idle.
    fn degraded(&self) -> bool {
        upstreams_degraded(&self.upstreams.status())
    }

    /// Straight through to the binary's allocator module — the one place that
    /// knows which allocator is installed (see `crates/fastadhunter/src/allocator.rs`). Read now rather than taken
    /// from the last telemetry poll, so `/debug/memory` gets figures from the
    /// same instant as the component heaps it reads beside them.
    fn allocator(&self) -> Option<fah_model::AllocatorStats> {
        crate::allocator::stats()
    }

    fn process(&self) -> Option<fah_model::ProcessStats> {
        crate::process::stats()
    }

    /// Straight through: the registry already assembles the L1 value, and it is
    /// the only place that can read its own atomics.
    fn engine(&self) -> fah_model::EngineTelemetry {
        self.metrics.engine_telemetry()
    }

    fn listeners(&self) -> fah_model::ListenerTelemetry {
        let read = |counters: &Option<Arc<fah_http::ProxyCounters>>| {
            counters.as_ref().map(|counters| counters.snapshot().into())
        };
        fah_model::ListenerTelemetry {
            http: read(&self.http),
            https: read(&self.https),
        }
    }
}

fn upstreams_degraded(status: &[UpstreamStatus]) -> bool {
    !status.is_empty()
        && status
            .iter()
            .all(|server| server.state != fah_model::UpstreamState::Healthy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fah_model::UpstreamState;

    fn endpoint(state: UpstreamState, consecutive_failures: u64) -> UpstreamStatus {
        UpstreamStatus {
            address: "1.1.1.1".to_string(),
            protocol: fah_model::Protocol::Udp,
            attempts: 10,
            failures: consecutive_failures,
            consecutive_failures,
            tls_handshakes: 0,
            failure_runs: [0, 0, 0, 0],
            state,
            penalty_round: 0,
            penalties: 0,
            penalized_seconds_total: 0,
            probes: 0,
            probe_successes: 0,
            family: Some(fah_model::AddressFamily::V4),
            rtt: fah_model::UpstreamRtt::default(),
        }
    }

    #[test]
    fn adaptive_degraded_when_every_endpoint_is_penalized() {
        let status = [
            endpoint(UpstreamState::Penalized, 3),
            endpoint(UpstreamState::Penalized, 5),
        ];
        assert!(upstreams_degraded(&status));
    }

    #[test]
    fn adaptive_degraded_while_one_endpoint_probes() {
        let status = [
            endpoint(UpstreamState::Penalized, 3),
            endpoint(UpstreamState::Probing, 5),
        ];
        assert!(
            upstreams_degraded(&status),
            "a probe in flight is an attempt, not a recovery"
        );
    }

    #[test]
    fn adaptive_is_not_degraded_with_one_healthy_endpoint() {
        let status = [
            endpoint(UpstreamState::Penalized, 3),
            endpoint(UpstreamState::Healthy, 0),
        ];
        assert!(!upstreams_degraded(&status));
    }

    #[test]
    fn adaptive_ignores_a_frozen_streak_on_a_healthy_endpoint() {
        let status = [endpoint(UpstreamState::Healthy, 7)];
        assert!(
            !upstreams_degraded(&status),
            "consecutive_failures is a diagnostic under adaptive, not the signal"
        );
    }

    #[test]
    fn an_empty_pool_is_idle_not_degraded() {
        assert!(!upstreams_degraded(&[]));
    }
}
