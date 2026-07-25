//! [`Stats`]: owns aggregates, client registry and query log; the internal
//! handle `fah-api` (p1-09) will hold to serve API.md's stats/query/client
//! endpoints. Consumes `QueryEvent`s from the bounded channel `fah-dns`
//! emits into — created and wired in `fastadhunter` (siblings never import
//! each other, ARCHITECTURE.md §Dependency Layering).

use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fah_config::{HistoryConfig, QueryLogConfig, StatsConfig};
use fah_model::{
    ClientHits, DailyTopN, DomainHits, HistoryRange, HistoryResolution, HistorySeries, PerfSample,
    PerfSeries, QueryEvent, TopItems, TopKind,
};
use tokio::task::JoinHandle;

use crate::aggregates::Aggregates;
use crate::client_registry::{ClientRegistry, ClientView};
use crate::dto::{ClientCount, DomainCount, StatsSnapshot};
use crate::history::{HistoryReader, PerfWriter, RollupWriter};
use crate::query_log::ring::Ring;
use crate::query_log::segment::SegmentWriter;
use crate::query_log::{QueryLogFilter, QueryPage};
use crate::snapshot::{self, SnapshotData};
use fah_model::StatsHeap;

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
    /// Long-term hourly/daily aggregate rollups on `/data/history` — written by
    /// the history scheduler, never the per-query path (hard rule 3). Async
    /// mutex for the same reason as `segment`.
    history: tokio::sync::Mutex<RollupWriter>,
    /// Per-interval perf/system/cache sample series on `/data/history/perf`.
    /// The binary's sampler builds each [`PerfSample`] (it alone reads
    /// `fah-metrics`, the cache port and RSS) and hands it here to persist —
    /// off the per-query path (hard rule 3). Async mutex like `history`.
    perf: tokio::sync::Mutex<PerfWriter>,
    /// Reads the same two stores back for `GET /api/v1/history/*`. Holds paths
    /// only and takes none of the writers' locks, so serving a 90-day chart
    /// never stalls a flush.
    history_reader: HistoryReader,
    /// Master switch for the history writers and the perf series. Live-settable
    /// through `POST /api/v1/config` ([`Self::set_history_enabled`]); a disabled
    /// history simply drops each flush, mirroring `query_log.enabled`.
    history_enabled: AtomicBool,
    /// Retention shared with both history writers (one atomic, two readers), so
    /// [`Self::set_history_retention_days`] moves the next prune's cut-off for
    /// both without reconstructing either (hard rule 3: atomic swap).
    history_retention_days: Arc<AtomicU32>,
}

impl Stats {
    pub fn new(
        stats_config: &StatsConfig,
        query_log_config: &QueryLogConfig,
        history_config: &HistoryConfig,
        data_dir: PathBuf,
    ) -> Self {
        let history_retention_days = Arc::new(AtomicU32::new(history_config.retention_days));
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
            history: tokio::sync::Mutex::new(RollupWriter::new(
                data_dir.join("history").join("rollups"),
                Arc::clone(&history_retention_days),
            )),
            perf: tokio::sync::Mutex::new(PerfWriter::new(
                data_dir.join("history").join("perf"),
                Arc::clone(&history_retention_days),
            )),
            history_reader: HistoryReader::new(
                data_dir.join("history").join("rollups"),
                data_dir.join("history").join("perf"),
            ),
            history_enabled: AtomicBool::new(history_config.enabled),
            history_retention_days,
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
        if let Err(err) = self.history.lock().await.boot().await {
            tracing::warn!(error = %err, "failed to initialize history rollup writer");
        }
        if let Err(err) = self.perf.lock().await.boot().await {
            tracing::warn!(error = %err, "failed to initialize perf sample writer");
        }
    }

    /// Records one completed query. The sole write path — called by the
    /// binary's event fan-out task (the single consumer of the pipeline's
    /// `QueryEvent` channel) and directly in tests.
    pub fn record(&self, event: QueryEvent) {
        let at = event.query.timestamp;
        let blocked = matches!(event.verdict, fah_model::Verdict::Block(_));

        self.aggregates.lock().unwrap().record(
            &event.query.domain,
            &event.query.qtype,
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

    /// This crate's contribution to the memory breakdown (p2-07).
    ///
    /// Takes the three sync locks in turn — never the async `segment`,
    /// `history` or `perf` writers, so a poll can never contend with a flush
    /// doing I/O. Those writers hold only paths and small buffers; their real
    /// weight is on `/data`, bounded by `retention_max_mb`, and disk is not
    /// what this accounts for.
    ///
    /// Called on the 10 s telemetry poll, never on the query path. See
    /// [`crate::heap`] for which structures are walked and which track a
    /// running total, and why.
    pub fn heap(&self) -> StatsHeap {
        StatsHeap {
            aggregates: self
                .aggregates
                .lock()
                .expect("aggregates mutex poisoned")
                .heap_bytes() as u64,
            clients: self
                .clients
                .lock()
                .expect("clients mutex poisoned")
                .heap_bytes() as u64,
            ring: self.ring.lock().expect("ring mutex poisoned").heap_bytes() as u64,
            pending_log: {
                let pending = self.pending_log.lock().expect("pending mutex poisoned");
                (pending.capacity() * std::mem::size_of::<crate::query_log::QueryLogEntry>()
                    + pending
                        .iter()
                        .map(crate::query_log::entry_string_bytes)
                        .sum::<usize>()) as u64
            },
        }
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

    /// Captures completed-hour and completed-day aggregates into `/data/history`
    /// on a timer — reuses the snapshot cadence (default 300 s), which catches
    /// every hour boundary well before its 24h ring slot is overwritten.
    pub fn spawn_history_scheduler(self: &Arc<Self>) -> JoinHandle<()> {
        let stats = Arc::clone(self);
        let interval = self.snapshot_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                stats.flush_history(SystemTime::now()).await;
            }
        })
    }

    /// One history tick: append any newly-completed hours, flush a day's top-N
    /// when a day rolls over, then prune. All work is off the per-query path —
    /// a cheap snapshot is taken under the aggregate/client locks (no `.await`
    /// held across them), then handed to the writer for I/O.
    pub async fn flush_history(&self, now: SystemTime) {
        if !self.history_enabled.load(Ordering::Relaxed) {
            return;
        }
        let completed = self.aggregates.lock().unwrap().completed_hour_rollups(now);
        let mut history = self.history.lock().await;
        if let Err(err) = history.append_hours(&completed).await {
            tracing::warn!(error = %err, "failed to append history rollups");
        }
        if let Some(day) = history.day_due(now) {
            let top = self.daily_top_n(day, now);
            if let Err(err) = history.append_daily_top(&top).await {
                tracing::warn!(error = %err, "failed to write daily top-N rollup");
            }
        }
        if let Err(err) = history.maybe_prune(now).await {
            tracing::warn!(error = %err, "failed to prune history rollups");
        }
    }

    /// Persists one perf sample built by the binary's sampler, then prunes on
    /// the fallback cadence. Append-only and off the per-query path (hard rule
    /// 3); the sampler owns the interval, so `Stats` just writes what it's
    /// handed. Retention is measured against the sample's own capture time
    /// (`ts`) rather than wall-clock `now` — self-consistent with the day-file
    /// the sample lands in, and the two are the same instant in production.
    pub async fn persist_perf_sample(&self, sample: PerfSample) {
        if !self.history_enabled.load(Ordering::Relaxed) {
            return;
        }
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(sample.ts);
        let mut perf = self.perf.lock().await;
        if let Err(err) = perf.append(&sample).await {
            tracing::warn!(error = %err, "failed to append perf sample");
        }
        if let Err(err) = perf.maybe_prune(now).await {
            tracing::warn!(error = %err, "failed to prune perf samples");
        }
    }

    /// Aggregate history within `range` (`GET /api/v1/history/summary`),
    /// bounded to `max_points` — see [`HistorySeries::stride`] for whether the
    /// series was decimated to fit.
    ///
    /// **Blocking.** This walks `/data/history/rollups` with `std::fs`; the
    /// caller runs it on a blocking thread (`tokio::task::spawn_blocking`), one
    /// hop for the whole multi-file scan rather than one per file, which is what
    /// `tokio::fs` costs internally. Nothing here is on the per-query path.
    pub fn history_summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> io::Result<HistorySeries> {
        self.history_reader.summary(range, resolution, max_points)
    }

    /// The perf sample series within `range` (`GET /api/v1/history/perf`),
    /// bounded to `max_points`. Blocking, like [`Self::history_summary`].
    pub fn history_perf(&self, range: HistoryRange, max_points: usize) -> io::Result<PerfSeries> {
        self.history_reader.perf(range, max_points)
    }

    /// Top-N over `range`, merged from the daily top-N files
    /// (`GET /api/v1/history/top`). An approximation — each day-file already
    /// holds only that day's top-N. Blocking, like [`Self::history_summary`].
    pub fn history_top(
        &self,
        range: HistoryRange,
        kind: TopKind,
        limit: usize,
    ) -> io::Result<TopItems> {
        self.history_reader.top(range, kind, limit)
    }

    /// Live-applies `history.retention_days` from `POST /api/v1/config`. The
    /// atomic is shared with both writers, so the next prune (rollups and perf)
    /// keeps the new window — no restart, nothing reconstructed (hard rule 3).
    pub fn set_history_retention_days(&self, days: u32) {
        self.history_retention_days.store(days, Ordering::Relaxed);
    }

    /// Live-applies `history.enabled`. When `false`, [`Self::flush_history`] and
    /// [`Self::persist_perf_sample`] become no-ops on their next tick.
    pub fn set_history_enabled(&self, enabled: bool) {
        self.history_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the history writers and perf series are currently on — read by
    /// the binary's sampler so a disabled history skips even building a sample.
    pub fn history_enabled(&self) -> bool {
        self.history_enabled.load(Ordering::Relaxed)
    }

    /// Builds a completed day's top-N from the rolling-24h aggregates and client
    /// registry. The window is the last 24h at flush time (≈ the completed day,
    /// since the flush fires shortly after midnight); an approximation, like
    /// the space-saving top-N it reads.
    fn daily_top_n(&self, day_epoch: u64, now: SystemTime) -> DailyTopN {
        let aggregates = self.aggregates.lock().unwrap();
        let clients = self.clients.lock().unwrap();
        DailyTopN {
            day_epoch,
            top_blocked: to_domain_hits(aggregates.top_blocked(DEFAULT_TOP_N, now)),
            top_queried: to_domain_hits(aggregates.top_queried(DEFAULT_TOP_N, now)),
            top_clients: clients
                .top_by_activity(now, DEFAULT_TOP_N)
                .into_iter()
                .map(|c| ClientHits {
                    ip: c.ip,
                    name: c.name,
                    count: c.queries_24h,
                })
                .collect(),
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

fn to_domain_hits(top: Vec<(std::sync::Arc<str>, u64)>) -> Vec<DomainHits> {
    top.into_iter()
        .map(|(domain, count)| DomainHits {
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

    fn event_at(
        domain: &str,
        qtype: QueryType,
        client: IpAddr,
        verdict: Verdict,
        cache_hit: bool,
        at: SystemTime,
    ) -> QueryEvent {
        QueryEvent::new(
            Query::new(domain, qtype, client, at),
            verdict,
            std::time::Duration::from_micros(100),
            cache_hit,
            true,
            false,
        )
    }

    fn block(domain: &str) -> Verdict {
        Verdict::Block(DecisiveRule::new("oisd-basic", format!("||{domain}^")))
    }

    #[tokio::test]
    async fn record_updates_aggregates_registry_and_ring() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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
    async fn kill_and_restart_recovers_stats_from_the_last_snapshot() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        {
            let stats = Stats::new(
                &stats_config,
                &query_log_config,
                &HistoryConfig::default(),
                dir.path().to_path_buf(),
            );
            stats.boot().await;
            stats.record(event(
                "ads.example.com",
                client,
                Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
            ));
            stats.save_snapshot().await; // simulates the periodic snapshot beating the kill -9
        }

        let restarted = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        restarted.boot().await;

        let snapshot = restarted.snapshot(SystemTime::now());
        assert_eq!(snapshot.queries_total, 1);
        assert_eq!(snapshot.blocked_total, 1);
    }

    /// The instrument has to *move* when memory moves, or a flat reading
    /// proves nothing (p2-07).
    #[tokio::test]
    async fn heap_accounting_tracks_recorded_traffic_and_stays_bounded() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let empty = stats.heap();
        assert!(
            empty.ring > 0,
            "the ring allocates its buffer up front — capacity is what occupies RAM, not fill"
        );

        for i in 0..500u32 {
            let client = IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8));
            stats.record(event(
                &format!("domain-{i}.example.com"),
                client,
                Verdict::Pass,
            ));
        }
        let loaded = stats.heap();

        assert!(
            loaded.aggregates > empty.aggregates,
            "tracked domains must show up in the aggregates: {} -> {}",
            empty.aggregates,
            loaded.aggregates
        );
        assert!(
            loaded.clients > empty.clients,
            "distinct clients must show up in the registry: {} -> {}",
            empty.clients,
            loaded.clients
        );
        assert!(loaded.total() > empty.total());

        // Bounded: compare two *saturated* states, not empty against full.
        // Growing from 500 clients toward the 4,096 cap is legitimate fill;
        // what hard rule 4 promises is that once the caps are reached, more
        // distinct keys cost nothing further.
        for i in 500..40_000u32 {
            let client = IpAddr::V4(Ipv4Addr::new(10, 1, (i / 256) as u8, (i % 256) as u8));
            stats.record(event(
                &format!("domain-{i}.example.com"),
                client,
                Verdict::Pass,
            ));
        }
        let saturated = stats.heap();

        for i in 40_000..80_000u32 {
            let client = IpAddr::V4(Ipv4Addr::new(11, 1, (i / 256) as u8, (i % 256) as u8));
            stats.record(event(
                &format!("other-{i}.example.net"),
                client,
                Verdict::Pass,
            ));
        }
        let still_saturated = stats.heap();

        let growth = still_saturated.total() as f64 / saturated.total() as f64;
        assert!(
            growth < 1.25,
            "past the caps, 40,000 more distinct clients and domains must cost              almost nothing (hard rule 4); grew {growth:.2}x ({} -> {} bytes)",
            saturated.total(),
            still_saturated.total()
        );
    }

    #[tokio::test]
    async fn flush_persists_pending_entries_to_a_segment() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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

        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
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
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));

        let page = stats.query_log(&QueryLogFilter::default(), 10, None);
        assert!(page.items.is_empty());
        assert_eq!(stats.snapshot(SystemTime::now()).queries_total, 1);
    }

    fn at_hour(hour: u64) -> SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(hour * 3600)
    }

    #[tokio::test]
    async fn flush_history_writes_the_completed_hour_with_per_type_counts() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let hour = 471_000u64; // 2023-09-… — an arbitrary completed hour
                               // Hour `hour`: 2× A (one blocked, one a cache hit) + 1× AAAA.
        stats.record(event_at(
            "ads.example.com",
            QueryType::A,
            client,
            block("ads.example.com"),
            false,
            at_hour(hour),
        ));
        stats.record(event_at(
            "example.com",
            QueryType::A,
            client,
            Verdict::Pass,
            true,
            at_hour(hour),
        ));
        stats.record(event_at(
            "example.com",
            QueryType::Aaaa,
            client,
            Verdict::Pass,
            false,
            at_hour(hour),
        ));
        // A query in the next hour makes `hour` a completed hour.
        stats.record(event_at(
            "later.com",
            QueryType::A,
            client,
            Verdict::Pass,
            false,
            at_hour(hour + 1),
        ));

        stats.flush_history(at_hour(hour + 1)).await;

        // day-of(hour) = hour/24 = 19625 = 2023-09-25.
        let path = dir
            .path()
            .join("history")
            .join("rollups")
            .join("rollup-2023-09-25.jsonl");
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<fah_model::HourRollup> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 1, "exactly one completed hour");
        let rollup = &lines[0];
        assert_eq!(rollup.hour_epoch, hour);
        assert_eq!(rollup.queries, 3);
        assert_eq!(rollup.blocked, 1);
        assert_eq!(rollup.cache_hits, 1);
        assert_eq!(rollup.per_type.get("A"), Some(&2));
        assert_eq!(rollup.per_type.get("AAAA"), Some(&1));
    }

    #[tokio::test]
    async fn flush_history_is_idempotent_across_ticks() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let hour = 471_000u64;
        stats.record(event_at(
            "a.com",
            QueryType::A,
            client,
            Verdict::Pass,
            false,
            at_hour(hour),
        ));
        stats.record(event_at(
            "b.com",
            QueryType::A,
            client,
            Verdict::Pass,
            false,
            at_hour(hour + 1),
        ));

        // Two ticks over the same completed hour must not double-write.
        stats.flush_history(at_hour(hour + 1)).await;
        stats.flush_history(at_hour(hour + 1)).await;

        let path = dir
            .path()
            .join("history")
            .join("rollups")
            .join("rollup-2023-09-25.jsonl");
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(text.lines().count(), 1);
    }

    #[tokio::test]
    async fn persist_perf_sample_appends_a_line_to_the_day_file() {
        use fah_model::{CacheStatsSample, LatencySummary, PerfSample};

        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        // day 19625 = 2023-09-25.
        let ts = 19625 * 24 * 3600 + 60;
        stats
            .persist_perf_sample(PerfSample {
                ts,
                rss_bytes: 55_000_000,
                qps: 12.0,
                queries_delta: 720,
                blocked_delta: 200,
                allowed_delta: 4,
                cache: CacheStatsSample {
                    entries: 1000,
                    capacity: 16_384,
                    fresh: 900,
                    stale: 80,
                    expired: 20,
                    hits: 5000,
                    misses: 1200,
                    evictions: 34,
                    bytes: 21_000_000,
                    max_bytes: 67_108_864,
                },
                latency: LatencySummary {
                    block_p50: 0.0001,
                    block_p99: 0.0005,
                    cache_hit_p50: 0.0001,
                    cache_hit_p99: 0.00025,
                    forward_p50: 0.005,
                    forward_p99: 0.05,
                },
                upstreams: vec![],
            })
            .await;

        let path = dir
            .path()
            .join("history")
            .join("perf")
            .join("perf-2023-09-25.jsonl");
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<PerfSample> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].qps, 12.0);
        assert_eq!(lines[0].rss_bytes, 55_000_000);
    }

    #[tokio::test]
    async fn flush_history_writes_a_daily_top_n_when_a_day_completes() {
        let (stats_config, query_log_config) = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        // day 19625 = 2023-09-25; last hour of that day, then into the next day.
        let last_hour = 19625 * 24 + 23;
        stats.record(event_at(
            "ads.example.com",
            QueryType::A,
            client,
            block("ads.example.com"),
            false,
            at_hour(last_hour),
        ));

        // First tick (still day 19625): baselines the day, no top-N yet.
        stats.flush_history(at_hour(last_hour)).await;
        assert!(!dir
            .path()
            .join("history")
            .join("rollups")
            .join("top-2023-09-25.json")
            .exists());

        // Next tick after midnight: day 19625 completed → its top-N is flushed.
        stats.flush_history(at_hour(last_hour + 1)).await;
        let path = dir
            .path()
            .join("history")
            .join("rollups")
            .join("top-2023-09-25.json");
        let top: fah_model::DailyTopN =
            serde_json::from_str(&tokio::fs::read_to_string(&path).await.unwrap()).unwrap();
        assert_eq!(top.day_epoch, 19625);
        assert_eq!(top.top_blocked[0].domain, "ads.example.com");
        assert_eq!(top.top_clients[0].ip, client);
    }

    #[tokio::test]
    async fn history_enabled_toggles_the_flush_live() {
        let (stats_config, query_log_config) = config();
        let history_config = HistoryConfig {
            enabled: false,
            ..HistoryConfig::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &query_log_config,
            &history_config,
            dir.path().to_path_buf(),
        );
        stats.boot().await;
        assert!(!stats.history_enabled());

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let hour = 471_000u64;
        stats.record(event_at(
            "a.com",
            QueryType::A,
            client,
            Verdict::Pass,
            false,
            at_hour(hour),
        ));
        stats.record(event_at(
            "b.com",
            QueryType::A,
            client,
            Verdict::Pass,
            false,
            at_hour(hour + 1),
        ));

        let rollups = dir.path().join("history").join("rollups");
        // Disabled: the completed hour is not persisted.
        stats.flush_history(at_hour(hour + 1)).await;
        let mut read = tokio::fs::read_dir(&rollups).await.unwrap();
        assert!(
            read.next_entry().await.unwrap().is_none(),
            "a disabled history writes nothing"
        );

        // A POST /api/v1/config would call this; the next flush persists.
        stats.set_history_enabled(true);
        assert!(stats.history_enabled());
        stats.flush_history(at_hour(hour + 1)).await;
        let mut read = tokio::fs::read_dir(&rollups).await.unwrap();
        assert!(
            read.next_entry().await.unwrap().is_some(),
            "re-enabling takes effect on the next flush, no restart"
        );
    }
}
