//! End-to-end proof of the observability history (phase 1.5): the real
//! `fah-stats` writers persist rollups, a daily top-N and a perf sample to a
//! tempdir `/data`, and the real `fah-api` server reads them straight back over
//! HTTPS through `GET /api/v1/history/{summary,perf,top}` — then a live
//! `POST /api/v1/config` retention change is shown to move the next prune's
//! cut-off.
//!
//! Why in-process rather than the spawned binary of `e2e.rs`: the rollup writer
//! only captures an hour once the wall clock has left it, and a day's top-N only
//! once the day has completed. The running binary stamps every query with
//! `SystemTime::now()`, so no natural hour or day boundary is crossable inside a
//! test's lifetime. Assembling the real `Stats` behind the real `ApiServer` here
//! lets the test record across a chosen hour/day boundary and flush at the later
//! instant — exactly the "advance the clock past an hour boundary" the writers
//! are built around — while still exercising the whole disk→read→HTTP chain the
//! dashboard will depend on. The per-crate unit tests own the arithmetic; this
//! owns the wiring between them.
//!
//! `fastadhunter` is the L4 binary, the one crate allowed to see both
//! `fah-stats` and `fah-api` (ARCHITECTURE.md §Dependency Layering), so the
//! port adapter below is a faithful stand-in for `src/adapters.rs`'s
//! `StatsAdapter` (which the test, compiling against the binary target, cannot
//! import).

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fah_api::{
    ApiKeyStore, ApiServer, AppStateBuilder, CacheClean, CacheSource, CacheStats, ClientEntry,
    ConfigStore, HistorySource, StatsOverview, StatsSource, TelemetrySource,
};
use fah_config::{Config, RulesConfig};
use fah_model::{
    CacheStatsSample, DecisiveRule, HistoryRange, HistoryResolution, HistorySeries, LatencySummary,
    PerfSample, PerfSeries, Query, QueryEvent, QueryType, TopItems, TopKind, Verdict,
};
use fah_stats::Stats;
use serde_json::{json, Value};

// ── Fixed calendar anchors (UTC) ────────────────────────────────────────
// Day 19625 = 2023-09-25, the day the `fah-stats` reader/writer tests already
// use — so the RFC 3339 window below is known without any date formatting.
const DAY: u64 = 19_625;
const SECS_PER_HOUR: u64 = 3_600;
const SECS_PER_DAY: u64 = 86_400;
const DAY_START: &str = "2023-09-25T00:00:00Z";
const NEXT_DAY_START: &str = "2023-09-26T00:00:00Z";

const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

/// RSS and cache-entry values seeded into the perf sample, asserted back out of
/// `GET /api/v1/history/perf` — non-zero and distinctive so a wrong-field bug
/// can't pass by coincidence.
const SAMPLE_RSS_BYTES: u64 = 57_213_952;
const SAMPLE_CACHE_ENTRIES: u64 = 1_234;
/// Components summing to 30,000,000 B, so the residual the API derives from
/// `SAMPLE_RSS_BYTES` is a distinctive 27,213,952 B.
const SAMPLE_MEMORY: fah_model::MemoryComponents = fah_model::MemoryComponents {
    ruleset: 23_000_000,
    cache: 5_000_000,
    stats: fah_model::StatsHeap {
        aggregates: 1_000_000,
        clients: 500_000,
    },
};
const SAMPLE_MINOR_FAULTS: u64 = 4_211_337;

fn at_hour(hour_epoch: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(hour_epoch * SECS_PER_HOUR)
}

// ─── Populate → read the history over HTTPS ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_is_written_to_disk_and_served_over_http() {
    let harness = Harness::start(90).await;
    let stats = Arc::clone(&harness.stats);

    // Hour 10:00 on 2023-09-25: three queries — a block, a cache-hit pass, and
    // an AAAA pass — so the rollup carries per-type counts and a non-trivial
    // blocked/cache split.
    let ten = DAY * 24 + 10;
    stats.record(event(
        "ads.example.com",
        QueryType::A,
        block("ads.example.com"),
        false,
        at_hour(ten),
    ));
    stats.record(event(
        "cdn.example.com",
        QueryType::A,
        Verdict::Pass,
        true,
        at_hour(ten),
    ));
    stats.record(event(
        "cdn.example.com",
        QueryType::Aaaa,
        Verdict::Pass,
        false,
        at_hour(ten),
    ));
    // A second block late in the day keeps ads.example.com inside the rolling
    // 24h window when the day's top-N is cut at midnight.
    let last = DAY * 24 + 23;
    stats.record(event(
        "ads.example.com",
        QueryType::A,
        block("ads.example.com"),
        false,
        at_hour(last),
    ));

    // Flush at 11:00 — hour 10:00 is now a completed hour and is captured.
    stats.flush_history(at_hour(ten + 1)).await;
    // Cross midnight into 2023-09-26: this completes hour 23:00 (captured into
    // the same day-file) and completes the day, so its top-N is flushed.
    let past_midnight = (DAY + 1) * 24;
    stats.record(event(
        "boundary.example.com",
        QueryType::A,
        Verdict::Pass,
        false,
        at_hour(past_midnight),
    ));
    stats.flush_history(at_hour(past_midnight)).await;

    // A perf sample with a distinctive RSS and cache-entry count.
    stats
        .persist_perf_sample(perf_sample(DAY * SECS_PER_DAY + 10 * SECS_PER_HOUR))
        .await;

    // The rollup day-file must actually exist on disk (the write half of the
    // contract) before we prove the read half.
    let rollup = harness
        .data_dir
        .path()
        .join("history")
        .join("rollups")
        .join("rollup-2023-09-25.jsonl");
    assert!(
        rollup.exists(),
        "a completed hour must produce {}",
        rollup.display()
    );

    // ── GET /history/summary ──
    let summary = harness
        .get_json(&format!(
            "/api/v1/history/summary?from={DAY_START}&to={NEXT_DAY_START}&resolution=hour"
        ))
        .await;
    assert_eq!(summary["resolution"], "hour");
    let items = summary["items"].as_array().expect("summary items");
    let ten_oclock = items
        .iter()
        .find(|item| item["queries"] == 3)
        .expect("the 10:00 rollup (3 queries) must be served back");
    assert_eq!(ten_oclock["blocked"], 1);
    assert_eq!(ten_oclock["cache_hits"], 1);
    assert_eq!(ten_oclock["per_type"]["A"], 2);
    assert_eq!(ten_oclock["per_type"]["AAAA"], 1);
    assert_eq!(
        ten_oclock["ts"], "2023-09-25T10:00:00Z",
        "the hour is echoed in the RFC 3339 spelling the rest of the API uses"
    );

    // ── GET /history/perf ── carries the seeded RSS and cache stats.
    let perf = harness
        .get_json(&format!(
            "/api/v1/history/perf?from={DAY_START}&to={NEXT_DAY_START}"
        ))
        .await;
    let sample = &perf["items"].as_array().expect("perf items")[0];
    assert_eq!(sample["rss_bytes"], SAMPLE_RSS_BYTES);
    assert_eq!(sample["cache"]["entries"], SAMPLE_CACHE_ENTRIES);
    assert_eq!(sample["cache"]["max_bytes"], 67_108_864);
    // The memory breakdown survives the disk round trip, and the residual is
    // derived from this row's own RSS rather than read back from it (p2-07).
    assert_eq!(sample["memory"]["ruleset_bytes"], SAMPLE_MEMORY.ruleset);
    assert_eq!(
        sample["memory"]["stats_clients_bytes"],
        SAMPLE_MEMORY.stats.clients
    );
    assert_eq!(sample["memory"]["accounted_bytes"], 29_500_000);
    assert_eq!(
        sample["memory"]["residual_bytes"],
        SAMPLE_RSS_BYTES - 29_500_000
    );
    assert_eq!(sample["minor_page_faults"], SAMPLE_MINOR_FAULTS);
    assert!(
        sample["memory"].get("rss_bytes").is_none(),
        "the persisted breakdown must not carry a second copy of the RSS"
    );

    // ── GET /history/top ── returns the seeded top-N (ads blocked twice).
    let top = harness
        .get_json(&format!(
            "/api/v1/history/top?from={DAY_START}&to={NEXT_DAY_START}&kind=blocked"
        ))
        .await;
    assert_eq!(top["kind"], "blocked");
    let top_items = top["items"].as_array().expect("top items");
    assert_eq!(top_items[0]["domain"], "ads.example.com");
    assert_eq!(
        top_items[0]["count"], 2,
        "both blocks of the day are merged into the daily top-N"
    );
}

// ─── Retention flipped live via POST /config moves the prune cut-off ─────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_config_retention_change_prunes_on_the_next_flush() {
    // Fixed calendar days so the API windows are known strings, and the writer
    // (not the test) produces every filename:
    //   old    = 2023-09-25 (day 19625) — 40 days before `now`
    //   recent = 2023-10-25 (day 19655) — 10 days before `now`
    //   now    = 2023-11-04 (day 19665)
    let (old_day, recent_day, now_day) = (19_625u64, 19_655u64, 19_665u64);
    const OLD: (&str, &str) = ("2023-09-25T00:00:00Z", "2023-09-26T00:00:00Z");
    const RECENT: (&str, &str) = ("2023-10-25T00:00:00Z", "2023-10-26T00:00:00Z");

    // A prior instance rolled both days over, days ago — written through the
    // real writer onto the shared `/data`.
    let data_dir = tempfile::tempdir().unwrap();
    write_rollup_day(data_dir.path(), old_day).await;
    write_rollup_day(data_dir.path(), recent_day).await;

    // Boot the served instance over those files, wide (90 days). At 90 days
    // neither would be pruned, so deleting the 40-day one can only come from
    // tightening retention below 40 — the change the POST makes.
    let harness = Harness::start_with_data(data_dir, 90).await;

    // Both days read back before any prune — the files are really there.
    assert!(
        !harness.summary_is_empty(OLD).await,
        "the 40-day-old day must be present before retention is tightened"
    );
    assert!(!harness.summary_is_empty(RECENT).await);

    // Flip retention 90 → 30 through the real config endpoint.
    let response = harness
        .post(
            "/api/v1/config",
            json!({ "history": { "retention_days": 30 } }),
        )
        .await;
    assert!(
        response.status().is_success(),
        "POST /config retention change: {}",
        response.status()
    );
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["applied"], true, "retention_days is runtime-applied");

    // The first flush after boot always prunes (RollupWriter::maybe_prune). It
    // now sees retention = 30, so the 40-day day-file goes and the 10-day stays.
    harness
        .stats
        .flush_history(at_hour(now_day * 24 + 12))
        .await;

    assert!(
        harness.summary_is_empty(OLD).await,
        "the 40-day-old day must be pruned once retention is tightened to 30"
    );
    assert!(
        !harness.summary_is_empty(RECENT).await,
        "the 10-day-old day is inside the 30-day window and must survive"
    );
}

// ─── Fixtures ───────────────────────────────────────────────────────────

fn event(
    domain: &str,
    qtype: QueryType,
    verdict: Verdict,
    cache_hit: bool,
    at: SystemTime,
) -> QueryEvent {
    QueryEvent::new(
        Query::new(domain, qtype, CLIENT, at),
        verdict,
        Duration::from_micros(100),
        cache_hit,
        true,
        None,
    )
}

fn block(domain: &str) -> Verdict {
    Verdict::Block(DecisiveRule::new("oisd-basic", format!("||{domain}^")))
}

fn perf_sample(ts: u64) -> PerfSample {
    PerfSample {
        ts,
        rss_bytes: SAMPLE_RSS_BYTES,
        qps: 12.0,
        queries_delta: 720,
        blocked_delta: 200,
        allowed_delta: 4,
        cache: CacheStatsSample {
            entries: SAMPLE_CACHE_ENTRIES,
            capacity: 16_384,
            fresh: 1_100,
            stale: 100,
            expired: 34,
            hits: 5_000,
            misses: 1_200,
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
        memory: SAMPLE_MEMORY,
        minor_page_faults: SAMPLE_MINOR_FAULTS,
        upstreams: vec![],
    }
}

/// Lays down a real rollup day-file for `day_epoch` on the shared `/data`, the
/// way a long-running instance that rolled that day over days ago would have:
/// a throwaway `Stats` records one query in the day and flushes the hour, so the
/// **filename and JSONL are produced by the real writer** — the test never
/// restates fah-stats' calendar-to-filename scheme or its row format, so nothing
/// here can drift out of step with the writer under test. This is the only way
/// to place a file older than a test's own lifetime.
async fn write_rollup_day(data_dir: &std::path::Path, day_epoch: u64) {
    let mut config = Config::default();
    config.history.enabled = true;
    // Wide window so this writer never prunes the fresh file it is laying down.
    config.history.retention_days = 3_650;
    let stats = Stats::new(&config.stats, &config.history, data_dir.to_path_buf());
    stats.boot().await;
    let hour = day_epoch * 24;
    stats.record(event(
        "seed.example.com",
        QueryType::A,
        Verdict::Pass,
        false,
        at_hour(hour),
    ));
    // Flushing one hour later completes `hour`, which the writer captures.
    stats.flush_history(at_hour(hour + 1)).await;
}

// ─── Harness: the real Stats behind the real ApiServer ──────────────────

struct Harness {
    server: ApiServer,
    client: reqwest::Client,
    key: String,
    base: String,
    stats: Arc<Stats>,
    data_dir: tempfile::TempDir,
    _config_dir: tempfile::TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.shutdown();
    }
}

impl Harness {
    /// Boots a `Stats` with history enabled at `retention_days` on a fresh,
    /// empty `/data`, wraps it in the port adapter, and serves the real API over
    /// HTTPS with a self-signed cert.
    async fn start(retention_days: u32) -> Self {
        Self::start_with_data(tempfile::tempdir().unwrap(), retention_days).await
    }

    /// Same, but on a caller-provided `/data` — used to boot *over* day-files an
    /// earlier instance already wrote, the way a restart picks history up from
    /// disk. Boot reads the writers' cursors from those files; it does not prune.
    async fn start_with_data(data_dir: tempfile::TempDir, retention_days: u32) -> Self {
        let config_dir = tempfile::tempdir().unwrap();

        let mut config = Config::default();
        config.history.enabled = true;
        config.history.retention_days = retention_days;
        // No lists: keep the ListManager offline (no refresh reaches the net).
        config.rules = RulesConfig {
            refresh_hours_default: 24,
            lists: vec![],
        };
        let config_path = config_dir.path().join("fastadhunter.toml");
        config.save(&config_path).unwrap();

        let stats = Arc::new(Stats::new(
            &config.stats,
            &config.history,
            data_dir.path().to_path_buf(),
        ));
        stats.boot().await;

        let rules = Arc::new(
            fah_rules::ListManager::new(&config.rules, data_dir.path().to_path_buf()).unwrap(),
        );
        rules.boot().await;

        let (keys, generated) = ApiKeyStore::load_or_create(config_dir.path()).unwrap();
        let key = generated.expect("first boot generates a key");
        let tls = Some(fah_api::load_or_generate_tls(config_dir.path()).unwrap());

        let port = Arc::new(StatsPort(Arc::clone(&stats)));
        let state = AppStateBuilder {
            rules,
            policies: Arc::new(fah_rules::PolicyState::default()),
            stats: Arc::clone(&port) as Arc<dyn StatsSource>,
            history: port as Arc<dyn HistorySource>,
            telemetry: Arc::new(NoTelemetry),
            cache: Arc::new(NoCache),
            config: Arc::new(ConfigStore::new(config, config_path)),
            keys: Arc::new(keys),
        };
        let server = ApiServer::bind("127.0.0.1", 0, tls, state).await.unwrap();
        let base = server.base_url();

        let client = reqwest::Client::builder()
            // Self-signed appliance cert by design (SECURITY.md) — curl's `-k`.
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        Self {
            server,
            client,
            key,
            base,
            stats,
            data_dir,
            _config_dir: config_dir,
        }
    }

    async fn get_json(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.key)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "GET {path} returned {}",
            response.status()
        );
        response.json().await.unwrap()
    }

    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    /// Whether `GET /history/summary` over `[from, to)` returns no points — the
    /// black-box read of "is this day's rollup on disk", so the prune assertions
    /// never reconstruct a filename.
    async fn summary_is_empty(&self, (from, to): (&str, &str)) -> bool {
        let body = self
            .get_json(&format!(
                "/api/v1/history/summary?from={from}&to={to}&resolution=hour"
            ))
            .await;
        body["items"].as_array().expect("summary items").is_empty()
    }
}

/// The `StatsSource` + `HistorySource` port over the real `Stats`, mirroring the
/// binary's `StatsAdapter`. The history reads and `apply_history_config` are the
/// live methods under test; the rest of `StatsSource` is not exercised by these
/// tests and is stubbed to keep the double small.
struct StatsPort(Arc<Stats>);

impl StatsSource for StatsPort {
    fn overview(&self, now: SystemTime) -> StatsOverview {
        let snapshot = self.0.snapshot(now);
        StatsOverview {
            window: snapshot.window,
            queries_total: snapshot.queries_total,
            blocked_total: snapshot.blocked_total,
            blocked_percent: snapshot.blocked_percent,
            cache_hit_percent: snapshot.cache_hit_percent,
            top_blocked_domains: vec![],
            top_queried_domains: vec![],
            top_clients: vec![],
            buckets: vec![],
            policies: vec![],
        }
    }

    fn named_clients(&self) -> Vec<(IpAddr, Arc<str>)> {
        vec![]
    }

    fn clients(&self, _now: SystemTime) -> Vec<ClientEntry> {
        vec![]
    }

    fn set_client_name(&self, _ip: IpAddr, _name: Option<String>) -> Option<ClientEntry> {
        None
    }

    fn client_name(&self, ip: IpAddr) -> Option<String> {
        self.0.client_name(ip)
    }

    fn apply_history_config(&self, enabled: bool, retention_days: u32) {
        self.0.set_history_enabled(enabled);
        self.0.set_history_retention_days(retention_days);
    }

    fn heap(&self) -> fah_model::StatsHeap {
        self.0.heap()
    }
}

impl HistorySource for StatsPort {
    fn summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> io::Result<HistorySeries> {
        self.0.history_summary(range, resolution, max_points)
    }

    fn perf(&self, range: HistoryRange, max_points: usize) -> io::Result<PerfSeries> {
        self.0.history_perf(range, max_points)
    }

    fn top(&self, range: HistoryRange, kind: TopKind, limit: usize) -> io::Result<TopItems> {
        self.0.history_top(range, kind, limit)
    }
}

struct NoTelemetry;

impl TelemetrySource for NoTelemetry {
    fn degraded(&self) -> bool {
        false
    }

    fn allocator(&self) -> Option<fah_model::AllocatorStats> {
        None
    }

    fn process(&self) -> Option<fah_model::ProcessStats> {
        None
    }

    fn engine(&self) -> fah_model::EngineTelemetry {
        fah_model::EngineTelemetry::default()
    }
}

struct NoCache;

impl CacheSource for NoCache {
    fn stats(&self) -> CacheStats {
        CacheStats {
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
        }
    }

    fn clean(&self, _purge_stale: bool) -> CacheClean {
        CacheClean {
            removed_expired: 0,
            removed_stale: 0,
            entries_before: 0,
            entries_after: 0,
            freed_bytes: 0,
            duration: Duration::ZERO,
        }
    }
}
