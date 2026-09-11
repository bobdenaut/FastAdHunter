//! [`Stats`]: owns the aggregates and the client registry; the internal
//! handle `fah-api` (p1-09) will hold to serve API.md's stats/query/client
//! endpoints. Consumes `QueryEvent`s from the bounded channel `fah-dns`
//! emits into — created and wired in `fastadhunter` (siblings never import
//! each other, ARCHITECTURE.md §Dependency Layering).

use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fah_config::{HistoryConfig, StatsConfig};
use fah_model::{
    ClientHits, DailyTopN, DomainHits, HistoryRange, HistoryResolution, HistorySeries, PerfSample,
    PerfSeries, QueryEvent, TopItems, TopKind,
};
use tokio::task::JoinHandle;

use crate::aggregates::Aggregates;
use crate::client_registry::{ClientRegistry, ClientView, InterceptedOutcome};
use crate::dto::{ClientCount, DomainCount, StatsSnapshot};
use crate::history::{HistoryReader, PerfWriter, RollupWriter};
use crate::snapshot::{self, SnapshotData};
use fah_model::StatsHeap;

const DEFAULT_TOP_N: usize = 10;

pub struct Stats {
    data_dir: PathBuf,
    snapshot_interval: Duration,
    aggregates: Mutex<Aggregates>,
    clients: Mutex<ClientRegistry>,
    /// Long-term hourly/daily aggregate rollups on `/data/history` — written by
    /// the history scheduler, never the per-query path (hard rule 3). Async
    /// mutex because rotate/prune do I/O, mirroring `fah_rules::ListManager`'s
    /// `compile_lock` (a lock guarding a section that awaits).
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
    /// history simply drops each flush.
    history_enabled: AtomicBool,
    /// Retention shared with both history writers (one atomic, two readers), so
    /// [`Self::set_history_retention_days`] moves the next prune's cut-off for
    /// both without reconstructing either (hard rule 3: atomic swap).
    history_retention_days: Arc<AtomicU32>,
}

impl Stats {
    pub fn new(
        stats_config: &StatsConfig,
        history_config: &HistoryConfig,
        data_dir: PathBuf,
    ) -> Self {
        let history_retention_days = Arc::new(AtomicU32::new(history_config.retention_days));
        Self {
            snapshot_interval: Duration::from_secs(u64::from(
                stats_config.snapshot_interval_seconds.max(1),
            )),
            aggregates: Mutex::new(Aggregates::default()),
            clients: Mutex::new(ClientRegistry::default()),
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

    /// Loads the persisted snapshot (if any) — no network, mirrors
    /// `ListManager::boot`'s cache-first convention.
    pub async fn boot(&self) {
        if let Some(data) = snapshot::load(&self.data_dir).await {
            *self.aggregates.lock().unwrap() = data.aggregates;
            *self.clients.lock().unwrap() = data.clients;
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

        {
            let mut aggregates = self.aggregates.lock().unwrap();
            aggregates.record(
                &event.query.domain,
                &event.query.qtype,
                &event.verdict,
                event.cache_hit,
                at,
            );
            aggregates.record_policy(event.policy.as_deref(), blocked, at);
        }
        self.clients
            .lock()
            .unwrap()
            .record(event.query.client_ip, at, blocked, event.cache_hit);
    }

    /// Records one completed HTTP request (p2-04). The proxy's counterpart of
    /// [`Stats::record`], on the same fan-out task.
    ///
    /// **Deliberately not fed into the domain aggregates.** `/stats`'s top
    /// domains have meant "names asked for" since p1-07; one page load is a
    /// single DNS question and then dozens of HTTP requests to the same host,
    /// so folding requests in would not enrich that table, it would inflate it
    /// by whatever a site's asset count happens to be. Per-*client* activity is
    /// fed in, because "this client made N requests, M blocked" reads the same
    /// way whichever pipeline refused them — and p2-06 needs exactly that
    /// number per client.
    pub fn record_https(&self, event: fah_model::RequestEvent) {
        let outcome = if event.status == fah_model::CLIENT_CERT_REJECTED {
            Some(InterceptedOutcome::Rejected)
        } else if !event.request.method.is_empty() {
            Some(InterceptedOutcome::Completed)
        } else {
            None
        };
        let client_ip = event.request.client_ip;
        let at = event.request.timestamp;
        self.record_http(event);
        if let Some(outcome) = outcome {
            self.clients
                .lock()
                .unwrap()
                .record_intercepted(client_ip, at, outcome);
        }
    }

    pub fn record_http(&self, event: fah_model::RequestEvent) {
        let at = event.request.timestamp;
        let blocked = matches!(event.verdict, fah_model::Verdict::Block(_));
        let client_ip = event.request.client_ip;

        // Per-policy counts do take requests: "this policy blocked N" reads the
        // same whichever pipeline refused them.
        self.aggregates
            .lock()
            .unwrap()
            .record_policy(event.policy.as_deref(), blocked, at);

        // `cache_hit: false` — an HTTP request has no cache to hit; the DNS
        // cache answered a different question earlier.
        self.clients
            .lock()
            .unwrap()
            .record(client_ip, at, blocked, false);
    }

    /// This crate's contribution to the memory breakdown (p2-07).
    ///
    /// Takes the two sync locks in turn — never the async `history` or `perf`
    /// writers, so a poll can never contend with a flush doing I/O. Those
    /// writers hold only paths and small buffers; their real weight is on
    /// `/data`, bounded by `history.retention_days`, and disk is not what this
    /// accounts for.
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
    pub fn history_perf(
        &self,
        range: HistoryRange,
        max_points: usize,
        include_upstreams: bool,
    ) -> io::Result<PerfSeries> {
        self.history_reader
            .perf(range, max_points, include_upstreams)
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
            policies: aggregates.policies(now),
        }
    }

    /// Every client that currently carries a name — what the binary feeds
    /// `PolicyState::refresh` so name assignments resolve to addresses.
    pub fn named_clients(&self) -> Vec<(IpAddr, std::sync::Arc<str>)> {
        self.clients.lock().unwrap().named()
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

    fn config() -> StatsConfig {
        StatsConfig {
            snapshot_interval_seconds: 300,
        }
    }

    fn event(domain: &str, client: IpAddr, verdict: Verdict) -> QueryEvent {
        fah_model::QueryEvent::new(
            Query::new(domain, QueryType::A, client, SystemTime::now()),
            verdict,
            std::time::Duration::from_micros(100),
            false,
            true,
            None,
            fah_model::ClientTransport::Udp,
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
        fah_model::QueryEvent::new(
            Query::new(domain, qtype, client, at),
            verdict,
            std::time::Duration::from_micros(100),
            cache_hit,
            true,
            None,
            fah_model::ClientTransport::Udp,
        )
    }

    fn block(domain: &str) -> Verdict {
        Verdict::Block(DecisiveRule::new("oisd-basic", format!("||{domain}^")))
    }

    #[tokio::test]
    async fn record_updates_aggregates_and_the_client_registry() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
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
    }

    /// Per-policy counters, and the `default` row that covers events no policy
    /// was assigned for (p2-06).
    #[tokio::test]
    async fn per_policy_counters_split_by_policy() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let kid = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));
        let other = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 51));
        stats.record(
            event("games.example.com", kid, block("games.example.com"))
                .under_policy(Some(std::sync::Arc::from("kids"))),
        );
        stats.record(
            event("example.com", kid, Verdict::Pass)
                .under_policy(Some(std::sync::Arc::from("kids"))),
        );
        // No policy: the default decided it.
        stats.record(event("example.org", other, Verdict::Pass));

        let snapshot = stats.snapshot(SystemTime::now());
        let counts: Vec<(&str, u64, u64)> = snapshot
            .policies
            .iter()
            .map(|p| (&*p.policy, p.queries, p.blocked))
            .collect();
        assert_eq!(counts, vec![("kids", 2, 1), ("default", 1, 0)]);
    }

    /// An HTTP request counts toward its policy but never toward the domain
    /// tables — the p2-04 split, kept.
    #[tokio::test]
    async fn https_events_classify_into_completed_and_rejected_per_client() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;
        let phone = IpAddr::V4(Ipv4Addr::new(192, 168, 10, 11));
        let https = |method: &str, status: u16| {
            fah_model::RequestEvent::new(
                fah_model::Request {
                    host: "login5.spotify.com".to_string(),
                    path: String::new(),
                    method: method.to_string(),
                    resource_type: fah_model::ResourceType::Other,
                    client_ip: phone,
                    timestamp: SystemTime::now(),
                },
                Verdict::Pass,
                std::time::Duration::from_micros(100),
                status,
                0,
            )
        };

        stats.record_https(https("", fah_model::CLIENT_CERT_REJECTED));
        stats.record_https(https("", fah_model::CLIENT_CERT_REJECTED));
        stats.record_https(https("GET", 200));
        stats.record_https(https("GET", 421));
        stats.record_https(https("", 0));
        stats.record_https(https("", fah_model::UPSTREAM_CERT_FAILURE));

        let clients = stats.clients(SystemTime::now());
        let view = clients.iter().find(|view| view.ip == phone).unwrap();
        assert_eq!(
            view.intercepted.completed, 2,
            "a request inside the session"
        );
        assert_eq!(view.intercepted.rejected, 2, "one per 525");
        assert!(view.intercepted.last_completed.is_some());
        assert!(view.intercepted.last_rejected.is_some());
    }

    #[tokio::test]
    async fn http_requests_count_toward_a_policy_but_not_the_domain_tables() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        stats.record_http(
            fah_model::RequestEvent::new(
                fah_model::Request {
                    host: "ads.example.com".to_string(),
                    path: "/pixel.gif".to_string(),
                    method: "GET".to_string(),
                    resource_type: fah_model::ResourceType::Image,
                    client_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
                    timestamp: SystemTime::now(),
                },
                block("ads.example.com"),
                std::time::Duration::from_micros(100),
                200,
                0,
            )
            .under_policy(Some(std::sync::Arc::from("kids"))),
        );

        let snapshot = stats.snapshot(SystemTime::now());
        assert_eq!(snapshot.queries_total, 0, "domain tables stay DNS-only");
        assert_eq!(
            snapshot.policies,
            vec![crate::PolicyCount {
                policy: std::sync::Arc::from("kids"),
                queries: 1,
                blocked: 1,
            }]
        );
    }

    /// The name is resolved on demand, not captured per event — the websocket
    /// decorates each event by calling this at publish time.
    #[tokio::test]
    async fn a_client_name_is_readable_once_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        stats.record(event("example.com", client, Verdict::Pass));
        assert_eq!(stats.client_name(client), None);

        stats.set_client_name(client, Some("liviu-phone".to_string()));
        assert_eq!(stats.client_name(client).as_deref(), Some("liviu-phone"));
    }

    #[tokio::test]
    async fn kill_and_restart_recovers_stats_from_the_last_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

        {
            let stats = Stats::new(
                &config(),
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
            &config(),
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
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        let empty = stats.heap();

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

    /// Every path and size under `root`, recursively — enough to catch a file
    /// appearing or growing.
    fn tree(root: &std::path::Path) -> Vec<(PathBuf, u64)> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(root) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => found.extend(tree(&path)),
                Ok(meta) => found.push((path, meta.len())),
                Err(_) => {}
            }
        }
        found.sort();
        found
    }

    /// Recording writes nothing to `/data` on the query path — the only files
    /// this crate creates are the snapshot and the history series, both on
    /// their own schedulers.
    #[tokio::test]
    async fn recording_touches_no_file_on_the_query_path() {
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &config(),
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;
        // `boot` creates the history directories; what must not change is
        // anything below them once queries start arriving.
        let after_boot = tree(dir.path());

        let client = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        for _ in 0..1_000 {
            stats.record(event("example.com", client, Verdict::Pass));
        }

        assert_eq!(
            tree(dir.path()),
            after_boot,
            "recording must not create or grow a file under /data"
        );
        assert_eq!(stats.snapshot(SystemTime::now()).queries_total, 1_000);
    }

    fn at_hour(hour: u64) -> SystemTime {
        std::time::UNIX_EPOCH + Duration::from_secs(hour * 3600)
    }

    #[tokio::test]
    async fn flush_history_writes_the_completed_hour_with_per_type_counts() {
        let stats_config = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
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
        let stats_config = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
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

        let stats_config = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
            &HistoryConfig::default(),
            dir.path().to_path_buf(),
        );
        stats.boot().await;

        // day 19625 = 2023-09-25.
        let ts = 19625 * 24 * 3600 + 60;
        stats
            .persist_perf_sample(PerfSample {
                ts,
                answers_delta: Default::default(),
                allocator_committed_bytes: 0,
                list_fetch: Default::default(),
                concurrent_connections: Default::default(),
                rss_bytes: 55_000_000,
                peak_rss: 123_539_456,
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
                memory: fah_model::MemoryComponents::default(),
                minor_page_faults: 0,
                rss_anon_bytes: 0,
                rss_file_bytes: 0,
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
        let stats_config = config();
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(
            &stats_config,
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
        let stats_config = config();
        let history_config = HistoryConfig {
            enabled: false,
            ..HistoryConfig::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let stats = Stats::new(&stats_config, &history_config, dir.path().to_path_buf());
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
