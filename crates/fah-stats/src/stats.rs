//! [`Stats`]: owns aggregates, client registry and query log; the internal
//! handle `fah-api` (p1-09) will hold to serve API.md's stats/query/client
//! endpoints. Consumes `QueryEvent`s from the bounded channel `fah-dns`
//! emits into — created and wired in `fastadhunter` (siblings never import
//! each other, ARCHITECTURE.md §Dependency Layering).

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fah_config::{QueryLogConfig, StatsConfig};
use fah_model::QueryEvent;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::aggregates::Aggregates;
use crate::client_registry::{ClientRegistry, ClientView};
use crate::dto::{ClientCount, DomainCount, StatsSnapshot};
use crate::query_log::ring::Ring;
use crate::query_log::segment::SegmentWriter;
use crate::query_log::{QueryLogFilter, QueryPage};
use crate::snapshot::{self, SnapshotData};

const DEFAULT_TOP_N: usize = 10;

/// Cap on entries buffered between segment flushes (hard rule 4: a stalled
/// disk or a dead flush task must not let memory grow with traffic). Sized
/// for ~3k QPS over a 5 s flush interval; beyond that, newest entries are
/// dropped and counted — they're still in the ring for the query API.
const MAX_PENDING_LOG: usize = 16_384;

pub struct Stats {
    query_log_enabled: bool,
    data_dir: PathBuf,
    snapshot_interval: Duration,
    flush_interval: Duration,
    retention_days: u32,
    retention_max_mb: u32,
    aggregates: Mutex<Aggregates>,
    clients: Mutex<ClientRegistry>,
    ring: Mutex<Ring>,
    /// Entries recorded since the last flush — batched to keep segment
    /// writes off the per-query path (flushed by the scheduler, not by
    /// [`Stats::record`]). Capped at [`MAX_PENDING_LOG`]; overflow is
    /// dropped and counted in `pending_dropped`.
    pending_log: Mutex<Vec<crate::query_log::QueryLogEntry>>,
    /// Entries dropped from the pending batch because the cap was hit —
    /// exposed via [`Self::query_log_overflow_dropped`] for fah-metrics.
    pending_dropped: AtomicU64,
    /// Touched only by the flush scheduler, but async (rotate/prune do I/O),
    /// so it's a `tokio::sync::Mutex` — mirrors `fah_rules::ListManager`'s
    /// `compile_lock` (a lock guarding a section that awaits).
    segment: tokio::sync::Mutex<SegmentWriter>,
}

impl Stats {
    pub fn new(
        stats_config: &StatsConfig,
        query_log_config: &QueryLogConfig,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            query_log_enabled: query_log_config.enabled,
            snapshot_interval: Duration::from_secs(u64::from(
                stats_config.snapshot_interval_seconds.max(1),
            )),
            flush_interval: Duration::from_secs(u64::from(
                query_log_config.flush_interval_seconds.max(1),
            )),
            retention_days: query_log_config.retention_days,
            retention_max_mb: query_log_config.retention_max_mb,
            aggregates: Mutex::new(Aggregates::default()),
            clients: Mutex::new(ClientRegistry::default()),
            ring: Mutex::new(Ring::new(query_log_config.ring_entries as usize)),
            pending_log: Mutex::new(Vec::new()),
            pending_dropped: AtomicU64::new(0),
            segment: tokio::sync::Mutex::new(SegmentWriter::new(
                data_dir.join("query_log").join("segments"),
            )),
            data_dir,
        }
    }

    /// Loads the persisted snapshot (if any) and resumes segment numbering —
    /// no network, mirrors `ListManager::boot`'s cache-first convention.
    pub async fn boot(&self) {
        if let Some(data) = snapshot::load(&self.data_dir).await {
            *self.aggregates.lock().unwrap() = data.aggregates;
            *self.clients.lock().unwrap() = data.clients;
        }
        if self.query_log_enabled {
            if let Err(err) = self.segment.lock().await.boot().await {
                tracing::warn!(error = %err, "failed to initialize query log segment writer");
            }
        }
    }

    /// Records one completed query. The sole write path — called by the
    /// event-consumer task ([`Self::spawn_collector`]) and directly in tests.
    pub fn record(&self, event: QueryEvent) {
        let at = event.query.timestamp;
        let blocked = matches!(event.verdict, fah_model::Verdict::Block(_));

        self.aggregates.lock().unwrap().record(
            &event.query.domain,
            &event.verdict,
            event.cache_hit,
            at,
        );
        let client_name = {
            let mut clients = self.clients.lock().unwrap();
            clients.record(event.query.client_ip, at, blocked, event.cache_hit);
            clients.name(event.query.client_ip)
        };

        if !self.query_log_enabled {
            return;
        }
        let entry = self.ring.lock().unwrap().push(event, client_name);
        let mut pending = self.pending_log.lock().unwrap();
        if pending.len() >= MAX_PENDING_LOG {
            self.pending_dropped.fetch_add(1, Ordering::Relaxed);
        } else {
            pending.push(entry);
        }
    }

    /// Query-log entries lost to the pending-batch cap (flush stalled or
    /// falling behind). The DNS-side channel drops are counted separately,
    /// on the producer (`fah_dns::Pipeline::dropped_events`).
    pub fn query_log_overflow_dropped(&self) -> u64 {
        self.pending_dropped.load(Ordering::Relaxed)
    }

    /// Consumes `QueryEvent`s until the sender side closes (a slow consumer's
    /// drops are already counted on the producer side —
    /// `fah_dns::Pipeline::dropped_events`; the collector itself never drops,
    /// it just processes as fast as `record` allows).
    pub fn spawn_collector(
        self: &Arc<Self>,
        mut events: mpsc::Receiver<QueryEvent>,
    ) -> JoinHandle<()> {
        let stats = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                stats.record(event);
            }
        })
    }

    pub fn spawn_snapshot_scheduler(self: &Arc<Self>) -> JoinHandle<()> {
        let stats = Arc::clone(self);
        let interval = self.snapshot_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                stats.save_snapshot().await;
            }
        })
    }

    pub async fn save_snapshot(&self) {
        let data = SnapshotData {
            aggregates: self.aggregates.lock().unwrap().clone(),
            clients: self.clients.lock().unwrap().clone(),
        };
        if let Err(err) = snapshot::save(&self.data_dir, &data).await {
            tracing::warn!(error = %err, "failed to save stats snapshot");
        }
    }

    pub fn spawn_query_log_scheduler(self: &Arc<Self>) -> JoinHandle<()> {
        let stats = Arc::clone(self);
        let interval = self.flush_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                stats.flush_query_log().await;
            }
        })
    }

    pub async fn flush_query_log(&self) {
        if !self.query_log_enabled {
            return;
        }
        let batch = std::mem::take(&mut *self.pending_log.lock().unwrap());
        let mut segment = self.segment.lock().await;
        let rotated = match segment.append(&batch).await {
            Ok(rotated) => rotated,
            Err(err) => {
                tracing::warn!(error = %err, "failed to flush query log segment; will retry");
                drop(segment);
                self.requeue(batch);
                return;
            }
        };
        if let Err(err) = segment
            .maybe_prune(
                self.retention_days,
                self.retention_max_mb,
                SystemTime::now(),
                rotated,
            )
            .await
        {
            tracing::warn!(error = %err, "failed to prune query log segments");
        }
    }

    /// Puts a batch that failed to persist back in front of whatever was
    /// recorded meanwhile, so a transient disk error loses nothing; the
    /// [`MAX_PENDING_LOG`] cap still holds (oldest dropped and counted), so
    /// a *persistent* error can't grow memory (hard rule 4).
    fn requeue(&self, mut batch: Vec<crate::query_log::QueryLogEntry>) {
        let mut pending = self.pending_log.lock().unwrap();
        batch.append(&mut pending);
        if batch.len() > MAX_PENDING_LOG {
            let overflow = batch.len() - MAX_PENDING_LOG;
            batch.drain(..overflow);
            self.pending_dropped
                .fetch_add(overflow as u64, Ordering::Relaxed);
        }
        *pending = batch;
    }

    /// Every field is windowed to the rolling 24h the `window` field
    /// declares (API.md `GET /api/v1/stats`).
    pub fn snapshot(&self, now: SystemTime) -> StatsSnapshot {
        let aggregates = self.aggregates.lock().unwrap();
        let clients = self.clients.lock().unwrap();
        StatsSnapshot {
            window: "24h",
            queries_total: aggregates.queries_total(now),
            blocked_total: aggregates.blocked_total(now),
            blocked_percent: aggregates.blocked_percent(now),
            cache_hit_percent: aggregates.cache_hit_percent(now),
            top_blocked_domains: to_domain_counts(aggregates.top_blocked(DEFAULT_TOP_N, now)),
            top_queried_domains: to_domain_counts(aggregates.top_queried(DEFAULT_TOP_N, now)),
            top_clients: clients
                .top_by_activity(now, DEFAULT_TOP_N)
                .into_iter()
                .map(|c| ClientCount {
                    ip: c.ip,
                    name: c.name,
                    count: c.queries_24h,
                })
                .collect(),
            buckets: aggregates.buckets(now),
        }
    }

    pub fn query_log(
        &self,
        filter: &QueryLogFilter,
        limit: usize,
        cursor: Option<&str>,
    ) -> QueryPage {
        let cursor = cursor.and_then(|c| c.parse::<u64>().ok());
        let filter = filter.normalized();
        self.ring.lock().unwrap().query(&filter, limit, cursor)
    }

    pub fn clients(&self, now: SystemTime) -> Vec<ClientView> {
        self.clients.lock().unwrap().list(now)
    }

    /// One client's assigned name, if it has one. Used to decorate live
    /// `WS /api/v1/events` query events, where re-listing every client per
    /// event would be far too heavy.
    pub fn client_name(&self, ip: IpAddr) -> Option<String> {
        self.clients.lock().unwrap().name(ip)
    }

    pub fn set_client_name(&self, ip: IpAddr, name: Option<String>) -> Option<ClientView> {
        self.clients.lock().unwrap().set_name(ip, name)
    }
}

fn to_domain_counts(top: Vec<(std::sync::Arc<str>, u64)>) -> Vec<DomainCount> {
    top.into_iter()
        .map(|(domain, count)| DomainCount {
            domain: domain.to_string(),
            count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use fah_model::{DecisiveRule, Query, QueryType, Verdict};

    use super::*;

    fn config() -> (StatsConfig, QueryLogConfig) {
        (
            StatsConfig {
                snapshot_interval_seconds: 300,
            },
            QueryLogConfig {
                enabled: true,
                ring_entries: 100,
                retention_days: 7,
                retention_max_mb: 500,
                flush_interval_seconds: 5,
            },
        )
    }

    fn event(domain: &str, client: IpAddr, verdict: Verdict) -> QueryEvent {
        QueryEvent::new(
            Query::new(domain, QueryType::A, client, SystemTime::now()),
            verdict,
            std::time::Duration::from_micros(100),
            false,
            true,
            false,
        )
    }

    #[tokio::test]
    async fn record_updates_aggregates_registry_and_ring() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event(
            "ads.example.com",
            client,
            Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
        ));
        stats.record(event("example.com", client, Verdict::Pass));

        let snapshot = stats.snapshot(SystemTime::now());
        assert_eq!(snapshot.queries_total, 2);
        assert_eq!(snapshot.blocked_total, 1);
        assert_eq!(snapshot.top_clients[0].ip, client);

        let page = stats.query_log(&QueryLogFilter::default(), 10, None);
        assert_eq!(page.items.len(), 2);
    }

    #[tokio::test]
    async fn client_name_is_resolved_onto_new_log_entries() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));
        stats.set_client_name(client, Some("liviu-phone".to_string()));
        stats.record(event("second.example.com", client, Verdict::Pass));

        let page = stats.query_log(&QueryLogFilter::default(), 10, None);
        assert_eq!(page.items[0].client_name.as_deref(), Some("liviu-phone"));
        assert_eq!(page.items[1].client_name, None);
    }

    #[tokio::test]
    async fn collector_consumes_events_from_the_channel() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Arc::new(Stats::new(
            &stats_config,
            &query_log_config,
            dir.path().to_path_buf(),
        ));
        stats.boot().await;

        let (tx, rx) = mpsc::channel(8);
        let handle = stats.spawn_collector(rx);

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        tx.send(event("example.com", client, Verdict::Pass))
            .await
            .unwrap();
        drop(tx);
        handle.await.unwrap();

        assert_eq!(stats.snapshot(SystemTime::now()).queries_total, 1);
    }

    #[tokio::test]
    async fn kill_and_restart_recovers_stats_from_the_last_snapshot() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        {
            let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
            stats.boot().await;
            stats.record(event(
                "ads.example.com",
                client,
                Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
            ));
            stats.save_snapshot().await; // simulates the periodic snapshot beating the kill -9
        }

        let restarted = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        restarted.boot().await;

        let snapshot = restarted.snapshot(SystemTime::now());
        assert_eq!(snapshot.queries_total, 1);
        assert_eq!(snapshot.blocked_total, 1);
    }

    #[tokio::test]
    async fn flush_persists_pending_entries_to_a_segment() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));
        stats.flush_query_log().await;

        let segments_dir = dir.path().join("query_log").join("segments");
        let mut entries = tokio::fs::read_dir(&segments_dir).await.unwrap();
        let first = entries.next_entry().await.unwrap();
        assert!(
            first.is_some(),
            "flush must write at least one segment file"
        );
    }

    #[tokio::test]
    async fn pending_log_stays_bounded_when_flush_never_runs() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let extra = 100u64;
        for _ in 0..(MAX_PENDING_LOG as u64 + extra) {
            stats.record(event("example.com", client, Verdict::Pass));
        }

        assert_eq!(stats.pending_log.lock().unwrap().len(), MAX_PENDING_LOG);
        assert_eq!(stats.query_log_overflow_dropped(), extra);
    }

    #[tokio::test]
    async fn failed_flush_keeps_entries_for_the_next_attempt() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        // A *file* where the query_log directory should be makes every
        // segment write fail until it's removed.
        let blocker = dir.path().join("query_log");
        tokio::fs::write(&blocker, "in the way").await.unwrap();

        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));
        stats.flush_query_log().await;
        assert_eq!(
            stats.pending_log.lock().unwrap().len(),
            1,
            "a failed flush must keep the batch for the next attempt"
        );

        tokio::fs::remove_file(&blocker).await.unwrap();
        stats.flush_query_log().await;
        assert!(stats.pending_log.lock().unwrap().is_empty());
        let segments_dir = dir.path().join("query_log").join("segments");
        let mut entries = tokio::fs::read_dir(&segments_dir).await.unwrap();
        assert!(entries.next_entry().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn domain_filter_is_case_insensitive() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        // The pipeline lowercases domains before emitting events.
        stats.record(event("ads.example.com", client, Verdict::Pass));

        let filter = QueryLogFilter {
            domain: Some("ADS".to_string()),
            ..Default::default()
        };
        let page = stats.query_log(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
    }

    #[tokio::test]
    async fn disabled_query_log_skips_the_ring_and_segments() {
        let (stats_config, mut query_log_config) = config();
        query_log_config.enabled = false;
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &query_log_config, dir.path().to_path_buf());
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));

        let page = stats.query_log(&QueryLogFilter::default(), 10, None);
        assert!(page.items.is_empty());
        assert_eq!(stats.snapshot(SystemTime::now()).queries_total, 1);
    }
}
