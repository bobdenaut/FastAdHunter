//! End-to-end tests against a real server over real HTTPS with the
//! self-signed certificate, exercising API.md's documented contract.
//!
//! The stats/telemetry handles are fakes implementing the port traits — this
//! crate cannot depend on `fah-stats`/`fah-metrics` (L3 siblings,
//! ARCHITECTURE.md §Dependency Layering). Everything else is real: real
//! rustls, real axum routing, the real `ListManager`, the real config store.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fah_api::{
    ApiKeyStore, ApiServer, AppStateBuilder, BucketCount, CacheClean, CacheSource, CacheStats,
    ClientCount, ClientEntry, ConfigStore, DomainCount, HistorySource, PolicyCount, QueryLogPage,
    QueryLogRequest, QueryRecord, StatsOverview, StatsSource, TelemetrySource,
};
use fah_config::{Config, RulesConfig};
use fah_model::{
    CacheStatsSample, ClientHits, DecisiveRule, DomainHits, HistoryPoint, HistoryRange,
    HistoryResolution, HistorySeries, LatencySummary, PerfSample, PerfSeries, Query, QueryEvent,
    QueryType, TopItems, TopKind, Verdict,
};
use fah_rules::ListManager;
use serde_json::{json, Value};

// ─── Fakes for the L3 sibling ports ────────────────────────────────────

#[derive(Default)]
struct FakeStats {
    clients: Mutex<Vec<ClientEntry>>,
    queries: Mutex<Vec<QueryRecord>>,
    /// The `(enabled, retention_days)` last pushed by `apply_history_config` —
    /// lets a test prove `POST /api/v1/config` reaches the history writers live.
    applied_history: Mutex<Option<(bool, u32)>>,
}

impl FakeStats {
    fn with_client(ip: IpAddr) -> Self {
        Self {
            clients: Mutex::new(vec![ClientEntry {
                ip,
                name: None,
                first_seen: SystemTime::UNIX_EPOCH,
                last_seen: SystemTime::UNIX_EPOCH,
                queries_24h: 30_122,
                blocked_24h: 3_020,
            }]),
            queries: Mutex::new(vec![QueryRecord {
                event: fah_model::Event::dns(blocked_event(ip)),
                client_name: Some("liviu-phone".to_string()),
            }]),
            applied_history: Mutex::new(None),
        }
    }
}

fn blocked_event(client: IpAddr) -> QueryEvent {
    QueryEvent::new(
        Query::new(
            "ads.example.com",
            QueryType::A,
            client,
            SystemTime::UNIX_EPOCH,
        ),
        Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
        Duration::from_micros(300),
        false,
        false,
        false,
    )
}

impl StatsSource for FakeStats {
    fn overview(&self, _now: SystemTime) -> StatsOverview {
        StatsOverview {
            window: "24h",
            queries_total: 184_233,
            blocked_total: 23_411,
            blocked_percent: 12.7,
            cache_hit_percent: 61.4,
            top_blocked_domains: vec![DomainCount {
                domain: "ads.example.com".to_string(),
                count: 1_289,
            }],
            top_queried_domains: vec![DomainCount {
                domain: "api.example.org".to_string(),
                count: 4_021,
            }],
            top_clients: vec![ClientCount {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15)),
                name: Some("liviu-phone".to_string()),
                count: 30_122,
            }],
            buckets: vec![BucketCount {
                start: SystemTime::UNIX_EPOCH,
                queries: 5_120,
                blocked: 610,
            }],
            policies: vec![PolicyCount {
                policy: "kids".to_string(),
                queries: 812,
                blocked: 244,
            }],
        }
    }

    fn queries(&self, request: &QueryLogRequest) -> QueryLogPage {
        let items: Vec<QueryRecord> = self
            .queries
            .lock()
            .unwrap()
            .iter()
            .take(request.limit)
            .cloned()
            .collect();
        QueryLogPage {
            items,
            next_cursor: None,
        }
    }

    fn clients(&self, _now: SystemTime) -> Vec<ClientEntry> {
        self.clients.lock().unwrap().clone()
    }

    fn set_client_name(&self, ip: IpAddr, name: Option<String>) -> Option<ClientEntry> {
        let mut clients = self.clients.lock().unwrap();
        let entry = clients.iter_mut().find(|entry| entry.ip == ip)?;
        entry.name = name;
        Some(entry.clone())
    }

    fn client_name(&self, ip: IpAddr) -> Option<String> {
        self.clients
            .lock()
            .unwrap()
            .iter()
            .find(|entry| entry.ip == ip)
            .and_then(|entry| entry.name.clone())
    }

    fn named_clients(&self) -> Vec<(IpAddr, Arc<str>)> {
        self.clients
            .lock()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.name.as_deref().map(|n| (entry.ip, Arc::from(n))))
            .collect()
    }

    fn apply_history_config(&self, enabled: bool, retention_days: u32) {
        *self.applied_history.lock().unwrap() = Some((enabled, retention_days));
    }

    fn heap(&self) -> fah_model::StatsHeap {
        fah_model::StatsHeap {
            aggregates: 40_000,
            clients: 8_000,
            ring: 300_000,
            pending_log: 12_000,
        }
    }
}

/// Serves fixed fixtures and records what the handler asked for. The real
/// aggregation is `fah-stats`' (an L3 sibling this crate cannot import, so its
/// reader is tested there) — what these tests own is the boundary: query-string
/// parsing, the range that reaches the port, and the JSON shape API.md
/// documents.
#[derive(Default)]
struct FakeHistory {
    /// Every `(from, to, resolution, max_points)` a summary read was called with.
    summary_calls: Mutex<Vec<(SystemTime, SystemTime, HistoryResolution, usize)>>,
    top_calls: Mutex<Vec<(TopKind, usize)>>,
    /// Makes every read answer with nothing — the "range with no data" case.
    empty: Mutex<bool>,
}

impl FakeHistory {
    fn is_empty(&self) -> bool {
        *self.empty.lock().unwrap()
    }
}

impl HistorySource for FakeHistory {
    fn summary(
        &self,
        range: HistoryRange,
        resolution: HistoryResolution,
        max_points: usize,
    ) -> std::io::Result<HistorySeries> {
        self.summary_calls
            .lock()
            .unwrap()
            .push((range.from, range.to, resolution, max_points));
        if self.is_empty() {
            return Ok(HistorySeries {
                points: vec![],
                stride: 1,
            });
        }
        Ok(HistorySeries {
            points: vec![
                HistoryPoint {
                    ts: 0,
                    queries: 100,
                    blocked: 10,
                    cache_hits: 50,
                    per_type: BTreeMap::from([("A".to_string(), 100)]),
                },
                HistoryPoint {
                    ts: 3_600,
                    queries: 200,
                    blocked: 50,
                    cache_hits: 100,
                    per_type: BTreeMap::from([("AAAA".to_string(), 200)]),
                },
            ],
            stride: 2,
        })
    }

    fn perf(&self, _range: HistoryRange, _max_points: usize) -> std::io::Result<PerfSeries> {
        if self.is_empty() {
            return Ok(PerfSeries {
                samples: vec![],
                stride: 1,
            });
        }
        Ok(PerfSeries {
            samples: vec![PerfSample {
                ts: 3_600,
                rss_bytes: 55_000_000,
                qps: 12.5,
                queries_delta: 750,
                blocked_delta: 210,
                allowed_delta: 5,
                cache: CacheStatsSample {
                    entries: 10_000,
                    capacity: 16_384,
                    fresh: 9_000,
                    stale: 800,
                    expired: 200,
                    hits: 500_000,
                    misses: 120_000,
                    evictions: 3_400,
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
                // 55 MB RSS against 30 MB accounted → a 25 MB residual the
                // tests below assert is derived, not stored.
                memory: fah_model::MemoryComponents {
                    ruleset: 23_000_000,
                    cache: 5_000_000,
                    stats: fah_model::StatsHeap {
                        aggregates: 1_000_000,
                        clients: 500_000,
                        ring: 499_000,
                        pending_log: 1_000,
                    },
                },
                minor_page_faults: 4_211_337,
                upstreams: vec![],
            }],
            stride: 1,
        })
    }

    fn top(&self, _range: HistoryRange, kind: TopKind, limit: usize) -> std::io::Result<TopItems> {
        self.top_calls.lock().unwrap().push((kind, limit));
        if self.is_empty() {
            return Ok(match kind {
                TopKind::Clients => TopItems::Clients(vec![]),
                _ => TopItems::Domains(vec![]),
            });
        }
        Ok(match kind {
            TopKind::Clients => TopItems::Clients(vec![ClientHits {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15)),
                name: Some("liviu-phone".to_string()),
                count: 30_122,
            }]),
            _ => TopItems::Domains(vec![
                DomainHits {
                    domain: "ads.example.com".to_string(),
                    count: 1_289,
                },
                DomainHits {
                    domain: "tracker.example.org".to_string(),
                    count: 640,
                },
            ]),
        })
    }
}

struct FakeCache;

impl CacheSource for FakeCache {
    fn stats(&self) -> CacheStats {
        CacheStats {
            entries: 7_261,
            capacity: 10_000,
            fresh: 7_026,
            stale: 52,
            expired: 183,
            hits: 18_639_283,
            misses: 1_543_921,
            evictions: 21_483,
            bytes: 21_000_000,
            max_bytes: 67_108_864,
            estimated_bytes: 2_846_720,
        }
    }

    fn clean(&self, purge_stale: bool) -> CacheClean {
        CacheClean {
            removed_expired: 183,
            // Echoes the flag so the route test proves `?stale=true` lands.
            removed_stale: if purge_stale { 52 } else { 0 },
            entries_before: 7_261,
            entries_after: if purge_stale { 7_026 } else { 7_078 },
            freed_bytes: 71_744,
            duration: Duration::from_micros(4_700),
        }
    }
}

struct FakeTelemetry {
    degraded: bool,
}

impl TelemetrySource for FakeTelemetry {
    fn prometheus_text(&self) -> String {
        "# HELP fastadhunter_queries_total Total DNS queries processed, by verdict.\n\
         # TYPE fastadhunter_queries_total counter\n\
         fastadhunter_queries_total{verdict=\"block\"} 1\n"
            .to_string()
    }

    /// `None` on purpose: the test binary does not install mimalloc, so the
    /// figures would be meaningless. This also exercises the absent branch —
    /// the allocator fields must serialize as `null`, never as a fabricated 0.
    fn allocator(&self) -> Option<fah_model::AllocatorStats> {
        None
    }

    fn degraded(&self) -> bool {
        self.degraded
    }
}

// ─── Harness ───────────────────────────────────────────────────────────

struct Harness {
    server: ApiServer,
    client: reqwest::Client,
    key: String,
    base: String,
    config_path: std::path::PathBuf,
    rules: Arc<ListManager>,
    stats: Arc<FakeStats>,
    history: Arc<FakeHistory>,
    _config_dir: tempfile::TempDir,
    _data_dir: tempfile::TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.shutdown();
    }
}

struct HarnessOptions {
    tls: bool,
    metrics_public: bool,
    degraded: bool,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            tls: true,
            metrics_public: true,
            degraded: false,
        }
    }
}

async fn start() -> Harness {
    start_with(HarnessOptions::default()).await
}

async fn start_with(options: HarnessOptions) -> Harness {
    let config_dir = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    let mut config = Config::default();
    config.api.metrics_public = options.metrics_public;
    config.api.tls = options.tls;
    // No default lists: the shipped oisd entry would have the scheduler (and
    // some tests) reaching for the network.
    config.rules = RulesConfig {
        refresh_hours_default: 24,
        lists: vec![],
    };
    let config_path = config_dir.path().join("fastadhunter.toml");
    config.save(&config_path).unwrap();

    let rules = Arc::new(ListManager::new(&config.rules, data_dir.path().to_path_buf()).unwrap());
    rules.boot().await;

    let (keys, generated) = ApiKeyStore::load_or_create(config_dir.path()).unwrap();
    let key = generated.expect("first boot generates a key");

    let tls_config = options
        .tls
        .then(|| fah_api::load_or_generate_tls(config_dir.path()).unwrap());

    let stats = Arc::new(FakeStats::with_client(IpAddr::V4(Ipv4Addr::new(
        192, 168, 10, 15,
    ))));
    let history = Arc::new(FakeHistory::default());
    let state = AppStateBuilder {
        rules: Arc::clone(&rules),
        policies: Arc::new(fah_rules::PolicyState::default()),
        stats: Arc::clone(&stats) as Arc<dyn StatsSource>,
        history: Arc::clone(&history) as Arc<dyn HistorySource>,
        telemetry: Arc::new(FakeTelemetry {
            degraded: options.degraded,
        }),
        cache: Arc::new(FakeCache),
        config: Arc::new(ConfigStore::new(config, config_path.clone())),
        keys: Arc::new(keys),
    };

    let server = ApiServer::bind("127.0.0.1", 0, tls_config, state)
        .await
        .unwrap();
    let base = server.base_url();

    let client = reqwest::Client::builder()
        // The certificate is self-signed by design (SECURITY.md) — this is
        // the programmatic equivalent of curl's `-k`.
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    Harness {
        server,
        client,
        key,
        base,
        config_path,
        rules,
        stats,
        history,
        _config_dir: config_dir,
        _data_dir: data_dir,
    }
}

impl Harness {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(self.url(path))
            .bearer_auth(&self.key)
            .send()
            .await
            .unwrap()
    }

    async fn get_json(&self, path: &str) -> Value {
        let response = self.get(path).await;
        assert!(response.status().is_success(), "GET {path}");
        response.json().await.unwrap()
    }
}

// ─── Auth ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_missing_or_wrong_key_is_rejected_in_the_documented_error_shape() {
    let harness = start().await;

    for request in [
        harness.client.get(harness.url("/api/v1/stats")),
        harness
            .client
            .get(harness.url("/api/v1/stats"))
            .bearer_auth("not-the-key"),
        harness
            .client
            .get(harness.url("/api/v1/stats"))
            .header("Authorization", "Basic abc"),
    ] {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 401);

        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "unauthorized");
        assert!(body["error"]["message"].is_string());
    }
}

#[tokio::test]
async fn every_v1_route_requires_the_key() {
    let harness = start().await;

    // A representative route from each group; none may bypass auth.
    for path in [
        "/api/v1/stats",
        "/api/v1/queries",
        "/api/v1/clients",
        "/api/v1/history/summary",
        "/api/v1/history/perf",
        "/api/v1/history/top",
        "/api/v1/lists",
        "/api/v1/rules/user",
        "/api/v1/cache",
        "/api/v1/config",
        "/api/v1/debug/memory",
    ] {
        let response = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 401, "{path} must require the key");
    }
}

#[tokio::test]
async fn health_and_metrics_are_public_by_default_but_lock_down_when_configured() {
    let harness = start().await;
    for path in ["/health", "/metrics"] {
        let response = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 200, "{path} is exempt by default");
    }

    let locked = start_with(HarnessOptions {
        metrics_public: false,
        ..Default::default()
    })
    .await;
    for path in ["/health", "/metrics"] {
        let response = locked.client.get(locked.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 401, "{path} follows metrics_public");

        let authorized = locked.get(path).await;
        assert_eq!(authorized.status(), 200, "{path} still works with the key");
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_json_not_found() {
    let harness = start().await;
    let response = harness.get("/api/v1/nope").await;
    assert_eq!(response.status(), 404);

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "not_found");
}

// ─── TLS ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn https_is_the_default_and_a_plain_client_cannot_talk_to_it() {
    let harness = start().await;
    assert!(harness.base.starts_with("https://"));

    let strict = reqwest::Client::new();
    assert!(
        strict.get(harness.url("/health")).send().await.is_err(),
        "a client that validates certificates must reject the self-signed one"
    );
}

#[tokio::test]
async fn plain_http_is_available_as_the_documented_opt_out() {
    let harness = start_with(HarnessOptions {
        tls: false,
        ..Default::default()
    })
    .await;
    assert!(harness.base.starts_with("http://"));

    let response = harness.get("/health").await;
    assert_eq!(response.status(), 200);
}

// ─── Health & telemetry ────────────────────────────────────────────────

#[tokio::test]
async fn health_reports_status_version_and_uptime() {
    let harness = start().await;
    let body = harness.get_json("/health").await;

    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["uptime_seconds"].is_u64());
}

#[tokio::test]
async fn health_reports_degraded_when_every_upstream_is_failing() {
    let harness = start_with(HarnessOptions {
        degraded: true,
        ..Default::default()
    })
    .await;
    assert_eq!(harness.get_json("/health").await["status"], "degraded");
}

#[tokio::test]
async fn metrics_serves_prometheus_text_with_the_right_content_type() {
    let harness = start().await;
    let response = harness.get("/metrics").await;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.starts_with("text/plain"), "got {content_type}");

    let body = response.text().await.unwrap();
    assert!(body.contains("# HELP fastadhunter_queries_total"));
    assert!(body.contains("# TYPE fastadhunter_queries_total counter"));
}

// ─── Statistics & query log ────────────────────────────────────────────

#[tokio::test]
async fn stats_matches_the_documented_shape() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/stats").await;

    assert_eq!(body["window"], "24h");
    assert_eq!(body["queries_total"], 184_233);
    assert_eq!(body["blocked_total"], 23_411);
    assert_eq!(body["blocked_percent"], 12.7);
    assert_eq!(body["cache_hit_percent"], 61.4);
    assert_eq!(body["top_blocked_domains"][0]["domain"], "ads.example.com");
    assert_eq!(body["top_blocked_domains"][0]["count"], 1_289);
    assert_eq!(body["top_clients"][0]["ip"], "192.168.10.15");
    assert_eq!(body["top_clients"][0]["name"], "liviu-phone");
    // RFC 3339, not serde's SystemTime struct.
    assert_eq!(body["buckets"][0]["start"], "1970-01-01T00:00:00Z");
    assert_eq!(body["buckets"][0]["queries"], 5_120);
    assert_eq!(body["policies"][0]["policy"], "kids");
    assert_eq!(body["policies"][0]["queries"], 812);
    assert_eq!(body["policies"][0]["blocked"], 244);
}

#[tokio::test]
async fn queries_matches_the_documented_shape() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/queries").await;

    let item = &body["items"][0];
    assert_eq!(item["ts"], "1970-01-01T00:00:00Z");
    assert_eq!(item["client"], "192.168.10.15");
    assert_eq!(item["client_name"], "liviu-phone");
    assert_eq!(item["domain"], "ads.example.com");
    assert_eq!(item["qtype"], "A");
    assert_eq!(item["verdict"], "block");
    assert_eq!(item["rule"], "||ads.example.com^");
    assert_eq!(item["list"], "oisd-basic");
    assert_eq!(item["duration_ms"], 0.3);
    assert!(item["upstream"].is_null());
    assert_eq!(item["cached"], false);
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn query_filters_are_validated() {
    let harness = start().await;

    let response = harness.get("/api/v1/queries?verdict=maybe").await;
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "bad_request");

    assert_eq!(
        harness.get("/api/v1/queries?client=nope").await.status(),
        400
    );
    assert_eq!(
        harness.get("/api/v1/queries?from=yesterday").await.status(),
        400
    );
    // Valid filters pass through.
    assert_eq!(
        harness
            .get("/api/v1/queries?verdict=block&limit=10&domain=ads")
            .await
            .status(),
        200
    );
}

// ─── History (persisted series) ────────────────────────────────────────

#[tokio::test]
async fn history_summary_matches_the_documented_shape_at_both_resolutions() {
    let harness = start().await;

    let body = harness
        .get_json(
            "/api/v1/history/summary\
             ?from=2026-07-01T00:00:00Z&to=2026-07-08T00:00:00Z&resolution=day",
        )
        .await;

    assert_eq!(body["resolution"], "day");
    assert_eq!(body["from"], "2026-07-01T00:00:00Z");
    assert_eq!(body["to"], "2026-07-08T00:00:00Z");
    assert_eq!(body["stride"], 2, "decimation is reported, not hidden");

    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["ts"], "1970-01-01T00:00:00Z");
    assert_eq!(items[0]["queries"], 100);
    assert_eq!(items[0]["blocked"], 10);
    assert_eq!(items[0]["blocked_percent"], 10.0, "derived at the boundary");
    assert_eq!(items[0]["cache_hits"], 50);
    assert_eq!(items[0]["per_type"]["A"], 100);
    assert_eq!(items[1]["blocked_percent"], 25.0);

    // The range and resolution the query string asked for reached the reader.
    let calls = harness.history.summary_calls.lock().unwrap();
    let (from, to, resolution, max_points) = calls.last().unwrap();
    assert_eq!(*resolution, HistoryResolution::Day);
    assert_eq!(
        to.duration_since(*from).unwrap(),
        Duration::from_secs(7 * 24 * 3600)
    );
    assert_eq!(*max_points, 5_000, "the documented summary default");
}

#[tokio::test]
async fn history_defaults_to_the_last_24h_at_hour_resolution() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/history/summary").await;
    assert_eq!(body["resolution"], "hour");

    let calls = harness.history.summary_calls.lock().unwrap();
    let (from, to, resolution, _) = calls.last().unwrap();
    assert_eq!(*resolution, HistoryResolution::Hour);
    let window = to.duration_since(*from).unwrap();
    assert_eq!(window, Duration::from_secs(24 * 3600));
    // `to` defaults to now, so the window ends about now.
    assert!(SystemTime::now().duration_since(*to).unwrap() < Duration::from_secs(10));
}

#[tokio::test]
async fn a_history_range_with_no_data_is_an_empty_series_not_a_404() {
    let harness = start().await;
    *harness.history.empty.lock().unwrap() = true;

    for path in [
        "/api/v1/history/summary?from=1970-01-01T00:00:00Z&to=1970-01-02T00:00:00Z",
        "/api/v1/history/perf?from=1970-01-01T00:00:00Z&to=1970-01-02T00:00:00Z",
        "/api/v1/history/top?from=1970-01-01T00:00:00Z&to=1970-01-02T00:00:00Z",
    ] {
        let response = harness.get(path).await;
        assert_eq!(response.status(), 200, "{path} must not 404 on empty");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["items"].as_array().unwrap().len(), 0, "{path}");
    }
}

#[tokio::test]
async fn history_range_and_enum_parameters_are_validated() {
    let harness = start().await;

    for path in [
        // `to` before `from`: a window that cannot contain anything is a
        // request bug, not an empty answer.
        "/api/v1/history/summary?from=2026-07-08T00:00:00Z&to=2026-07-01T00:00:00Z",
        "/api/v1/history/perf?from=2026-07-08T00:00:00Z&to=2026-07-01T00:00:00Z",
        "/api/v1/history/top?from=2026-07-08T00:00:00Z&to=2026-07-01T00:00:00Z",
        "/api/v1/history/summary?from=yesterday",
        "/api/v1/history/summary?resolution=minute",
        "/api/v1/history/summary?max_points=lots",
        "/api/v1/history/top?kind=everything",
        "/api/v1/history/perf?fields=rss_bytes,nonsense",
    ] {
        let response = harness.get(path).await;
        assert_eq!(response.status(), 400, "{path} must be rejected");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "bad_request", "{path}");
    }
}

#[tokio::test]
async fn history_perf_serves_the_sample_series_and_fields_trim_it() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/history/perf").await;
    let item = &body["items"][0];
    assert_eq!(item["ts"], "1970-01-01T01:00:00Z");
    assert_eq!(item["rss_bytes"], 55_000_000u64);
    assert_eq!(item["qps"], 12.5);
    assert_eq!(item["queries_delta"], 750);
    assert_eq!(item["cache"]["entries"], 10_000);
    assert_eq!(item["latency"]["forward_p99"], 0.05);
    assert!(item["upstreams"].is_array());

    // `fields` drops the keys it did not name — absent, not null.
    let body = harness
        .get_json("/api/v1/history/perf?fields=rss_bytes,cache")
        .await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["cache", "rss_bytes", "ts"]);
}

#[tokio::test]
async fn history_perf_derives_the_residual_from_each_row() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/history/perf").await;
    let memory = &body["items"][0]["memory"];
    assert_eq!(memory["ruleset_bytes"], 23_000_000u64);
    assert_eq!(memory["stats_clients_bytes"], 500_000);
    assert_eq!(memory["accounted_bytes"], 30_000_000u64);
    // 55,000,000 RSS − 30,000,000 accounted, computed on read: the row stores
    // neither the residual nor a second RSS.
    assert_eq!(memory["residual_bytes"], 25_000_000u64);
    assert!(memory.get("process_rss").is_none());
    assert_eq!(body["items"][0]["minor_page_faults"], 4_211_337u64);

    // Both are selectable, and selecting one does not drag the other in.
    let body = harness.get_json("/api/v1/history/perf?fields=memory").await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["memory", "ts"]);

    let body = harness
        .get_json("/api/v1/history/perf?fields=minor_page_faults")
        .await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["minor_page_faults", "ts"]);
}

/// The live and the persisted surface must agree by construction: both build a
/// `MemoryBreakdown` and ask it for `accounted`/`residual` (p2-07).
#[tokio::test]
async fn live_and_persisted_breakdowns_use_the_same_keys_and_arithmetic() {
    let harness = start().await;

    let live = harness.get_json("/api/v1/debug/memory").await;
    let row = &harness.get_json("/api/v1/history/perf").await["items"][0]["memory"];

    for key in [
        "ruleset_bytes",
        "cache_estimated_bytes",
        "stats_aggregates_bytes",
        "stats_clients_bytes",
        "query_log_ring_bytes",
        "query_log_pending_bytes",
        "accounted_bytes",
        "residual_bytes",
    ] {
        assert!(live.get(key).is_some(), "/debug/memory lost {key}");
        assert!(row.get(key).is_some(), "the perf row lost {key}");
    }
    assert_eq!(
        live["accounted_bytes"].as_u64().unwrap(),
        live["ruleset_bytes"].as_u64().unwrap()
            + live["cache_estimated_bytes"].as_u64().unwrap()
            + live["stats_aggregates_bytes"].as_u64().unwrap()
            + live["stats_clients_bytes"].as_u64().unwrap()
            + live["query_log_ring_bytes"].as_u64().unwrap()
            + live["query_log_pending_bytes"].as_u64().unwrap(),
    );
}

#[tokio::test]
async fn history_top_ranks_domains_by_default_and_clients_on_request() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/history/top").await;
    assert_eq!(body["kind"], "blocked");
    assert_eq!(body["items"][0]["domain"], "ads.example.com");
    assert_eq!(body["items"][0]["count"], 1_289);

    let body = harness
        .get_json("/api/v1/history/top?kind=clients&n=1")
        .await;
    assert_eq!(body["kind"], "clients");
    assert_eq!(body["items"][0]["ip"], "192.168.10.15");
    assert_eq!(body["items"][0]["name"], "liviu-phone");

    let calls = harness.history.top_calls.lock().unwrap();
    assert_eq!(calls[0], (TopKind::Blocked, 10), "the documented default n");
    assert_eq!(calls[1], (TopKind::Clients, 1));
}

#[tokio::test]
async fn a_wide_history_request_is_clamped_to_the_documented_ceiling() {
    let harness = start().await;

    harness
        .get_json("/api/v1/history/summary?max_points=999999")
        .await;
    harness.get_json("/api/v1/history/top?n=999999").await;

    assert_eq!(
        harness.history.summary_calls.lock().unwrap()[0].3,
        10_000,
        "max_points is clamped, not honored verbatim"
    );
    assert_eq!(harness.history.top_calls.lock().unwrap()[0].1, 100);
}

// ─── Cache & memory ────────────────────────────────────────────────────

#[tokio::test]
async fn cache_stats_match_the_documented_shape() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/cache").await;

    assert_eq!(body["entries"], 7_261);
    assert_eq!(body["capacity"], 10_000);
    assert_eq!(body["fresh"], 7_026);
    assert_eq!(body["stale"], 52);
    assert_eq!(body["expired"], 183);
    assert_eq!(body["hits"], 18_639_283u64);
    assert_eq!(body["misses"], 1_543_921);
    assert_eq!(body["evictions"], 21_483);
    assert_eq!(body["load_percent"], 72.61);
    // The byte bound rides alongside the entry bound (p1.5-05) so a dashboard
    // can see which of the two is the one about to evict.
    assert_eq!(body["bytes"], 21_000_000);
    assert_eq!(body["max_bytes"], 67_108_864u64);
    assert_eq!(body["byte_load_percent"], 31.29);
}

#[tokio::test]
async fn cache_clean_keeps_stale_by_default_and_purges_on_request() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/cache/clean"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["removed_expired"], 183);
    assert_eq!(body["removed_stale"], 0, "stale is RFC 8767 insurance");
    assert_eq!(body["entries_before"], 7_261);
    assert_eq!(body["entries_after"], 7_078);
    assert_eq!(body["freed_bytes"], 71_744);
    assert_eq!(body["duration_ms"], 4.7);

    let purged: Value = harness
        .client
        .post(harness.url("/api/v1/cache/clean?stale=true"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(purged["removed_stale"], 52, "?stale=true reaches the port");
}

#[tokio::test]
async fn debug_memory_reports_the_resident_parts() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/debug/memory").await;

    assert!(body["ruleset_bytes"].is_u64());
    assert_eq!(body["cache_entries"], 7_261);
    assert_eq!(body["cache_estimated_bytes"], 2_846_720);
    assert!(
        body["process_rss"].is_u64() || body["process_rss"].is_null(),
        "a number on the Linux target, null elsewhere — got {:?}",
        body["process_rss"]
    );

    // `FakeTelemetry::allocator` returns `None`, so every allocator field must
    // be present and `null` — never absent (a client cannot tell a renamed
    // field from an unavailable one) and never `0`, which would chart as a real
    // measurement of an allocator holding nothing (see `crates/fastadhunter/src/allocator.rs`).
    for field in [
        "allocator_committed_bytes",
        "allocator_committed_peak_bytes",
        "process_peak_rss",
        "major_page_faults",
        "minor_page_faults",
    ] {
        let value = body
            .get(field)
            .unwrap_or_else(|| panic!("{field} missing from /debug/memory"));
        assert!(
            value.is_null(),
            "{field} must be null when the allocator cannot report, not {value:?}"
        );
    }
}

// ─── Clients ───────────────────────────────────────────────────────────

#[tokio::test]
async fn clients_list_and_naming_round_trip() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/clients").await;
    let item = &body["items"][0];
    assert_eq!(item["ip"], "192.168.10.15");
    assert!(item["name"].is_null());
    assert_eq!(item["first_seen"], "1970-01-01T00:00:00Z");
    assert_eq!(item["queries_24h"], 30_122);
    assert_eq!(item["blocked_24h"], 3_020);

    let response = harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.15"))
        .bearer_auth(&harness.key)
        .json(&json!({"name": "liviu-phone"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let updated: Value = response.json().await.unwrap();
    assert_eq!(updated["name"], "liviu-phone");

    // `{"name": null}` clears it (API.md).
    let cleared: Value = harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.15"))
        .bearer_auth(&harness.key)
        .json(&json!({ "name": Value::Null }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(cleared["name"].is_null());
}

#[tokio::test]
async fn naming_an_unseen_client_is_a_not_found() {
    let harness = start().await;
    let response = harness
        .client
        .put(harness.url("/api/v1/clients/10.0.0.1"))
        .bearer_auth(&harness.key)
        .json(&json!({"name": "ghost"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

// ─── Rule lists ────────────────────────────────────────────────────────

#[tokio::test]
async fn lists_crud_round_trips_through_the_api() {
    let harness = start().await;
    assert_eq!(harness.get_json("/api/v1/lists").await["items"], json!([]));

    let created: Value = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"url": "https://example.org/oisd-basic.txt", "refresh_hours": 6}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["id"], "oisd-basic");
    assert_eq!(created["format"], "auto");
    assert_eq!(created["enabled"], true);
    assert_eq!(created["refresh_hours"], 6);
    assert_eq!(created["last_status"], "never");
    assert!(created["last_refresh"].is_null());
    assert_eq!(created["rules_total"], 0);

    // A duplicate id conflicts rather than silently replacing.
    let duplicate = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"url": "https://example.org/oisd-basic.txt"}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    let patched: Value = harness
        .client
        .patch(harness.url("/api/v1/lists/oisd-basic"))
        .bearer_auth(&harness.key)
        .json(&json!({"enabled": false, "refresh_hours": Value::Null}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(patched["enabled"], false);
    assert_eq!(
        patched["refresh_hours"], 24,
        "clearing the override falls back to the global default"
    );

    let deleted = harness
        .client
        .delete(harness.url("/api/v1/lists/oisd-basic"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
    assert_eq!(harness.get_json("/api/v1/lists").await["items"], json!([]));
}

/// A list added through the API has to survive a restart. It used to live
/// only in the in-memory `ListManager`: it served traffic, its content was
/// cached under `/data`, and then the next boot read `fastadhunter.toml`,
/// found no entry, and silently dropped the rules.
#[tokio::test]
async fn list_mutations_are_persisted_to_the_config_file() {
    let harness = start().await;

    // What a restart sees: the file, reparsed, not the running state.
    let on_disk = || {
        let text = std::fs::read_to_string(&harness.config_path).unwrap();
        Config::from_toml_str(&text).unwrap().rules.lists
    };
    assert!(on_disk().is_empty());

    for url in [
        "https://example.org/oisd-basic.txt",
        "https://example.org/extra.txt",
    ] {
        let response = harness
            .client
            .post(harness.url("/api/v1/lists"))
            .bearer_auth(&harness.key)
            .json(&json!({"url": url, "refresh_hours": 6}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }

    let persisted = on_disk();
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].id, "oisd-basic");
    assert_eq!(persisted[0].url, "https://example.org/oisd-basic.txt");
    assert_eq!(persisted[0].refresh_hours, Some(6));
    assert_eq!(persisted[1].id, "extra");

    // A rejected duplicate must not append a second entry.
    let duplicate = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"url": "https://example.org/oisd-basic.txt"}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);
    assert_eq!(on_disk().len(), 2, "a conflict changes nothing on disk");

    harness
        .client
        .patch(harness.url("/api/v1/lists/oisd-basic"))
        .bearer_auth(&harness.key)
        .json(&json!({"enabled": false, "refresh_hours": Value::Null}))
        .send()
        .await
        .unwrap();
    let patched = on_disk();
    assert!(!patched[0].enabled, "the disable outlives the process");
    assert_eq!(patched[0].refresh_hours, None);

    harness
        .client
        .delete(harness.url("/api/v1/lists/extra"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    let remaining = on_disk();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "oisd-basic");

    // A 404 on a list that never existed leaves the file alone.
    harness
        .client
        .delete(harness.url("/api/v1/lists/ghost"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(on_disk().len(), 1);
}

#[tokio::test]
async fn a_traversal_shaped_id_or_path_is_rejected_at_the_boundary() {
    let harness = start().await;

    for body in [
        json!({"url": "https://example.org/l.txt", "id": "../../config/apikey"}),
        json!({"url": "https://example.org/l.txt", "id": "UPPER"}),
        json!({"path": "../outside.txt"}),
    ] {
        let response = harness
            .client
            .post(harness.url("/api/v1/lists"))
            .bearer_auth(&harness.key)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 422, "{body} must be rejected");
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["error"]["code"], "validation_failed");
    }

    assert_eq!(
        harness.get_json("/api/v1/lists").await["items"],
        json!([]),
        "nothing was persisted"
    );
}

#[tokio::test]
async fn the_same_source_cannot_be_added_twice_under_different_ids() {
    let harness = start().await;
    let url = "https://example.org/oisd-basic.txt";

    let first = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"url": url}))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 201);

    // An explicit id sidesteps the id check, so this is the URL check or
    // nothing — and "nothing" means fetching, caching and compiling the same
    // list twice.
    let second = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"url": url, "id": "a-different-name"}))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 409);
    let body: Value = second.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("oisd-basic"),
        "the conflict names the list already holding the URL, got: {message}"
    );

    let lists: Value = harness
        .client
        .get(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(lists["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_local_file_list_can_be_added_by_path_and_refreshed() {
    let harness = start().await;
    // A file under the manager's own /data dir, as a mounted list would be.
    let list_path = harness._data_dir.path().join("local.txt");
    tokio::fs::write(&list_path, "ads.example.com\n")
        .await
        .unwrap();

    let created: Value = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"path": "local.txt"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["id"], "local");

    let accepted = harness
        .client
        .post(harness.url("/api/v1/lists/local/refresh"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 202, "refresh is accepted, not awaited");

    // The refresh runs in the background; wait for it to show up.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let body = harness.get_json("/api/v1/lists").await;
        if body["items"][0]["last_status"] == "ok" {
            assert_eq!(body["items"][0]["rules_active_dns"], 1);
            assert!(body["items"][0]["last_refresh"].is_string());
            break;
        }
        assert!(std::time::Instant::now() < deadline, "refresh never landed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn overlapping_lists_report_compiled_rules_net_of_duplicates() {
    // Two lists sharing two of three domains — the AdGuard/HaGeZi situation
    // that motivated dedup, in miniature. The per-list counts stay parse-based
    // (each list still *has* those rules); the envelope reports the merge.
    let harness = start().await;
    let data = harness._data_dir.path();
    tokio::fs::write(data.join("a.txt"), "ads.example.com\ntracker.example.org\n")
        .await
        .unwrap();
    tokio::fs::write(
        data.join("b.txt"),
        "ads.example.com\ntracker.example.org\nextra.example.net\n",
    )
    .await
    .unwrap();

    for name in ["a.txt", "b.txt"] {
        harness
            .client
            .post(harness.url("/api/v1/lists"))
            .bearer_auth(&harness.key)
            .json(&json!({"path": name}))
            .send()
            .await
            .unwrap();
        harness
            .client
            .post(harness.url(&format!(
                "/api/v1/lists/{}/refresh",
                name.trim_end_matches(".txt")
            )))
            .bearer_auth(&harness.key)
            .send()
            .await
            .unwrap();
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let body = harness.get_json("/api/v1/lists").await;
        if body["compiled_rules"] == 3 {
            assert_eq!(body["duplicates_removed"], 2);
            assert_eq!(
                body["items"][0]["rules_active_dns"], 2,
                "per-list counts stay parse-based, before the merge"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "both refreshes never landed: {body}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn operating_on_an_unknown_list_is_a_not_found() {
    let harness = start().await;
    for response in [
        harness
            .client
            .patch(harness.url("/api/v1/lists/ghost"))
            .bearer_auth(&harness.key)
            .json(&json!({"enabled": false}))
            .send()
            .await
            .unwrap(),
        harness
            .client
            .delete(harness.url("/api/v1/lists/ghost"))
            .bearer_auth(&harness.key)
            .send()
            .await
            .unwrap(),
        harness
            .client
            .post(harness.url("/api/v1/lists/ghost/refresh"))
            .bearer_auth(&harness.key)
            .send()
            .await
            .unwrap(),
    ] {
        assert_eq!(response.status(), 404);
    }
}

#[tokio::test]
async fn refresh_all_refreshes_every_list_best_effort_and_reports_each() {
    let harness = start().await;
    let data = harness._data_dir.path();
    // One good local list and one whose file is missing, so its fetch fails —
    // the failure must not stop the good one from refreshing (best-effort).
    tokio::fs::write(data.join("good.txt"), "ads.example.com\n")
        .await
        .unwrap();
    for name in ["good.txt", "ghost.txt"] {
        let created = harness
            .client
            .post(harness.url("/api/v1/lists"))
            .bearer_auth(&harness.key)
            .json(&json!({"path": name}))
            .send()
            .await
            .unwrap();
        assert!(created.status().is_success(), "add {name}");
    }

    let response = harness
        .client
        .post(harness.url("/api/v1/lists/refresh"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();

    assert_eq!(body["refreshed"], 1, "the good list refreshed");
    assert_eq!(
        body["failed"], 1,
        "the missing one failed but did not abort the batch"
    );

    let result = |id: &str| {
        body["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no result for {id}"))
            .clone()
    };
    let good = result("good");
    assert_eq!(good["status"], "ok");
    assert_eq!(good["rules_active_dns"], 1);
    assert!(good.get("error").is_none() || good["error"].is_null());

    let ghost = result("ghost");
    assert_eq!(ghost["status"], "failed");
    assert!(
        ghost["error"].is_string(),
        "a failure carries its error chain"
    );
    assert!(ghost.get("rules_active_dns").is_none() || ghost["rules_active_dns"].is_null());

    // Best-effort really applied: the good list's rule serves despite the bad one.
    assert!(matches!(
        harness
            .rules
            .matcher()
            .lookup("ads.example.com", &QueryType::A),
        fah_rules::MatchDecision::Block(_)
    ));
}

// ─── Rules ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn user_rules_put_applies_atomically_and_get_reads_them_back() {
    let harness = start().await;
    assert_eq!(
        harness.get_json("/api/v1/rules/user").await["rules"],
        json!([])
    );

    let response = harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||tracker.example.com^", "@@||goodsite.example.com^"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    assert_eq!(
        harness.get_json("/api/v1/rules/user").await["rules"],
        json!(["||tracker.example.com^", "@@||goodsite.example.com^"])
    );

    // The swap is live: the matcher already sees the new rule.
    assert!(matches!(
        harness
            .rules
            .matcher()
            .lookup("tracker.example.com", &QueryType::A),
        fah_rules::MatchDecision::Block(_)
    ));
}

#[tokio::test]
async fn user_rules_put_drops_exact_duplicates_preserving_order() {
    let harness = start().await;
    let response = harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": [
            "||bing.com^",
            "||applicationinsights.azure.com^",
            "||applicationinsights.azure.com^",
            "||bing.com^",
        ]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Exact duplicates collapse to the first occurrence; order is preserved.
    let expected = json!(["||bing.com^", "||applicationinsights.azure.com^"]);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["rules"],
        expected,
        "PUT echoes the deduped set"
    );
    assert_eq!(
        harness.get_json("/api/v1/rules/user").await["rules"],
        expected,
        "the persisted set is deduped"
    );
}

#[tokio::test]
async fn invalid_user_rules_are_rejected_with_per_line_messages() {
    let harness = start().await;
    let response = harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||ads.example.com^", "||^", "||also-fine.example^"]}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("line 2"), "got: {message}");

    // Nothing was applied.
    assert_eq!(
        harness.get_json("/api/v1/rules/user").await["rules"],
        json!([])
    );
}

#[tokio::test]
async fn rules_test_dry_runs_a_verdict() {
    let harness = start().await;
    harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||ads.example.com^"]}))
        .send()
        .await
        .unwrap();

    let blocked: Value = harness
        .client
        .post(harness.url("/api/v1/rules/test"))
        .bearer_auth(&harness.key)
        .json(&json!({"domain": "ads.example.com", "qtype": "A", "client": "192.168.10.15"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(blocked["verdict"], "block");
    assert_eq!(blocked["rule"], "||ads.example.com^");
    assert_eq!(blocked["list"], "user-rules");

    let passed: Value = harness
        .client
        .post(harness.url("/api/v1/rules/test"))
        .bearer_auth(&harness.key)
        .json(&json!({"domain": "example.org"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(passed["verdict"], "pass");
    assert!(passed["rule"].is_null());
    assert!(passed["list"].is_null());
    assert_eq!(
        passed["policy"], "default",
        "an unassigned client reports the default policy"
    );
}

// ─── Policies (p2-06) ──────────────────────────────────────────────────

/// The acceptance criterion: create a policy, assign a client, and the verdict
/// changes live — no restart, and the TOML records it.
#[tokio::test]
async fn a_policy_and_an_assignment_change_a_verdict_live() {
    let harness = start().await;
    harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||games.example.com^"]}))
        .send()
        .await
        .unwrap();

    let empty = harness.get_json("/api/v1/policies").await;
    assert_eq!(empty["items"], json!([]));
    assert_eq!(empty["timezone"], "UTC");
    assert_eq!(empty["active_assignments"], 0);

    // A policy that enables no list — so a client on it sees no rule at all.
    let created: Value = harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "open", "name": "Open", "lists": []}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["id"], "open");
    assert_eq!(created["name"], "Open");
    assert_eq!(created["assignments"], json!([]));

    // Before the assignment, the client is judged under the default policy.
    let before = test_rule(
        &harness,
        json!({"domain": "games.example.com", "client": "192.168.10.15"}),
    )
    .await;
    assert_eq!(before["verdict"], "block");
    assert_eq!(before["policy"], "default");

    let assigned: Value = harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.15/policy"))
        .bearer_auth(&harness.key)
        .json(&json!({"policy": "open"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(assigned["policy"], "open");
    assert_eq!(assigned["assignment"]["client"], "192.168.10.15");

    // Live: same request, different answer, no restart.
    let after = test_rule(
        &harness,
        json!({"domain": "games.example.com", "client": "192.168.10.15"}),
    )
    .await;
    assert_eq!(after["verdict"], "pass");
    assert_eq!(after["policy"], "open");

    // Everyone else still gets the default policy's verdict.
    let other = test_rule(
        &harness,
        json!({"domain": "games.example.com", "client": "192.168.10.99"}),
    )
    .await;
    assert_eq!(other["verdict"], "block");
    assert_eq!(other["policy"], "default");

    // Persisted, so the next boot serves the same thing.
    let on_disk = std::fs::read_to_string(&harness.config_path).unwrap();
    let reparsed = Config::from_toml_str(&on_disk).unwrap();
    assert_eq!(reparsed.policies.len(), 1);
    assert_eq!(reparsed.policies[0].assignments[0].client, "192.168.10.15");

    assert_eq!(
        harness.get_json("/api/v1/policies").await["active_assignments"],
        1
    );

    // Clearing it returns the client to the default policy, live.
    let cleared = harness
        .client
        .delete(harness.url("/api/v1/clients/192.168.10.15/policy"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 204);
    let reverted = test_rule(
        &harness,
        json!({"domain": "games.example.com", "client": "192.168.10.15"}),
    )
    .await;
    assert_eq!(reverted["verdict"], "block");
    assert_eq!(reverted["policy"], "default");
}

async fn test_rule(harness: &Harness, body: Value) -> Value {
    harness
        .client
        .post(harness.url("/api/v1/rules/test"))
        .bearer_auth(&harness.key)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// `rules/test` can answer for a policy directly, so "what would kids see?" is
/// answerable without owning a device on that policy.
#[tokio::test]
async fn rules_test_can_answer_under_a_named_policy() {
    let harness = start().await;
    harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||games.example.com^"]}))
        .send()
        .await
        .unwrap();
    harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "open", "lists": []}))
        .send()
        .await
        .unwrap();

    let under_open = test_rule(
        &harness,
        json!({"domain": "games.example.com", "policy": "open"}),
    )
    .await;
    assert_eq!(under_open["verdict"], "pass");
    assert_eq!(under_open["policy"], "open");

    let under_default = test_rule(&harness, json!({"domain": "games.example.com"})).await;
    assert_eq!(under_default["verdict"], "block");

    let unknown = harness
        .client
        .post(harness.url("/api/v1/rules/test"))
        .bearer_auth(&harness.key)
        .json(&json!({"domain": "games.example.com", "policy": "nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 422);
}

#[tokio::test]
async fn policy_crud_rejects_the_shapes_it_should() {
    let harness = start().await;

    // `default` is the implicit policy; redefining it is a mistake, not a
    // second policy that happens to share a name.
    let reserved = harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "default"}))
        .send()
        .await
        .unwrap();
    assert_eq!(reserved.status(), 422);

    for bad in ["../escape", "Kids", "a b"] {
        let response = harness
            .client
            .post(harness.url("/api/v1/policies"))
            .bearer_auth(&harness.key)
            .json(&json!({"id": bad}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 422, "{bad:?} must be rejected");
    }

    harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "kids"}))
        .send()
        .await
        .unwrap();
    let duplicate = harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "kids"}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    // Assigning to a policy that does not exist is a 404, not a silent no-op.
    let unknown = harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.15/policy"))
        .bearer_auth(&harness.key)
        .json(&json!({"policy": "ghost"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);

    let missing = harness
        .client
        .patch(harness.url("/api/v1/policies/ghost"))
        .bearer_auth(&harness.key)
        .json(&json!({"name": "Ghost"}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    let deleted = harness
        .client
        .delete(harness.url("/api/v1/policies/kids"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
}

/// A schedule survives the round trip in the spelling it was written in — the
/// API is not allowed to normalize `21:00` into something the TOML cannot show
/// back to the operator.
#[tokio::test]
async fn a_scheduled_assignment_round_trips_and_persists() {
    let harness = start().await;
    harness
        .client
        .post(harness.url("/api/v1/policies"))
        .bearer_auth(&harness.key)
        .json(&json!({"id": "kids"}))
        .send()
        .await
        .unwrap();
    harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.15/policy"))
        .bearer_auth(&harness.key)
        .json(&json!({
            "policy": "kids",
            "days": "mon-fri",
            "start": "21:00",
            "end": "07:00"
        }))
        .send()
        .await
        .unwrap();

    let items = harness.get_json("/api/v1/policies").await;
    let assignment = &items["items"][0]["assignments"][0];
    assert_eq!(assignment["client"], "192.168.10.15");
    assert_eq!(assignment["days"], "mon-fri");
    assert_eq!(assignment["start"], "21:00");
    assert_eq!(assignment["end"], "07:00");

    let reparsed =
        Config::from_toml_str(&std::fs::read_to_string(&harness.config_path).unwrap()).unwrap();
    assert_eq!(
        reparsed.policies[0].assignments[0].start.as_deref(),
        Some("21:00")
    );

    // A malformed schedule is rejected before anything is written.
    let bad = harness
        .client
        .put(harness.url("/api/v1/clients/192.168.10.16/policy"))
        .bearer_auth(&harness.key)
        .json(&json!({"policy": "kids", "start": "9pm", "end": "07:00"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 422);
    let unchanged =
        Config::from_toml_str(&std::fs::read_to_string(&harness.config_path).unwrap()).unwrap();
    assert_eq!(
        unchanged.policies[0].assignments.len(),
        1,
        "a rejected schedule must not be persisted"
    );
}

/// `[[policies]]` has one owner, like `[[rules.lists]]`: the endpoints that
/// can recompile the masks.
#[tokio::test]
async fn config_post_refuses_to_write_policies() {
    let harness = start().await;
    let response = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"policies": [{"id": "kids"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/policies"),
        "the error must name the endpoint that does own it: {body}"
    );
}

// ─── Configuration ─────────────────────────────────────────────────────

#[tokio::test]
async fn config_get_returns_the_effective_tree() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/config").await;

    assert_eq!(body["dns"]["cache"]["max_entries"], 10_000);
    assert_eq!(body["api"]["port"], 8443);
    assert_eq!(body["log"]["level"], "info");
    // Secrets live in their own /config files and never enter this tree.
    assert!(body.get("api_key").is_none());
    assert!(!serde_json::to_string(&body).unwrap().contains(&harness.key));
}

#[tokio::test]
async fn a_runtime_config_change_applies_live_and_is_written_back_to_the_toml() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": {"refresh_hours_default": 6}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["applied"], true);
    assert_eq!(body["restart_required"], false);

    // Visible through the API…
    assert_eq!(
        harness.get_json("/api/v1/config").await["rules"]["refresh_hours_default"],
        6
    );
    // …and on disk, so the file always reflects the running intent.
    let on_disk = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();
    let reparsed = Config::from_toml_str(&on_disk).unwrap();
    assert_eq!(reparsed.rules.refresh_hours_default, 6);
}

/// Rule lists have exactly one runtime owner: the `/lists` endpoints, which
/// apply live and write the TOML back. A `rules.lists` array reaching
/// `POST /config` would be a second writer that never reloads the engine — and
/// whose edit the next `/lists` mutation would silently overwrite — so it is
/// refused outright.
#[tokio::test]
async fn a_config_patch_carrying_rules_lists_is_refused() {
    let harness = start().await;
    let before = harness.get_json("/api/v1/config").await;

    let response = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({
            "rules": {"lists": [{"id": "sneaky", "url": "https://example.org/l.txt"}]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);

    let body: Value = response.json().await.unwrap();
    assert!(
        body.to_string().contains("/api/v1/lists"),
        "the refusal must point at the endpoint that does work: {body}"
    );

    // Nothing persisted, nothing swapped — a refused patch is a no-op.
    assert_eq!(harness.get_json("/api/v1/config").await, before);
}

/// A patch touching other `[rules]` keys is unaffected by that refusal.
#[tokio::test]
async fn a_config_patch_under_rules_without_lists_still_applies() {
    let harness = start().await;
    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": {"refresh_hours_default": 12}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["applied"], true);
    assert_eq!(
        harness.get_json("/api/v1/config").await["rules"]["refresh_hours_default"],
        12
    );
}

/// The `[dns.cache]` section is **boot**: `DnsCache::new` reads it once, at
/// startup, so a patch persists and asks for a restart instead of claiming an
/// apply that no code performs.
#[tokio::test]
async fn a_cache_config_change_persists_and_asks_for_a_restart() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"dns": {"cache": {"max_bytes": 33_554_432u64}}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["applied"], false);
    assert_eq!(body["restart_required"], true);

    let on_disk = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();
    assert_eq!(
        Config::from_toml_str(&on_disk).unwrap().dns.cache.max_bytes,
        33_554_432
    );
}

#[tokio::test]
async fn history_retention_change_is_pushed_live_to_the_writers() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"history": {"retention_days": 90}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["applied"], true);
    assert_eq!(body["restart_required"], false);

    // The handler reached the stats port with the merged effective value —
    // this is the live apply the writers' shared retention atomic consumes on
    // the next prune (no restart).
    assert_eq!(
        *harness.stats.applied_history.lock().unwrap(),
        Some((true, 90))
    );
}

#[tokio::test]
async fn a_boot_only_config_change_asks_for_a_restart() {
    let harness = start().await;
    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"api": {"port": 9443}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["applied"], false);
    assert_eq!(body["restart_required"], true);

    let on_disk = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();
    assert_eq!(Config::from_toml_str(&on_disk).unwrap().api.port, 9443);
}

#[tokio::test]
async fn an_invalid_config_patch_is_rejected_and_nothing_is_persisted() {
    let harness = start().await;
    let before = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();

    let response = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"dns": {"cache": {"max_entrees": 1}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");

    let after = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();
    assert_eq!(before, after, "a rejected patch must not touch the file");
}

#[tokio::test]
async fn rotating_the_api_key_returns_it_once_and_invalidates_the_old_one() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config/apikey/rotate"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rotated = body["api_key"].as_str().unwrap().to_string();
    assert_ne!(rotated, harness.key);

    // The old key stops working immediately…
    assert_eq!(harness.get("/api/v1/stats").await.status(), 401);
    // …and the new one works.
    let response = harness
        .client
        .get(harness.url("/api/v1/stats"))
        .bearer_auth(&rotated)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

// ─── Events (WebSocket) ────────────────────────────────────────────────

#[tokio::test]
async fn the_event_socket_streams_a_blocked_query_end_to_end() {
    use futures_util::StreamExt;

    let harness = start().await;
    let url = format!(
        "{}/api/v1/events?token={}",
        harness.base.replace("https://", "wss://"),
        harness.key
    );

    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    let (mut socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
            .await
            .expect("the events socket must accept a ?token= upgrade");

    // Publish as the binary's fan-out task would.
    let client = IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15));
    harness.server.events().publish_query(
        fah_model::Event::dns(blocked_event(client)),
        Some("liviu-phone".to_string()),
    );

    // The periodic stats push shares the socket; take messages until the
    // query arrives.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("timed out waiting for the query event")
            .expect("socket closed")
            .expect("websocket error");

        let Ok(text) = message.into_text() else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        match event["type"].as_str() {
            Some("query") => {
                assert_eq!(event["data"]["domain"], "ads.example.com");
                assert_eq!(event["data"]["verdict"], "block");
                assert_eq!(event["data"]["rule"], "||ads.example.com^");
                assert_eq!(event["data"]["client_name"], "liviu-phone");
                break;
            }
            Some("stats") => continue,
            other => panic!("unexpected event type: {other:?}"),
        }
    }
}

#[tokio::test]
async fn the_event_socket_pushes_periodic_stats() {
    use futures_util::StreamExt;

    let harness = start().await;
    let url = format!(
        "{}/api/v1/events?token={}",
        harness.base.replace("https://", "wss://"),
        harness.key
    );
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    let (mut socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
            .await
            .unwrap();

    let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .expect("the first stats push must arrive")
        .unwrap()
        .unwrap();
    let event: Value = serde_json::from_str(&message.into_text().unwrap()).unwrap();
    assert_eq!(event["type"], "stats");
    assert_eq!(event["data"]["queries_total"], 184_233);
}

#[tokio::test]
async fn the_event_socket_rejects_a_bad_token() {
    let harness = start().await;
    let url = format!(
        "{}/api/v1/events?token=wrong",
        harness.base.replace("https://", "wss://")
    );
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));

    assert!(
        tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
            .await
            .is_err(),
        "an unauthorized upgrade must not succeed"
    );
}

/// A rustls client that accepts the self-signed appliance certificate —
/// tungstenite has no `danger_accept_invalid_certs` switch, so the
/// verification bypass is spelled out here. Test-only.
fn insecure_client_config() -> rustls::ClientConfig {
    fah_api::install_crypto_provider();

    #[derive(Debug)]
    struct AcceptAny;

    impl rustls::client::danger::ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny))
        .with_no_client_auth()
}
