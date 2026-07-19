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
    BucketCount, ClientCount, ClientEntry, DomainCount, QueryLogPage, QueryLogRequest, QueryRecord,
    StatsOverview, StatsSource, TelemetrySource, VerdictFilter,
};
use fah_dns::UpstreamPool;
use fah_metrics::Metrics;
use fah_stats::{QueryLogFilter, Stats, VerdictKind};

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
        }
    }

    fn queries(&self, request: &QueryLogRequest) -> QueryLogPage {
        let filter = QueryLogFilter {
            client: request.client,
            domain: request.domain.clone(),
            verdict: request.verdict.map(|kind| match kind {
                VerdictFilter::Allow => VerdictKind::Allow,
                VerdictFilter::Block => VerdictKind::Block,
                VerdictFilter::Pass => VerdictKind::Pass,
            }),
            from: request.from,
            to: request.to,
        };
        let page = self
            .stats
            .query_log(&filter, request.limit, request.cursor.as_deref());
        QueryLogPage {
            items: page
                .items
                .into_iter()
                .map(|entry| QueryRecord {
                    event: entry.event,
                    client_name: entry.client_name,
                })
                .collect(),
            next_cursor: page.next_cursor,
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
}

fn client(view: fah_stats::ClientView) -> ClientEntry {
    ClientEntry {
        ip: view.ip,
        name: view.name,
        first_seen: view.first_seen,
        last_seen: view.last_seen,
        queries_24h: view.queries_24h,
        blocked_24h: view.blocked_24h,
    }
}

pub struct TelemetryAdapter {
    metrics: Arc<Metrics>,
    upstreams: UpstreamPool,
}

impl TelemetryAdapter {
    pub fn new(metrics: Arc<Metrics>, upstreams: UpstreamPool) -> Self {
        Self { metrics, upstreams }
    }
}

impl TelemetrySource for TelemetryAdapter {
    fn prometheus_text(&self) -> String {
        fah_metrics::encode(&self.metrics)
    }

    /// API.md's `degraded`: "all upstreams failing — serve-stale active".
    /// Read straight off the pool rather than the metrics snapshot so health
    /// never lags behind the poller's interval. A pool that has answered
    /// nothing yet (every server at zero attempts) is not degraded — it is
    /// simply idle.
    fn degraded(&self) -> bool {
        let status = self.upstreams.status();
        !status.is_empty() && status.iter().all(|server| server.consecutive_failures > 0)
    }
}
