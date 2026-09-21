//! End-to-end tests against a real server over real HTTPS with the
//! self-signed certificate, exercising API.md's documented contract.
//!
//! The stats/telemetry handles are fakes implementing the port traits — this
//! crate cannot depend on `fah-stats`/`fah-metrics` (L3 siblings,
//! ARCHITECTURE.md §Dependency Layering). Everything else is real: real
//! rustls, real axum routing, the real `ListManager`, the real config store.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fah_api::{
    ApiKeyStore, ApiServer, AppStateBuilder, AuthState, BucketCount, CacheClean, CacheSource,
    CacheStats, ClientCount, ClientEntry, ConfigStore, DomainCount, HistorySource, PolicyCount,
    RateLimits, StatsOverview, StatsSource, TelemetrySource,
};

const PASSWORD: &str = "correct-horse-battery-staple";
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
    /// The `(enabled, retention_days)` last pushed by `apply_history_config` —
    /// lets a test prove `POST /api/v1/config` reaches the history writers live.
    applied_history: Mutex<Option<(bool, u32)>>,
    applied_client_idle_expiry_days: Mutex<Option<u32>>,
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
                intercepted: fah_api::InterceptedHandshakes {
                    completed: 14,
                    rejected: 3,
                    last_completed: Some(SystemTime::UNIX_EPOCH),
                    last_rejected: None,
                },
            }]),
            applied_history: Mutex::new(None),
            applied_client_idle_expiry_days: Mutex::new(None),
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
        None,
        fah_model::ClientTransport::Udp,
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

    fn set_client_idle_expiry_days(&self, days: u32) {
        *self.applied_client_idle_expiry_days.lock().unwrap() = Some(days);
    }

    fn heap(&self) -> fah_model::StatsHeap {
        fah_model::StatsHeap {
            aggregates: 40_000,
            clients: 8_000,
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
    perf_calls: Mutex<Vec<bool>>,
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

    fn perf(
        &self,
        _range: HistoryRange,
        _max_points: usize,
        include_upstreams: bool,
    ) -> std::io::Result<PerfSeries> {
        self.perf_calls.lock().unwrap().push(include_upstreams);
        if self.is_empty() {
            return Ok(PerfSeries {
                samples: vec![],
                stride: 1,
            });
        }
        Ok(PerfSeries {
            samples: vec![PerfSample {
                ts: 3_600,
                allocator_committed_bytes: 0,
                list_fetch: Default::default(),
                concurrent_connections: fah_model::ConcurrentConnections { http: 3, https: 0 },
                answers_delta: fah_model::AnswerCounters {
                    servfail_synthesized: 9,
                    servfail_relayed: 4,
                    refused_relayed: 1,
                },
                rss_bytes: 55_000_000,
                peak_rss: 123_539_456,
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
                    },
                },
                minor_page_faults: 4_211_337,
                rss_anon_bytes: 38_000_000,
                rss_file_bytes: 19_000_000,
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
    /// `None` on purpose: the test binary does not install mimalloc, so the
    /// figures would be meaningless. This also exercises the absent branch —
    /// the allocator fields must serialize as `null`, never as a fabricated 0.
    /// `None` on purpose: the kernel figures below must still be served, which
    /// is exactly what an allocator swap would look like.
    fn allocator(&self) -> Option<fah_model::AllocatorStats> {
        None
    }

    fn process(&self) -> Option<fah_model::ProcessStats> {
        Some(fah_model::ProcessStats {
            peak_rss: 150_700_000,
            major_page_faults: 0,
            minor_page_faults: 4_211_337,
            ..Default::default()
        })
    }

    fn degraded(&self) -> bool {
        self.degraded
    }

    fn listeners(&self) -> fah_model::ListenerTelemetry {
        fah_model::ListenerTelemetry {
            http: Some(fah_model::ListenerCounters {
                connections: 5_120,
                requests: 5_333,
                blocked: 918,
                refused_claim: 2,
                refused_destination: 1,
                resolve_failures: 4,
                upstream_failures: 6,
                upstream_cert_failures: 0,
                client_cert_rejections: 0,
                alert_bad_certificate: 0,
                alert_certificate_unknown: 0,
                alert_access_denied: 0,
                handshakes_completed: 0,
                non_http: 3,
                non_tls: 0,
                hello_timeouts: 0,
                dropped_events: 0,
            }),
            https: None,
        }
    }

    /// Non-zero in every field the endpoint publishes, so a test asserting a
    /// field is present cannot pass on a default-constructed value.
    fn engine(&self) -> fah_model::EngineTelemetry {
        fah_model::EngineTelemetry {
            ruleset: fah_model::RulesetInfo {
                rules: 1_043_886,
                duplicates_removed: 41_207,
                compile_duration: Duration::from_millis(7_412),
            },
            counters: fah_model::EngineCounters {
                dns: fah_model::DnsCounters {
                    pass: 812_044,
                    allow: 1_201,
                    block: 96_318,
                    cache_hits: 640_119,
                    cache_misses: 269_446,
                    cache_stale: 3_187,
                    answers: fah_model::AnswerCounters {
                        servfail_synthesized: 1_204,
                        servfail_relayed: 88,
                        refused_relayed: 17,
                    },
                },
                http: fah_model::HttpCounters {
                    pass: 4_412,
                    allow: 0,
                    block: 918,
                    response_bytes: 148_223_904,
                    refused_claim: 3,
                    refused_destination: 11,
                },
                events_dropped: 7,
                dns_tcp_connections: fah_model::DnsTcpConnections {
                    active: 2,
                    peak: 9,
                    closed_oversize: 0,
                },
                dns_dot_connections: fah_model::DnsDotConnections {
                    active: 5,
                    peak: 23,
                    closed_oversize: 1,
                },
                dns_udp_inflight: fah_model::DnsUdpInflight {
                    active: 4,
                    peak: 37,
                    shed: 2,
                },
                tasks_died: 1,
                swr: fah_model::SwrCounters {
                    enqueued: 12_044,
                    deduplicated: 3_311,
                    dropped: 0,
                    completed: 8_702,
                    failed: 31,
                },
                cache_cleanup: fah_model::CacheCleanupCounters {
                    runs: 308,
                    entries_removed: 44_120,
                    bytes_freed: 9_871_232,
                    last_duration: Duration::from_micros(1_842),
                },
                lists: fah_model::ListFetchCounters {
                    bodies: 17,
                    not_modified: 3,
                    bytes_fetched: 27_580_000,
                },
            },
            latency: fah_model::LatencyTotals {
                dns: fah_model::DnsLatency {
                    block: stage(96_318, 2.114),
                    cache_hit: stage(640_119, 18.907),
                    forward: stage(269_446, 6_021.338),
                },
                http: fah_model::HttpLatency {
                    block: stage(918, 0.031),
                    forward: stage(4_412, 12.884),
                },
            },
            upstreams: vec![fah_model::UpstreamSample {
                address: "1.1.1.1:853".to_string(),
                protocol: fah_model::Protocol::Dot,
                attempts: 201_883,
                failures: 12,
                consecutive_failures: 0,
                tls_handshakes: 41,
                failure_runs: [5, 2, 0, 1],
                state: fah_model::UpstreamState::Penalized,
                penalty_round: 2,
                penalties: 9,
                penalized_seconds_total: 144,
                probes: 8,
                probe_successes: 3,
                family: Some(fah_model::AddressFamily::V4),
                rtt: fah_model::UpstreamRtt::default(),
            }],
        }
    }
}

fn stage(count: u64, sum_seconds: f64) -> fah_model::StageTotals {
    fah_model::StageTotals { count, sum_seconds }
}

// ─── Harness ───────────────────────────────────────────────────────────

#[derive(Default)]
struct FakeDns {
    seen: Mutex<Vec<(IpAddr, Vec<u8>)>>,
}

impl fah_api::DnsWireSource for FakeDns {
    fn resolve(&self, message: Vec<u8>, client: IpAddr) -> fah_api::WireResolving {
        let reply = match message.first() {
            Some(0xFF) => None,
            _ => Some([message.as_slice(), b"reply"].concat()),
        };
        self.seen.lock().unwrap().push((client, message));
        Box::pin(async move { reply })
    }
}

struct Harness {
    server: ApiServer,
    client: reqwest::Client,
    key: String,
    base: String,
    config_path: std::path::PathBuf,
    rules: Arc<ListManager>,
    stats: Arc<FakeStats>,
    history: Arc<FakeHistory>,
    auth: Arc<AuthState>,
    certs: Option<Arc<fah_api::CertStore>>,
    dns: Option<Arc<FakeDns>>,
    interception: Arc<fah_api::InterceptionStore>,
    document_path: std::path::PathBuf,
    _config_dir: tempfile::TempDir,
    data_dir: tempfile::TempDir,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.shutdown();
    }
}

struct HarnessOptions {
    tls: bool,
    degraded: bool,
    limits: RateLimits,
    certs: bool,
    doh: bool,
    interception_runtime: fah_api::InterceptionRuntime,
    address: &'static str,
    port: u16,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            tls: true,
            degraded: false,
            limits: AuthState::relaxed_limits(),
            certs: true,
            doh: true,
            interception_runtime: fah_api::InterceptionRuntime::Live,
            address: "127.0.0.1",
            port: 0,
        }
    }
}

async fn start() -> Harness {
    start_with(HarnessOptions::default()).await
}

async fn start_with(options: HarnessOptions) -> Harness {
    try_start_with(options).await.unwrap()
}

async fn try_start_with(options: HarnessOptions) -> std::io::Result<Harness> {
    let config_dir = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    let mut config = Config::default();
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
    let auth = Arc::new(
        AuthState::for_tests(config_dir.path(), data_dir.path(), PASSWORD, options.limits).unwrap(),
    );

    let tls_config = options
        .tls
        .then(|| fah_api::load_or_generate_tls(config_dir.path(), "127.0.0.1", None).unwrap());

    let stats = Arc::new(FakeStats::with_client(IpAddr::V4(Ipv4Addr::new(
        192, 168, 10, 15,
    ))));
    let history = Arc::new(FakeHistory::default());
    let certs = options
        .certs
        .then(|| Arc::new(fah_api::CertStore::open(config_dir.path()).unwrap()));
    let dns = options.doh.then(|| Arc::new(FakeDns::default()));
    let document_path = config_dir.path().join(fah_api::DOCUMENT_FILE);
    let interception = Arc::new(fah_api::InterceptionStore::new(
        Arc::new(fah_rules::interception::InterceptionState::default()),
        document_path.clone(),
        options.interception_runtime,
    ));
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
        interception: Arc::clone(&interception),
        keys: Arc::new(keys),
        auth: Arc::clone(&auth),
        certs: certs.clone(),
        doh: dns
            .clone()
            .map(|dns| dns as Arc<dyn fah_api::DnsWireSource>),
        dot: fah_api::DotListener::Listening {
            address: "[::]:853".parse().unwrap(),
        },
    };

    let server = ApiServer::bind(options.address, options.port, tls_config, state).await?;
    let base = server.base_url();

    let client = reqwest::Client::builder()
        // The certificate is self-signed by design (SECURITY.md) — this is
        // the programmatic equivalent of curl's `-k`.
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    Ok(Harness {
        server,
        client,
        key,
        base,
        config_path,
        rules,
        stats,
        history,
        auth,
        certs,
        dns,
        interception,
        document_path,
        _config_dir: config_dir,
        data_dir,
    })
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
        "/api/v1/telemetry",
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

const DNS_MESSAGE: &str = "application/dns-message";

fn content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

#[tokio::test]
async fn dns_query_answers_post_and_get_without_credentials_and_names_the_peer() {
    let harness = start().await;
    let dns = harness.dns.clone().expect("DoH is on by default");
    let message = vec![0x12, 0x34, 0x01, 0x00];

    let response = harness
        .client
        .post(harness.url("/dns-query"))
        .header("content-type", DNS_MESSAGE)
        .body(message.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(content_type(&response).as_deref(), Some(DNS_MESSAGE));
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        [message.as_slice(), b"reply"].concat()
    );

    let response = harness
        .client
        .get(harness.url("/dns-query?dns=EjQBAA"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(content_type(&response).as_deref(), Some(DNS_MESSAGE));
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        [message.as_slice(), b"reply"].concat()
    );

    let seen = dns.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for (client, seen_message) in seen.iter() {
        assert_eq!(*client, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(*seen_message, message);
    }
}

#[tokio::test]
async fn dns_query_rejects_what_rfc_8484_does_not_allow() {
    let harness = start().await;
    let post = |body: Vec<u8>, media: Option<&'static str>| {
        let mut request = harness.client.post(harness.url("/dns-query")).body(body);
        if let Some(media) = media {
            request = request.header("content-type", media);
        }
        request
    };

    for (request, status, why) in [
        (post(vec![1, 2, 3], None), 415, "no media type"),
        (
            post(vec![1, 2, 3], Some("application/json")),
            415,
            "wrong media type",
        ),
        (post(Vec::new(), Some(DNS_MESSAGE)), 400, "empty body"),
        (
            post(vec![0; 65_536], Some(DNS_MESSAGE)),
            413,
            "over the 65535-byte protocol maximum",
        ),
        (
            post(vec![0xFF], Some(DNS_MESSAGE)),
            400,
            "the pipeline had nothing to answer",
        ),
        (
            harness.client.get(harness.url("/dns-query?dns=EjQBAA==")),
            400,
            "padded base64url",
        ),
        (
            harness.client.get(harness.url("/dns-query?dns=%%%")),
            400,
            "not base64url",
        ),
        (
            harness.client.get(harness.url("/dns-query")),
            400,
            "missing dns parameter",
        ),
    ] {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), status, "{why}");
        assert_ne!(
            content_type(&response).as_deref(),
            Some(DNS_MESSAGE),
            "{why}: a rejection must never look like a DNS answer"
        );
    }
    assert!(harness.dns.as_ref().unwrap().seen.lock().unwrap().len() <= 1);
}

#[tokio::test]
async fn dns_query_is_absent_when_doh_is_disabled_and_everything_else_serves() {
    let harness = start_with(HarnessOptions {
        doh: false,
        ..HarnessOptions::default()
    })
    .await;

    let response = harness
        .client
        .get(harness.url("/dns-query?dns=EjQBAA"))
        .send()
        .await
        .unwrap();
    assert_ne!(content_type(&response).as_deref(), Some(DNS_MESSAGE));

    let response = harness
        .client
        .post(harness.url("/dns-query"))
        .header("content-type", DNS_MESSAGE)
        .body(vec![0x12, 0x34, 0x01, 0x00])
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), 200);
    assert_ne!(content_type(&response).as_deref(), Some(DNS_MESSAGE));

    assert_eq!(harness.get("/health").await.status(), 200);
    assert_eq!(harness.get("/api/v1/stats").await.status(), 200);
}

#[tokio::test]
async fn dns_query_is_absent_without_tls_because_doh_is_https_only() {
    let harness = start_with(HarnessOptions {
        tls: false,
        ..HarnessOptions::default()
    })
    .await;
    assert!(harness.base.starts_with("http://"));
    let dns = harness
        .dns
        .clone()
        .expect("the DoH port is wired; only the route must be absent");

    let response = harness
        .client
        .get(harness.url("/dns-query?dns=EjQBAA"))
        .send()
        .await
        .unwrap();
    assert_ne!(content_type(&response).as_deref(), Some(DNS_MESSAGE));

    let response = harness
        .client
        .post(harness.url("/dns-query"))
        .header("content-type", DNS_MESSAGE)
        .body(vec![0x12, 0x34, 0x01, 0x00])
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), 200);
    assert_ne!(content_type(&response).as_deref(), Some(DNS_MESSAGE));
    assert!(
        dns.seen.lock().unwrap().is_empty(),
        "a plaintext listener must never hand a query to the pipeline"
    );

    assert_eq!(harness.get("/health").await.status(), 200);
    assert_eq!(harness.get("/api/v1/stats").await.status(), 200);
}

#[tokio::test]
async fn dns_query_is_the_only_route_outside_the_admin_exemptions() {
    let harness = start().await;
    for path in ["/api/v1/dns-query", "/api/dns-query", "/dns-query/"] {
        let response = harness
            .client
            .get(harness.url(&format!("{path}?dns=EjQBAA")))
            .send()
            .await
            .unwrap();
        assert_ne!(
            content_type(&response).as_deref(),
            Some(DNS_MESSAGE),
            "{path} must not answer DNS"
        );
        if path.starts_with("/api/") {
            assert_eq!(response.status(), 401, "{path} stays behind auth");
        }
    }
}

#[tokio::test]
async fn health_is_public_and_every_api_route_is_not() {
    let harness = start().await;

    let health = harness
        .client
        .get(harness.url("/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);

    for path in ["/api/v1/telemetry", "/api/v1/stats", "/api/v1/cache"] {
        let response = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 401, "{path} needs the key");
        assert_eq!(harness.get(path).await.status(), 200, "{path} with the key");
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_json_not_found() {
    let harness = start().await;
    let response = harness.get("/api/v1/nope").await;
    assert_eq!(response.status(), 404);

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "not_found");

    let anonymous = harness
        .client
        .get(harness.url("/api/v1/nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);
    assert!(
        anonymous.headers().get("vary").is_none(),
        "the API surface must not be answered by the static service"
    );
}

#[tokio::test]
async fn every_path_under_api_is_json_never_the_shell() {
    let harness = start().await;

    for path in ["/api", "/api/", "/api/v2/stats", "/api/v1/nope"] {
        let response = harness.get(path).await;
        assert_eq!(response.status(), 404, "{path}");
        assert!(
            response.headers().get("vary").is_none(),
            "{path} was answered by the static service"
        );

        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "not_found", "{path}");

        let anonymous = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_eq!(anonymous.status(), 401, "{path} must stay behind the key");
    }
}

#[tokio::test]
async fn the_static_paths_are_outside_the_api_key_boundary() {
    let harness = start().await;

    for path in ["/", "/assets/app.deadbeef.js", "/lists/oisd-basic"] {
        let response = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_ne!(response.status(), 401, "{path} must not require the key");
        assert_eq!(
            response.headers().get("vary").and_then(|v| v.to_str().ok()),
            Some("accept-encoding"),
            "{path} must be answered by the static service"
        );
        assert_ne!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "{path} must not be answered by the API router"
        );

        let body = response.text().await.unwrap();
        assert!(
            !body.contains("\"error\""),
            "{path} was answered by the API, not the static service: {body}"
        );
    }
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
async fn metrics_is_gone() {
    let harness = start().await;
    assert_eq!(harness.get("/metrics").await.status(), 404);
}

// ─── Statistics ────────────────────────────────────────────────────────

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

// ─── Telemetry ─────────────────────────────────────────────────────────

#[tokio::test]
async fn there_is_no_query_log_endpoint() {
    let harness = start().await;
    assert_eq!(harness.get("/api/v1/queries").await.status(), 404);
}

#[tokio::test]
async fn telemetry_matches_the_documented_shape() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/telemetry").await;

    assert!(body["process"]["version"].is_string());
    assert!(body["process"]["uptime_seconds"].is_u64());

    assert_eq!(body["ruleset"]["rules"], 1_043_886);
    assert_eq!(body["ruleset"]["duplicates_removed"], 41_207);
    assert_eq!(body["ruleset"]["compile_duration_seconds"], 7.412);
    assert!(
        body["ruleset"].get("heap_bytes").is_none(),
        "ruleset size has one home, and it is memory.ruleset_bytes"
    );

    assert_eq!(body["counters"]["dns"]["block"], 96_318);
    assert_eq!(body["counters"]["dns"]["cache_stale"], 3_187);
    assert_eq!(
        body["counters"]["dns"]["answers"]["servfail_synthesized"],
        1_204
    );
    assert_eq!(body["counters"]["dns"]["answers"]["servfail_relayed"], 88);
    assert_eq!(body["counters"]["dns"]["answers"]["refused_relayed"], 17);
    assert_eq!(body["counters"]["http"]["response_bytes"], 148_223_904);
    assert_eq!(body["counters"]["http"]["refused_claim"], 3);
    assert_eq!(body["counters"]["http"]["refused_destination"], 11);
    assert_eq!(body["counters"]["events_dropped"], 7);
    assert_eq!(body["counters"]["swr"]["failed"], 31);
    assert_eq!(
        body["counters"]["cache_cleanup"]["last_duration_micros"],
        1_842
    );
    assert_eq!(body["counters"]["dns_tcp_connections"]["active"], 2);
    assert_eq!(body["counters"]["dns_tcp_connections"]["peak"], 9);
    assert_eq!(
        body["counters"]["dns_tcp_connections"]["closed_oversize"],
        0
    );
    assert_eq!(body["counters"]["dns_dot_connections"]["active"], 5);
    assert_eq!(body["counters"]["dns_dot_connections"]["peak"], 23);
    assert_eq!(
        body["counters"]["dns_dot_connections"]["closed_oversize"],
        1
    );
    assert_eq!(body["counters"]["dns_udp_inflight"]["active"], 4);
    assert_eq!(body["counters"]["dns_udp_inflight"]["peak"], 37);
    assert_eq!(body["counters"]["dns_udp_inflight"]["shed"], 2);
    assert_eq!(body["counters"]["tasks_died"], 1);

    let upstream = &body["upstreams"][0];
    assert_eq!(upstream["address"], "1.1.1.1:853");
    assert_eq!(upstream["protocol"], "dot");
    assert_eq!(upstream["attempts"], 201_883);
}

/// Both figures, never a pre-divided average: a lifetime mean flattens within
/// hours of uptime, so the endpoint hands over the numerator and denominator
/// and lets the caller delta them.
#[tokio::test]
async fn every_latency_stage_carries_count_and_sum_but_no_average() {
    let harness = start().await;
    let latency = harness.get_json("/api/v1/telemetry").await["latency"].clone();

    for (protocol, stages) in [
        ("dns", &["block", "cache_hit", "forward"][..]),
        ("http", &["block", "forward"][..]),
    ] {
        for stage in stages {
            let entry = &latency[protocol][stage];
            assert!(entry["count"].is_u64(), "{protocol}.{stage} count");
            assert!(entry["sum_seconds"].is_f64(), "{protocol}.{stage} sum");
            assert!(
                entry.get("mean_seconds").is_none() && entry.get("avg").is_none(),
                "{protocol}.{stage} must not serve a lifetime average"
            );
        }
    }
    assert_eq!(latency["dns"]["forward"]["count"], 269_446);
    assert_eq!(latency["http"]["block"]["sum_seconds"], 0.031);
}

/// The residual is `process_rss - accounted`, and it moves for two unrelated
/// reasons: memory the allocator holds, or file-backed pages charged for
/// reading `/data`. These two keys are the only thing that separates them, so
/// they must be on the stable surface rather than behind `/debug`. Served as
/// `null` where the kernel does not report the split — never as 0, which would
/// chart as a process with no heap.
#[tokio::test]
async fn the_memory_block_splits_rss_into_anon_and_file() {
    let harness = start().await;
    let memory = harness.get_json("/api/v1/telemetry").await["memory"].clone();

    for key in ["process_rss_anon", "process_rss_file"] {
        assert!(
            memory.get(key).is_some(),
            "{key} must be present, null or not"
        );
    }
}

/// The producer boundary, asserted structurally: `/debug/memory` is the
/// telemetry memory block **plus exactly the two allocator figures**. If a
/// future allocator field lands on the stable surface, this fails.
#[tokio::test]
async fn debug_memory_is_the_telemetry_block_plus_only_allocator_internals() {
    let harness = start().await;
    let telemetry = harness.get_json("/api/v1/telemetry").await;
    let debug = harness.get_json("/api/v1/debug/memory").await;

    let memory = telemetry["memory"].as_object().expect("memory object");
    let debug = debug.as_object().expect("debug/memory object");

    for key in memory.keys() {
        assert!(debug.contains_key(key), "/debug/memory dropped {key}");
    }
    let extra: Vec<_> = debug
        .keys()
        .filter(|key| !memory.contains_key(*key))
        .cloned()
        .collect();
    assert_eq!(
        extra,
        vec![
            "allocator_committed_bytes".to_string(),
            "allocator_committed_peak_bytes".to_string()
        ],
        "only allocator-specific figures may be debug-only"
    );
}

/// The reason the two live behind separate ports. `FakeTelemetry::allocator`
/// returns `None` — what swapping mimalloc out looks like — and the kernel
/// figures must still be served, because `/telemetry` promises them. A single
/// shared `Option` nulled all three here.
#[tokio::test]
async fn the_kernel_figures_survive_an_allocator_that_reports_nothing() {
    let harness = start().await;
    let memory = harness.get_json("/api/v1/telemetry").await["memory"].clone();

    assert_eq!(memory["process_peak_rss"], 150_700_000u64);
    assert_eq!(memory["major_page_faults"], 0);
    assert_eq!(memory["minor_page_faults"], 4_211_337u64);

    let debug = harness.get_json("/api/v1/debug/memory").await;
    assert!(
        debug["allocator_committed_bytes"].is_null(),
        "the allocator reported nothing, so only its own fields may be null"
    );
}

/// `/telemetry.cache` must be derived from the same port snapshot `/cache` is,
/// so the two can never report different numbers for the same instant.
#[tokio::test]
async fn the_telemetry_cache_block_equals_the_cache_endpoint() {
    let harness = start().await;
    let telemetry = harness.get_json("/api/v1/telemetry").await;
    let cache = harness.get_json("/api/v1/cache").await;

    assert_eq!(telemetry["cache"], cache);
}

/// Proves `collect()` gathered RSS and the components in one pass: if they came
/// from two reads the identity would only hold by luck.
#[tokio::test]
async fn the_residual_is_consistent_within_a_single_response() {
    let harness = start().await;
    let memory = harness.get_json("/api/v1/telemetry").await["memory"].clone();

    let accounted = memory["accounted_bytes"].as_u64().expect("accounted");
    match memory["process_rss"].as_u64() {
        Some(rss) => assert_eq!(
            memory["residual_bytes"].as_u64().expect("residual"),
            rss.saturating_sub(accounted)
        ),
        // Off Linux there is no procfs, so the residual is not computable.
        None => assert!(memory["residual_bytes"].is_null()),
    }
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
    assert_eq!(item["peak_rss"], 123_539_456u64);
    assert_eq!(item["qps"], 12.5);
    assert_eq!(item["queries_delta"], 750);
    assert_eq!(item["cache"]["entries"], 10_000);
    assert_eq!(item["latency"]["forward_p99"], 0.05);
    assert!(item["upstreams"].is_array());
    assert_eq!(item["answers_delta"]["servfail_synthesized"], 9);
    assert_eq!(item["answers_delta"]["servfail_relayed"], 4);
    assert_eq!(item["answers_delta"]["refused_relayed"], 1);

    // `fields` drops the keys it did not name — absent, not null.
    let body = harness
        .get_json("/api/v1/history/perf?fields=rss_bytes,cache")
        .await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["cache", "rss_bytes", "ts"]);

    // The peak is selectable on its own, and does not drag its instantaneous
    // sibling in — a chart of the compile peak wants one series, not two.
    let body = harness
        .get_json("/api/v1/history/perf?fields=peak_rss")
        .await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["peak_rss", "ts"]);

    let body = harness
        .get_json("/api/v1/history/perf?fields=answers_delta")
        .await;
    let item = body["items"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["answers_delta", "ts"]);
    assert_eq!(item["answers_delta"]["servfail_synthesized"], 9);
}

#[tokio::test]
async fn history_perf_reads_upstream_rows_only_when_the_fields_ask_for_them() {
    let harness = start().await;

    harness.get_json("/api/v1/history/perf").await;
    harness
        .get_json("/api/v1/history/perf?fields=rss_bytes,cache")
        .await;
    harness
        .get_json("/api/v1/history/perf?fields=upstreams")
        .await;
    harness
        .get_json("/api/v1/history/perf?fields=rss_bytes,upstreams")
        .await;

    let calls = harness.history.perf_calls.lock().unwrap();
    assert_eq!(*calls, [true, false, true, true]);
}

/// A typo must name every accepted key back, or the caller cannot discover the
/// one they wanted.
#[tokio::test]
async fn history_perf_rejects_an_unknown_field_and_lists_the_accepted_set() {
    let harness = start().await;

    let response = harness
        .get("/api/v1/history/perf?fields=rss_bytes,nonsense")
        .await;
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    let message = body["error"]["message"].as_str().expect("message");
    for name in [
        "rss_bytes",
        "peak_rss",
        "memory",
        "minor_page_faults",
        "rss_anon_bytes",
        "rss_file_bytes",
        "answers_delta",
    ] {
        assert!(message.contains(name), "{message:?} must list {name}");
    }
}

#[tokio::test]
async fn history_perf_derives_the_residual_from_each_row() {
    let harness = start().await;

    let body = harness.get_json("/api/v1/history/perf").await;
    let memory = &body["items"][0]["memory"];
    assert_eq!(memory["ruleset_bytes"], 23_000_000u64);
    assert_eq!(memory["stats_clients_bytes"], 500_000);
    assert_eq!(memory["accounted_bytes"], 29_500_000u64);
    // 55,000,000 RSS − 29,500,000 accounted, computed on read: the row stores
    // neither the residual nor a second RSS.
    assert_eq!(memory["residual_bytes"], 25_500_000u64);
    assert!(memory.get("process_rss").is_none());
    assert_eq!(body["items"][0]["minor_page_faults"], 4_211_337u64);
    // The split is what says whether the residual above is heap the
    // allocator holds or page cache charged for reading /data.
    assert_eq!(body["items"][0]["rss_anon_bytes"], 38_000_000u64);
    assert_eq!(body["items"][0]["rss_file_bytes"], 19_000_000u64);

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
            + live["stats_clients_bytes"].as_u64().unwrap(),
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

    // `FakeTelemetry::allocator` returns `None`, so both allocator fields must
    // be present and `null` — never absent (a client cannot tell a renamed
    // field from an unavailable one) and never `0`, which would chart as a real
    // measurement of an allocator holding nothing (see `crates/fastadhunter/src/allocator.rs`).
    for field in [
        "allocator_committed_bytes",
        "allocator_committed_peak_bytes",
    ] {
        let value = body
            .get(field)
            .unwrap_or_else(|| panic!("{field} missing from /debug/memory"));
        assert!(
            value.is_null(),
            "{field} must be null when the allocator cannot report, not {value:?}"
        );
    }
    // The kernel figures come from `getrusage`, not the allocator, so they are
    // unaffected by that `None` — asserted here as well as on `/telemetry`
    // because `/debug/memory` embeds the same block.
    assert_eq!(body["process_peak_rss"], 150_700_000u64);
    assert_eq!(body["minor_page_faults"], 4_211_337u64);
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
    assert_eq!(item["intercepted"]["completed"], 14);
    assert_eq!(item["intercepted"]["rejected"], 3);
    assert_eq!(
        item["intercepted"]["last_completed"],
        "1970-01-01T00:00:00Z"
    );
    assert!(item["intercepted"]["last_rejected"].is_null());
    assert_eq!(item["policy"], "default");
    assert!(
        item.get("assignment_source").is_none(),
        "an unassigned client carries no assignment source: {item}"
    );

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
    assert_eq!(
        updated["policy"], "default",
        "the naming response carries the same policy fields as the list: {updated}"
    );
    assert!(
        updated.get("assignment_source").is_none(),
        "an unassigned client carries no assignment source here either: {updated}"
    );

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
async fn clients_narrow_to_one_address_family_on_request() {
    let harness = start().await;
    harness.stats.clients.lock().unwrap().push(ClientEntry {
        ip: "2001:db8::15".parse().unwrap(),
        name: None,
        first_seen: SystemTime::UNIX_EPOCH,
        last_seen: SystemTime::UNIX_EPOCH,
        queries_24h: 1,
        blocked_24h: 0,
        intercepted: fah_api::InterceptedHandshakes::default(),
    });

    let listed = |body: Value| -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["ip"].as_str().unwrap().to_string())
            .collect()
    };

    assert_eq!(
        listed(harness.get_json("/api/v1/clients").await),
        ["192.168.10.15", "2001:db8::15"]
    );
    assert_eq!(
        listed(harness.get_json("/api/v1/clients?family=v4").await),
        ["192.168.10.15"]
    );
    assert_eq!(
        listed(harness.get_json("/api/v1/clients?family=v6").await),
        ["2001:db8::15"]
    );

    let rejected = harness.get("/api/v1/clients?family=ipv4").await;
    assert_eq!(rejected.status().as_u16(), 400);
}

#[tokio::test]
async fn clients_narrow_to_the_recently_seen_on_request() {
    let harness = start().await;
    harness.stats.clients.lock().unwrap().push(ClientEntry {
        ip: "192.168.10.16".parse().unwrap(),
        name: None,
        first_seen: SystemTime::now(),
        last_seen: SystemTime::now(),
        queries_24h: 1,
        blocked_24h: 0,
        intercepted: fah_api::InterceptedHandshakes::default(),
    });

    let listed = |body: Value| -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["ip"].as_str().unwrap().to_string())
            .collect()
    };

    assert_eq!(
        listed(harness.get_json("/api/v1/clients").await),
        ["192.168.10.15", "192.168.10.16"]
    );
    assert_eq!(
        listed(harness.get_json("/api/v1/clients?seen_within=24h").await),
        ["192.168.10.16"]
    );
    assert_eq!(
        listed(
            harness
                .get_json("/api/v1/clients?family=v4&seen_within=7d")
                .await
        ),
        ["192.168.10.16"]
    );

    let rejected = harness.get("/api/v1/clients?seen_within=1y").await;
    assert_eq!(rejected.status().as_u16(), 400);
}

#[tokio::test]
async fn clients_carry_the_in_force_policy_and_agree_with_the_per_client_endpoint() {
    let harness = start().await;

    let direct: IpAddr = "192.168.10.15".parse().unwrap();
    let by_subnet: IpAddr = "10.1.0.5".parse().unwrap();
    let by_name: IpAddr = "172.16.0.7".parse().unwrap();
    let unassigned: IpAddr = "203.0.113.9".parse().unwrap();

    for ip in [by_subnet, by_name, unassigned] {
        harness.stats.clients.lock().unwrap().push(ClientEntry {
            ip,
            name: None,
            first_seen: SystemTime::UNIX_EPOCH,
            last_seen: SystemTime::UNIX_EPOCH,
            queries_24h: 1,
            blocked_24h: 0,
            intercepted: fah_api::InterceptedHandshakes::default(),
        });
    }

    let response = harness
        .client
        .put(harness.url(&format!("/api/v1/clients/{by_name}")))
        .bearer_auth(&harness.key)
        .json(&json!({"name": "guest-tv"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        200,
        "a name assignment resolves only once the client carries that name"
    );

    for (id, client) in [
        ("kids", direct.to_string()),
        ("lan", "10.1.0.0/24".to_string()),
        ("guests", "guest-tv".to_string()),
    ] {
        let response = harness
            .client
            .post(harness.url("/api/v1/policies"))
            .bearer_auth(&harness.key)
            .json(&json!({
                "id": id,
                "lists": [],
                "assignments": [{"client": client}],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201, "creating the {id} policy");
    }

    let listed: HashMap<String, Value> = harness.get_json("/api/v1/clients").await["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["ip"].as_str().unwrap().to_string(), item.clone()))
        .collect();

    for (ip, policy, source) in [
        (direct, "kids", Some("direct")),
        (by_subnet, "lan", None),
        (by_name, "guests", None),
        (unassigned, "default", None),
    ] {
        let item = &listed[&ip.to_string()];
        assert_eq!(item["policy"], policy, "the policy in force for {ip}");
        assert_eq!(
            item.get("assignment_source").and_then(Value::as_str),
            source,
            "the assignment source for {ip}: {item}"
        );

        let per_client = harness
            .get_json(&format!("/api/v1/clients/{ip}/policy"))
            .await;
        assert_eq!(
            item["policy"], per_client["policy"],
            "the two endpoints must agree on the policy for {ip}"
        );
        assert_eq!(
            item.get("assignment_source").is_some(),
            per_client.get("assignment").is_some(),
            "the two endpoints must agree on what is direct for {ip}"
        );
    }
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
        json!({"path": "lists/../../outside.txt"}),
        json!({"path": "/config/auth-hash"}),
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
async fn a_rooted_path_inside_the_data_dir_is_accepted() {
    let harness = start().await;
    let file = harness.data_dir.path().join("srcs").join("local.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "ads.example.com\n").unwrap();

    let response = harness
        .client
        .post(harness.url("/api/v1/lists"))
        .bearer_auth(&harness.key)
        .json(&json!({"path": file.to_str().unwrap(), "id": "local"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 201, "{body}");
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
    let list_path = harness.data_dir.path().join("local.txt");
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
    let data = harness.data_dir.path();
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
        // Both conditions, not just the rule count: list `b` alone also
        // compiles to 3 rules with 0 duplicates, so waiting on `compiled_rules`
        // by itself can catch the single-list compile between the two refreshes.
        if body["compiled_rules"] == 3 && body["duplicates_removed"] == 2 {
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
    let data = harness.data_dir.path();
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
async fn user_rules_422_line_numbers_index_the_document_as_sent() {
    let harness = start().await;
    let response = harness
        .client
        .put(harness.url("/api/v1/rules/user"))
        .bearer_auth(&harness.key)
        .json(&json!({"rules": ["||dup.example^", "||dup.example^", "||^"]}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("line 3"), "got: {message}");
    assert!(!message.contains("line 2"), "got: {message}");

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

#[tokio::test]
async fn a_refresh_interval_patch_reaches_the_scheduler_and_not_only_the_report() {
    let harness = start().await;
    assert_eq!(
        harness.rules.default_refresh_hours(),
        24,
        "the harness starts on the documented default"
    );

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

    assert_eq!(
        harness.rules.default_refresh_hours(),
        6,
        "the patch answered applied-without-restart, so the value the scheduler reads has to \
         be the new one. GET /config and GET /lists report it straight off the config store, \
         so they answer 6 whether or not the scheduler ever hears about it — this is the only \
         assertion here that can tell the two apart"
    );
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
async fn client_idle_expiry_change_is_pushed_live_to_the_registry() {
    let harness = start().await;

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"stats": {"client_idle_expiry_days": 30}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["applied"], true);
    assert_eq!(body["restart_required"], false);
    assert_eq!(
        *harness
            .stats
            .applied_client_idle_expiry_days
            .lock()
            .unwrap(),
        Some(30)
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

type EventSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_events(harness: &Harness) -> EventSocket {
    let url = format!(
        "{}/api/v1/events?token={}",
        harness.base.replace("https://", "wss://"),
        harness.key
    );
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
        .await
        .expect("the events socket must accept a ?token= upgrade")
        .0
}

async fn subscribe(socket: &mut EventSocket, frame: &str) {
    use futures_util::SinkExt;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(frame.into()))
        .await
        .unwrap();
}

async fn wait_for_query_subscribers(harness: &Harness, expected: bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while harness.server.events().has_query_subscribers() != expected {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the server never reached has_query_subscribers() == {expected}"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn the_default_subscription_delivers_every_event_kind() {
    use futures_util::StreamExt;

    let harness = start().await;
    let mut socket = connect_events(&harness).await;

    let events = harness.server.events();
    events.publish_query(
        fah_model::Event::dns(blocked_event(IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15)))),
        None,
    );
    events.publish(fah_api::Event::ConfigChanged {
        restart_required: true,
    });
    events.publish(fah_api::Event::ListRefreshed {
        id: "oisd-basic".to_string(),
        status: "ok",
    });

    let mut seen = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while seen.len() < 4 {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("timed out before all four event kinds arrived")
            .expect("socket closed")
            .expect("websocket error");
        let Ok(text) = message.into_text() else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        seen.insert(event["type"].as_str().unwrap().to_string(), event);
    }

    assert_eq!(
        seen.keys().cloned().collect::<Vec<_>>(),
        vec![
            "config_changed".to_string(),
            "list_refreshed".to_string(),
            "query".to_string(),
            "stats".to_string()
        ],
        "a client that sends nothing still receives everything"
    );
}

#[tokio::test]
async fn a_stats_only_socket_receives_no_query_and_costs_the_engine_nothing() {
    use futures_util::StreamExt;

    let harness = start().await;
    let mut socket = connect_events(&harness).await;
    assert!(
        harness.server.events().has_query_subscribers(),
        "a fresh socket is counted from the instant it connects"
    );

    subscribe(&mut socket, r#"{"subscribe":["stats"]}"#).await;
    wait_for_query_subscribers(&harness, false).await;

    let events = harness.server.events();
    let burst = IpAddr::V4(Ipv4Addr::new(192, 168, 10, 15));
    for _ in 0..50 {
        events.publish_query(fah_model::Event::dns(blocked_event(burst)), None);
    }
    events.publish(fah_api::Event::ConfigChanged {
        restart_required: true,
    });

    subscribe(&mut socket, r#"{"subscribe":["query","stats"]}"#).await;
    wait_for_query_subscribers(&harness, true).await;
    let marker = IpAddr::V4(Ipv4Addr::new(10, 9, 9, 9));
    events.publish_query(fah_model::Event::dns(blocked_event(marker)), None);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("timed out waiting for the query that follows the widened subscription")
            .expect("socket closed")
            .expect("websocket error");
        let Ok(text) = message.into_text() else {
            continue;
        };
        let event: Value = serde_json::from_str(&text).unwrap();
        match event["type"].as_str() {
            Some("query") => {
                assert_eq!(
                    event["data"]["client"], "10.9.9.9",
                    "the burst published while stats-only must never reach this socket"
                );
                break;
            }
            Some("stats") => continue,
            other => panic!("a stats-only socket received {other:?}"),
        }
    }
}

#[tokio::test]
async fn an_unusable_subscription_frame_leaves_the_previous_set_standing() {
    let harness = start().await;
    let mut socket = connect_events(&harness).await;

    subscribe(&mut socket, r#"{"subscribe":["stats"]}"#).await;
    wait_for_query_subscribers(&harness, false).await;

    subscribe(&mut socket, r#"{"subscribe":["query","nonsense"]}"#).await;
    subscribe(&mut socket, "not json at all").await;
    assert!(
        !harness.server.events().has_query_subscribers(),
        "neither unusable frame may be partially applied: the previous \
         stats-only set stands until a usable frame replaces it"
    );

    subscribe(&mut socket, r#"{"subscribe":["query","stats"]}"#).await;
    wait_for_query_subscribers(&harness, true).await;
    assert!(
        harness.server.events().has_query_subscribers(),
        "the socket survived both unusable frames and still applies a valid one"
    );
}

#[tokio::test]
async fn a_socket_that_does_not_want_stats_is_kept_alive_by_a_ping() {
    use futures_util::StreamExt;

    let harness = start().await;
    let mut socket = connect_events(&harness).await;
    subscribe(&mut socket, r#"{"subscribe":["query"]}"#).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("a socket without stats must still be fed a ping")
            .expect("socket closed")
            .expect("websocket error");
        if matches!(message, tokio_tungstenite::tungstenite::Message::Ping(_)) {
            break;
        }
    }
}

#[tokio::test]
async fn an_oversized_frame_is_refused_and_releases_the_subscription() {
    use futures_util::StreamExt;

    let harness = start().await;
    let mut socket = connect_events(&harness).await;
    assert!(
        harness.server.events().has_query_subscribers(),
        "the default subscription counts against the engine gate"
    );

    let oversized = format!(r#"{{"subscribe":["{}"]}}"#, "q".repeat(2 * 4096));
    assert!(
        oversized.len() > 2 * 4096,
        "one unfragmented frame well past the 4096-byte cap"
    );
    subscribe(&mut socket, &oversized).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("the server must not leave an oversized frame unanswered");
        match message {
            None | Some(Err(_)) => break,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
            Some(Ok(_)) => continue,
        }
    }

    wait_for_query_subscribers(&harness, false).await;
}

#[tokio::test]
async fn a_frame_header_declaring_a_huge_payload_is_refused_before_the_payload() {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let harness = start().await;
    let mut socket = connect_events(&harness).await;

    let mut header = vec![0x81u8, 0xFF];
    header.extend_from_slice(&(8u64 * 1024 * 1024).to_be_bytes());
    header.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
    let stream = socket.get_mut();
    stream.write_all(&header).await.unwrap();
    stream.flush().await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect(
                "a header declaring 8 MiB must be refused from the header alone, \
                 never buffered while the payload is awaited",
            );
        match message {
            None | Some(Err(_)) => break,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
            Some(Ok(_)) => continue,
        }
    }

    wait_for_query_subscribers(&harness, false).await;
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

fn session_cookie(response: &reqwest::Response) -> String {
    let value = response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("this response must set the session cookie")
        .to_str()
        .unwrap();
    value.split(';').next().unwrap().trim().to_string()
}

async fn login(harness: &Harness, password: &str) -> reqwest::Response {
    harness
        .client
        .post(harness.url("/api/v1/auth/login"))
        .json(&json!({ "password": password }))
        .send()
        .await
        .unwrap()
}

async fn login_cookie(harness: &Harness) -> String {
    let response = login(harness, PASSWORD).await;
    assert_eq!(response.status(), 204);
    session_cookie(&response)
}

fn cookie_for(token: &str) -> String {
    format!("__Host-fah_session={token}")
}

async fn get_with_cookie(harness: &Harness, path: &str, cookie: &str) -> reqwest::Response {
    harness
        .client
        .get(harness.url(path))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .unwrap()
}

fn secret_file(harness: &Harness) -> String {
    std::fs::read_to_string(harness.data_dir.path().join("session-secret")).unwrap()
}

#[tokio::test]
async fn login_returns_an_empty_204_and_sets_the_host_prefixed_cookie() {
    let harness = start().await;
    let response = login(&harness, PASSWORD).await;

    assert_eq!(response.status(), 204);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let cookie = response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    for attribute in ["Secure", "HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.contains(attribute), "{cookie}");
    }
    assert!(cookie.starts_with("__Host-fah_session="));

    let body = response.bytes().await.unwrap();
    assert!(body.is_empty(), "the token must never travel in a body");
}

#[tokio::test]
async fn a_wrong_password_is_byte_identical_to_a_password_whose_hash_was_replaced() {
    let harness = start().await;

    let wrong = login(&harness, "not-the-password").await;
    let wrong_status = wrong.status();
    let wrong_headers = wrong.headers().clone();
    let wrong_body = wrong.text().await.unwrap();

    assert_eq!(wrong_status, 401);
    assert!(wrong_headers.get(reqwest::header::SET_COOKIE).is_none());
    assert_eq!(wrong_headers.get("cache-control").unwrap(), "no-store");

    let permit = harness.auth.try_argon2_permit().unwrap();
    let replacement = AuthState::hash_for_tests(permit, "a-different-password")
        .await
        .unwrap();
    harness.auth.replace_password(replacement).await.unwrap();

    let stale = login(&harness, PASSWORD).await;
    let stale_status = stale.status();
    let stale_headers = stale.headers().clone();
    let stale_body = stale.text().await.unwrap();

    assert_eq!(stale_status, wrong_status);
    assert_eq!(stale_body, wrong_body);
    assert_eq!(comparable(&stale_headers), comparable(&wrong_headers));
}

fn comparable(headers: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = headers
        .iter()
        .filter(|(name, _)| name.as_str() != "date")
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap().to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

#[tokio::test]
async fn a_session_cookie_authenticates_the_rest_surface() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;

    let response = get_with_cookie(&harness, "/api/v1/stats", &cookie).await;
    assert_eq!(response.status(), 200);

    let bare = harness
        .client
        .get(harness.url("/api/v1/stats"))
        .send()
        .await
        .unwrap();
    assert_eq!(bare.status(), 401);
}

#[tokio::test]
async fn expiry_is_enforced_from_the_token_not_the_cookie_attributes() {
    let harness = start().await;
    let expired = harness
        .auth
        .mint_for_tests(1, SystemTime::now() - Duration::from_secs(1))
        .await;

    let response = harness
        .client
        .get(harness.url("/api/v1/stats"))
        .header(reqwest::header::COOKIE, cookie_for(&expired))
        .header("expires", "Tue, 01 Jan 2999 00:00:00 GMT")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn a_tampered_or_unknown_version_token_is_rejected() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;

    let mut tampered = cookie.clone().into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'a' { b'b' } else { b'a' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert_eq!(
        get_with_cookie(&harness, "/api/v1/stats", &tampered)
            .await
            .status(),
        401
    );

    let version_two = harness
        .auth
        .mint_for_tests(2, SystemTime::now() + Duration::from_secs(3600))
        .await;
    let response = get_with_cookie(&harness, "/api/v1/stats", &cookie_for(&version_two)).await;
    assert_eq!(response.status(), 401);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");

    assert_eq!(
        get_with_cookie(&harness, "/api/v1/stats", &cookie)
            .await
            .status(),
        200,
        "the untouched cookie still works"
    );
}

#[tokio::test]
async fn logout_clears_the_cookie_and_logout_all_rotates_the_secret() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;

    let response = harness
        .client
        .post(harness.url("/api/v1/auth/logout"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    assert!(session_cookie(&response).ends_with('='));
    assert_eq!(
        get_with_cookie(&harness, "/api/v1/stats", &cookie)
            .await
            .status(),
        200,
        "logout is client-side: the token stays valid until expiry"
    );

    let before = secret_file(&harness);
    let response = harness
        .client
        .post(harness.url("/api/v1/auth/logout-all"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    assert_ne!(secret_file(&harness), before);
    assert_eq!(
        get_with_cookie(&harness, "/api/v1/stats", &cookie)
            .await
            .status(),
        401,
        "logout-all is the revocation"
    );
}

#[tokio::test]
async fn a_password_change_requires_the_current_password_and_kills_every_session() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;
    let secret_before = secret_file(&harness);

    let wrong = harness
        .client
        .post(harness.url("/api/v1/auth/password"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({
            "current_password": "not-the-password",
            "new_password": "a-perfectly-long-replacement",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    assert_eq!(wrong.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(
        secret_file(&harness),
        secret_before,
        "a failed change rotates nothing"
    );

    let short = harness
        .client
        .post(harness.url("/api/v1/auth/password"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({ "current_password": PASSWORD, "new_password": "short" }))
        .send()
        .await
        .unwrap();
    assert_eq!(short.status(), 422);
    assert_eq!(short.headers().get("cache-control").unwrap(), "no-store");
    let body: Value = short.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    assert_eq!(secret_file(&harness), secret_before);

    let changed = harness
        .client
        .post(harness.url("/api/v1/auth/password"))
        .header(reqwest::header::COOKIE, &cookie)
        .json(&json!({
            "current_password": PASSWORD,
            "new_password": "a-perfectly-long-replacement",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(changed.status(), 204);
    assert!(session_cookie(&changed).ends_with('='));
    assert_ne!(secret_file(&harness), secret_before);

    assert_eq!(
        get_with_cookie(&harness, "/api/v1/stats", &cookie)
            .await
            .status(),
        401,
        "the caller's own session dies too"
    );
    assert_eq!(login(&harness, PASSWORD).await.status(), 401);
    assert_eq!(
        login(&harness, "a-perfectly-long-replacement")
            .await
            .status(),
        204
    );
}

#[tokio::test]
async fn no_auth_response_ever_carries_the_password_or_the_secret() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;
    let secret = secret_file(&harness);

    let mut bodies = Vec::new();
    for (path, payload) in [
        ("/api/v1/auth/login", json!({ "password": PASSWORD })),
        ("/api/v1/auth/login", json!({ "password": "wrong" })),
        ("/api/v1/auth/logout", json!({})),
        (
            "/api/v1/auth/password",
            json!({ "current_password": "wrong", "new_password": "a-long-replacement" }),
        ),
    ] {
        let response = harness
            .client
            .post(harness.url(path))
            .header(reqwest::header::COOKIE, &cookie)
            .json(&payload)
            .send()
            .await
            .unwrap();
        bodies.push(response.text().await.unwrap());
    }
    bodies.push(harness.get("/api/v1/config").await.text().await.unwrap());

    for body in bodies {
        assert!(!body.contains(PASSWORD), "{body}");
        assert!(!body.contains(secret.trim()), "{body}");
        assert!(!body.contains("$argon2"), "{body}");
    }
}

#[tokio::test]
async fn get_config_omits_auth_material_and_post_config_refuses_it() {
    let harness = start().await;

    let body = harness.get("/api/v1/config").await.text().await.unwrap();
    assert!(!body.contains("$argon2"), "{body}");
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert!(parsed.get("auth").is_none());

    let response = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({ "auth": { "password_hash": "$argon2id$v=19$m=19456,t=2,p=1$x$y" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/auth/password"),
        "the guard names the endpoint that owns the password"
    );
}

#[tokio::test]
async fn a_saturated_verifier_answers_503_with_a_retry_after_of_one() {
    let harness = start().await;

    let mut held = Vec::new();
    while let Some(permit) = harness.auth.try_argon2_permit() {
        held.push(permit);
    }
    assert_eq!(held.len(), 2, "ARGON2_PERMITS is the documented bound");

    let response = login(&harness, PASSWORD).await;
    assert_eq!(response.status(), 503);
    assert_eq!(response.headers().get("retry-after").unwrap(), "1");
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert!(response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .is_none());
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unavailable");

    drop(held);
    assert_eq!(login(&harness, PASSWORD).await.status(), 204);
}

#[tokio::test]
async fn a_malformed_login_body_is_a_400_carrying_no_store() {
    let harness = start().await;
    let response = harness
        .client
        .post(harness.url("/api/v1/auth/login"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("{\"password\":")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
}

#[tokio::test]
async fn the_middleware_401_also_carries_no_store() {
    let harness = start().await;
    let response = harness
        .client
        .post(harness.url("/api/v1/auth/logout"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
}

#[tokio::test]
async fn the_production_limiter_rejects_a_burst_with_429_and_retry_after() {
    let harness = start_with(HarnessOptions {
        limits: AuthState::production_limits(),
        ..HarnessOptions::default()
    })
    .await;

    for attempt in 1..=5 {
        assert_eq!(
            login(&harness, "wrong").await.status(),
            401,
            "attempt {attempt} is a normal failure"
        );
    }

    let response = login(&harness, PASSWORD).await;
    assert_eq!(response.status(), 429);
    let retry_after: u64 = response
        .headers()
        .get("retry-after")
        .expect("a rate-limit rejection advertises when to retry")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry_after));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert!(response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .is_none());
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn patching_the_api_tls_boot_key_does_not_move_the_auth_decision() {
    let harness = start().await;
    assert_eq!(login(&harness, PASSWORD).await.status(), 204);

    let body: Value = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"api": {"tls": false}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["restart_required"], true);
    assert_eq!(
        harness.get_json("/api/v1/config").await["api"]["tls"],
        false,
        "the patched value is published live, which is why auth must not read it"
    );

    let response = login(&harness, PASSWORD).await;
    assert_eq!(
        response.status(),
        204,
        "the listener is still TLS, so session login stays available until a restart"
    );
    assert!(response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .is_some());

    let cookie = session_cookie(&response);
    let origin = own_origin(&harness);
    assert!(
        connect_socket(&harness, Some(&cookie), Some(&origin), false, false)
            .await
            .is_ok(),
        "the Origin scheme must still be derived from the live listener, not the patch"
    );
}

#[tokio::test]
async fn with_tls_off_only_login_is_refused_and_it_advertises_no_retry() {
    let harness = start_with(HarnessOptions {
        tls: false,
        ..HarnessOptions::default()
    })
    .await;

    let response = login(&harness, PASSWORD).await;
    assert_eq!(response.status(), 503);
    assert!(
        response.headers().get("retry-after").is_none(),
        "a boot-key condition never clears on its own, so it advertises no interval"
    );
    assert!(response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .is_none());
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unavailable");
    assert!(
        body["error"]["message"].as_str().unwrap().contains("TLS"),
        "the message names the requirement"
    );

    let logout = harness
        .client
        .post(harness.url("/api/v1/auth/logout"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 204);

    let before = secret_file(&harness);
    let logout_all = harness
        .client
        .post(harness.url("/api/v1/auth/logout-all"))
        .bearer_auth(&harness.key)
        .send()
        .await
        .unwrap();
    assert_eq!(logout_all.status(), 204);
    assert_ne!(secret_file(&harness), before);

    let wrong = harness
        .client
        .post(harness.url("/api/v1/auth/password"))
        .bearer_auth(&harness.key)
        .json(&json!({ "current_password": "wrong", "new_password": "a-long-replacement" }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    let right = harness
        .client
        .post(harness.url("/api/v1/auth/password"))
        .bearer_auth(&harness.key)
        .json(&json!({ "current_password": PASSWORD, "new_password": "a-long-replacement" }))
        .send()
        .await
        .unwrap();
    assert_eq!(right.status(), 204);
}

async fn connect_socket(
    harness: &Harness,
    cookie: Option<&str>,
    origin: Option<&str>,
    token: bool,
    bearer: bool,
) -> Result<EventSocket, tokio_tungstenite::tungstenite::Error> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;

    let base = harness.base.replace("https://", "wss://");
    let url = if token {
        format!("{base}/api/v1/events?token={}", harness.key)
    } else {
        format!("{base}/api/v1/events")
    };
    let mut request = url.into_client_request().unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_str(cookie).unwrap());
    }
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("origin", HeaderValue::from_str(origin).unwrap());
    }
    if bearer {
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", harness.key)).unwrap(),
        );
    }
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector))
        .await
        .map(|(socket, _)| socket)
}

fn own_origin(harness: &Harness) -> String {
    harness.base.clone()
}

#[tokio::test]
async fn the_socket_upgrades_with_only_the_cookie_and_a_matching_origin() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;
    let origin = own_origin(&harness);

    let socket = connect_socket(&harness, Some(&cookie), Some(&origin), false, false).await;
    assert!(socket.is_ok(), "a same-origin cookie upgrade must succeed");
    drop(socket);
    wait_for_query_subscribers(&harness, false).await;
}

#[tokio::test]
async fn the_socket_rejects_a_cookie_upgrade_with_a_foreign_or_absent_origin() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;

    assert!(
        connect_socket(
            &harness,
            Some(&cookie),
            Some("https://evil.example.com"),
            false,
            false
        )
        .await
        .is_err(),
        "a foreign Origin must not open a cookie-authenticated socket"
    );
    assert!(
        connect_socket(&harness, Some(&cookie), None, false, false)
            .await
            .is_err(),
        "a missing Origin is not acceptable from a browser"
    );
    assert!(
        !harness.server.events().has_query_subscribers(),
        "a rejected upgrade must never have taken a subscription slot"
    );
}

#[tokio::test]
async fn a_bearer_upgrade_with_no_origin_succeeds_in_both_forms() {
    let harness = start().await;

    let header_form = connect_socket(&harness, None, None, false, true).await;
    assert!(header_form.is_ok(), "Authorization header form");
    drop(header_form);
    wait_for_query_subscribers(&harness, false).await;

    let query_form = connect_socket(&harness, None, None, true, false).await;
    assert!(query_form.is_ok(), "?token= form");
    drop(query_form);
    wait_for_query_subscribers(&harness, false).await;
}

const CERT_PATHS: [&str; 4] = [
    "/api/v1/certificates",
    "/api/v1/certificates/ca/generate",
    "/api/v1/certificates/ca/export",
    "/api/v1/certificates/import",
];

struct Pem {
    cert: String,
    key: String,
}

fn server_pem(offset_days: i64, validity_days: i64) -> Pem {
    fah_api::install_crypto_provider();
    let mut params = rcgen::CertificateParams::new(vec!["fah.example".to_string()]).unwrap();
    params.not_before = time::OffsetDateTime::now_utc() + time::Duration::days(offset_days);
    params.not_after = params.not_before + time::Duration::days(validity_days);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    Pem {
        cert: cert.pem(),
        key: key.serialize_pem(),
    }
}

fn fingerprint(der: &[u8]) -> String {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, der);
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn der_of(pem: &str) -> Vec<u8> {
    let mut reader = pem.as_bytes();
    let mut certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(certs.len(), 1, "an export carries exactly one certificate");
    certs.swap_remove(0).to_vec()
}

impl Harness {
    async fn post_json(&self, path: &str, body: Value) -> reqwest::Response {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn post_ok(&self, path: &str, body: Value) -> Value {
        let response = self.post_json(path, body).await;
        assert!(response.status().is_success(), "POST {path}");
        response.json().await.unwrap()
    }
}

async fn generate_ca(harness: &Harness) -> Value {
    harness
        .post_ok(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true }),
        )
        .await
}

#[tokio::test]
async fn the_full_certificate_lifecycle_matches_the_documented_shapes() {
    let harness = start().await;

    let before = harness.get_json("/api/v1/certificates").await;
    assert_eq!(before["ca"]["present"], false);
    assert!(before["ca"]["fingerprint_sha256"].is_null());
    assert_eq!(before["api_certificate"]["source"], "self_signed");
    assert_eq!(
        before["dot"],
        serde_json::json!({ "state": "listening", "address": "[::]:853" })
    );
    assert_eq!(before["leaf_cache"]["capacity"], 512);
    assert_eq!(before["leaf_cache"]["size"], 0);

    let generated = generate_ca(&harness).await;
    assert_eq!(generated["archived_previous"], false);
    assert_eq!(generated["ca"]["present"], true);
    let first = generated["ca"]["fingerprint_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(first.contains(':'), "fingerprint is colon-separated hex");

    let status = harness.get_json("/api/v1/certificates").await;
    assert_eq!(status["ca"]["fingerprint_sha256"], first);
    assert_eq!(status["ca"]["subject"], "CN=FastAdHunter CA");
    assert!(status["ca"]["not_before"].as_str().unwrap().ends_with('Z'));
    assert!(status["ca"]["not_after"].as_str().unwrap().ends_with('Z'));

    let pem = harness.get("/api/v1/certificates/ca/export").await;
    assert_eq!(pem.status(), 200);
    assert_eq!(
        pem.headers()["content-type"].to_str().unwrap(),
        "application/x-pem-file"
    );
    assert_eq!(
        pem.headers()["content-disposition"].to_str().unwrap(),
        "attachment; filename=\"fastadhunter-ca.pem\""
    );
    let pem_body = pem.text().await.unwrap();
    assert_eq!(fingerprint(&der_of(&pem_body)), first);

    let der = harness
        .get("/api/v1/certificates/ca/export?format=der")
        .await;
    assert_eq!(der.status(), 200);
    assert_eq!(
        der.headers()["content-type"].to_str().unwrap(),
        "application/pkix-cert"
    );
    assert_eq!(
        der.headers()["content-disposition"].to_str().unwrap(),
        "attachment; filename=\"fastadhunter-ca.crt\""
    );
    let der_body = der.bytes().await.unwrap();
    assert_eq!(fingerprint(&der_body), first);

    let pair = server_pem(0, 30);
    let imported = harness
        .post_ok(
            "/api/v1/certificates/import",
            json!({ "format": "pem", "cert_pem": pair.cert, "key_pem": pair.key }),
        )
        .await;
    assert_eq!(imported["applied"], false);
    assert_eq!(imported["restart_required"], true);
    assert_eq!(imported["source"], "imported");

    let after_import = harness.get_json("/api/v1/certificates").await;
    assert_eq!(after_import["api_certificate"]["source"], "imported");
    assert_eq!(
        after_import["ca"]["fingerprint_sha256"], first,
        "importing the API pair must not touch the authority"
    );

    let again = generate_ca(&harness).await;
    assert_eq!(again["archived_previous"], true);
    let second = again["ca"]["fingerprint_sha256"].as_str().unwrap();
    assert_ne!(second, first, "a regeneration mints a new authority");
    assert_eq!(
        harness.get_json("/api/v1/certificates").await["ca"]["fingerprint_sha256"],
        second
    );
}

#[tokio::test]
async fn no_export_of_either_format_carries_private_material() {
    let harness = start().await;
    generate_ca(&harness).await;

    let pem = harness
        .get("/api/v1/certificates/ca/export?format=pem")
        .await
        .text()
        .await
        .unwrap();
    assert!(pem.contains("BEGIN CERTIFICATE"));
    assert!(
        !pem.contains("PRIVATE"),
        "the PEM export leaked key material"
    );

    let der = harness
        .get("/api/v1/certificates/ca/export?format=der")
        .await
        .bytes()
        .await
        .unwrap();
    assert!(!der.is_empty());
    assert!(
        !der.windows(7).any(|window| window == b"PRIVATE"),
        "the DER export leaked key material"
    );

    let store = harness.certs.as_ref().unwrap();
    assert!(!store.ca_public_pem().unwrap().contains("PRIVATE"));
}

#[tokio::test]
async fn an_import_carrying_pasted_key_material_never_reaches_a_response_or_disk() {
    let harness = start().await;
    let pair = server_pem(0, 30);
    let blob = format!("{}{}", pair.cert, pair.key);

    let response = harness
        .post_json(
            "/api/v1/certificates/import",
            json!({ "cert_pem": blob, "key_pem": pair.key }),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(
        !body.contains("PRIVATE"),
        "the response echoed key material"
    );

    let stored = std::fs::read_to_string(harness._config_dir.path().join("api-cert.pem")).unwrap();
    assert!(
        !stored.contains("PRIVATE"),
        "the stored certificate kept the pasted key"
    );
}

#[tokio::test]
async fn each_invalid_import_class_is_a_422_naming_its_cause() {
    let harness = start().await;
    let valid = server_pem(0, 30);
    let other = server_pem(0, 30);
    let expired = server_pem(-40, 10);
    let future = server_pem(5, 30);

    let cases: Vec<(Value, &str)> = vec![
        (
            json!({ "cert_pem": expired.cert, "key_pem": expired.key }),
            "expired:",
        ),
        (
            json!({ "cert_pem": future.cert, "key_pem": future.key }),
            "not_yet_valid:",
        ),
        (
            json!({ "cert_pem": valid.cert, "key_pem": other.key }),
            "key_mismatch:",
        ),
        (
            json!({ "cert_pem": "not a certificate", "key_pem": valid.key }),
            "parse:",
        ),
        (json!({ "key_pem": valid.key }), "parse:"),
        (
            json!({ "format": "pfx", "pfx_base64": "AAAA", "passphrase": "hunter2" }),
            "unsupported_format:",
        ),
    ];

    for (body, prefix) in cases {
        let response = harness.post_json("/api/v1/certificates/import", body).await;
        assert_eq!(response.status(), 422, "{prefix}");
        let payload: Value = response.json().await.unwrap();
        assert_eq!(payload["error"]["code"], "validation_failed");
        let message = payload["error"]["message"].as_str().unwrap();
        assert!(
            message.starts_with(prefix),
            "expected {prefix} got {message}"
        );
    }

    let status = harness.get_json("/api/v1/certificates").await;
    assert_eq!(
        status["api_certificate"]["source"], "self_signed",
        "a rejected import must not replace the API pair"
    );
}

#[tokio::test]
async fn a_rejected_import_never_echoes_the_submitted_material() {
    let harness = start().await;
    let valid = server_pem(0, 30);
    let secret = server_pem(0, 30).key;

    let response = harness
        .post_json(
            "/api/v1/certificates/import",
            json!({ "cert_pem": valid.cert, "key_pem": secret.clone() }),
        )
        .await;
    assert_eq!(response.status(), 422);

    let body = response.text().await.unwrap();
    for line in secret.lines().filter(|line| !line.starts_with('-')) {
        assert!(!body.contains(line), "the error echoed a key line");
    }
    for line in valid.cert.lines().filter(|line| !line.starts_with('-')) {
        assert!(!body.contains(line), "the error echoed a certificate line");
    }
}

#[tokio::test]
async fn generation_needs_an_explicit_confirmation() {
    let harness = start().await;

    for body in [json!({}), json!({ "confirm": false })] {
        let response = harness
            .post_json("/api/v1/certificates/ca/generate", body)
            .await;
        assert_eq!(response.status(), 400);
        let payload: Value = response.json().await.unwrap();
        assert_eq!(payload["error"]["code"], "bad_request");
        assert!(payload["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("confirm: true is required"));
    }

    let response = harness
        .post_json(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": "yes" }),
        )
        .await;
    assert_eq!(
        response.status(),
        400,
        "a wrongly typed confirm is not a yes"
    );

    assert_eq!(
        harness.get_json("/api/v1/certificates").await["ca"]["present"],
        false,
        "a refused generation must leave the store untouched"
    );
}

#[tokio::test]
async fn generation_parameters_are_range_checked() {
    let harness = start().await;

    for body in [
        json!({ "confirm": true, "validity_days": 0 }),
        json!({ "confirm": true, "validity_days": 7301 }),
        json!({ "confirm": true, "common_name": "   " }),
        json!({ "confirm": true, "common_name": "x".repeat(65) }),
    ] {
        let response = harness
            .post_json("/api/v1/certificates/ca/generate", body)
            .await;
        assert_eq!(response.status(), 422);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "validation_failed"
        );
    }

    let named = harness
        .post_ok(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true, "common_name": "Household CA", "validity_days": 30 }),
        )
        .await;
    assert_eq!(named["ca"]["subject"], "CN=Household CA");
}

#[tokio::test]
async fn export_answers_404_without_an_authority_and_422_for_an_unknown_format() {
    let harness = start().await;

    let missing = harness.get("/api/v1/certificates/ca/export").await;
    assert_eq!(missing.status(), 404);
    assert_eq!(
        missing.json::<Value>().await.unwrap()["error"]["code"],
        "not_found"
    );

    let bad = harness
        .get("/api/v1/certificates/ca/export?format=pfx")
        .await;
    assert_eq!(bad.status(), 422);
    assert_eq!(
        bad.json::<Value>().await.unwrap()["error"]["code"],
        "validation_failed"
    );
}

#[tokio::test]
async fn an_oversized_certificate_body_is_refused_by_the_route_limit() {
    let harness = start().await;

    let response = harness
        .post_json(
            "/api/v1/certificates/import",
            json!({ "cert_pem": "A".repeat(300 * 1024), "key_pem": "B" }),
        )
        .await;
    assert_eq!(response.status(), 400);
    let message = response.json::<Value>().await.unwrap()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("262144"), "{message}");
}

#[tokio::test]
async fn every_certificate_route_takes_both_credentials_and_refuses_neither() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;

    for path in CERT_PATHS {
        let response = harness.client.get(harness.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 401, "{path} must require a credential");
        assert_eq!(
            response.headers()["cache-control"].to_str().unwrap(),
            "no-store"
        );
    }

    for path in [CERT_PATHS[0], CERT_PATHS[2]] {
        for request in [
            harness
                .client
                .get(harness.url(path))
                .bearer_auth(&harness.key),
            harness
                .client
                .get(harness.url(path))
                .header("Cookie", &cookie),
        ] {
            let status = request.send().await.unwrap().status();
            assert_ne!(status, 401, "{path} rejected a valid credential");
        }
    }
}

#[tokio::test]
async fn both_certificate_posts_work_with_either_credential_and_fail_with_neither() {
    let harness = start().await;
    let cookie = login_cookie(&harness).await;
    let pairs = [server_pem(0, 30), server_pem(0, 30)];

    let bodies = [
        (
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true }),
        ),
        (
            "/api/v1/certificates/import",
            json!({ "cert_pem": pairs[0].cert, "key_pem": pairs[0].key }),
        ),
    ];

    for (path, body) in &bodies {
        let response = harness
            .client
            .post(harness.url(path))
            .json(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "POST {path} without a credential");
    }

    for (index, (path, body)) in bodies.iter().enumerate() {
        let bearer = harness
            .client
            .post(harness.url(path))
            .bearer_auth(&harness.key)
            .json(body)
            .send()
            .await
            .unwrap();
        assert_eq!(bearer.status(), 200, "POST {path} with the bearer key");

        let with_cookie = match index {
            1 => json!({ "cert_pem": pairs[1].cert, "key_pem": pairs[1].key }),
            _ => body.clone(),
        };
        let session = harness
            .client
            .post(harness.url(path))
            .header("Cookie", &cookie)
            .json(&with_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(session.status(), 200, "POST {path} with a session cookie");
    }

    let status = harness.get_json("/api/v1/certificates").await;
    assert_eq!(status["ca"]["present"], true);
    assert_eq!(status["api_certificate"]["source"], "imported");
}

#[tokio::test]
async fn the_error_paths_carry_no_store_too() {
    let harness = start().await;

    let responses = vec![
        harness.get("/api/v1/certificates/ca/export").await,
        harness
            .get("/api/v1/certificates/ca/export?format=pfx")
            .await,
        harness
            .post_json("/api/v1/certificates/ca/generate", json!({}))
            .await,
        harness
            .post_json("/api/v1/certificates/import", json!({ "cert_pem": "x" }))
            .await,
        harness.get("/api/v1/certificates/import").await,
    ];

    for response in responses {
        let url = response.url().clone();
        let status = response.status();
        assert!(status.is_client_error(), "{url} answered {status}");
        assert_eq!(
            response.headers()["cache-control"].to_str().unwrap(),
            "no-store",
            "{url} answered {status} without no-store"
        );
    }
}

#[tokio::test]
async fn a_body_that_is_not_documented_json_is_a_400_on_both_posts() {
    let harness = start().await;

    for path in [
        "/api/v1/certificates/ca/generate",
        "/api/v1/certificates/import",
    ] {
        let untyped = harness
            .client
            .post(harness.url(path))
            .bearer_auth(&harness.key)
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(untyped.status(), 400, "{path} without a content type");

        let malformed = harness
            .client
            .post(harness.url(path))
            .bearer_auth(&harness.key)
            .header("Content-Type", "application/json")
            .body("{\"confirm\":")
            .send()
            .await
            .unwrap();
        assert_eq!(malformed.status(), 400, "{path} with malformed JSON");
        assert_eq!(
            malformed.json::<Value>().await.unwrap()["error"]["code"],
            "bad_request"
        );
    }
}

#[tokio::test]
async fn the_body_limit_covers_generation_as_well_as_import() {
    let harness = start().await;

    let response = harness
        .post_json(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true, "common_name": "A".repeat(300 * 1024) }),
        )
        .await;
    assert_eq!(response.status(), 400);
    let message = response.json::<Value>().await.unwrap()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("262144"), "{message}");

    assert_eq!(
        harness.get_json("/api/v1/certificates").await["ca"]["present"],
        false,
        "a refused body must not reach the store"
    );
}

#[tokio::test]
async fn a_common_name_is_bounded_in_bytes_not_characters() {
    let harness = start().await;

    let response = harness
        .post_json(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true, "common_name": "é".repeat(33) }),
        )
        .await;
    assert_eq!(
        response.status(),
        422,
        "33 two-byte characters are 66 bytes, past RFC 5280's ub-common-name"
    );
    assert_eq!(
        harness.get_json("/api/v1/certificates").await["ca"]["present"],
        false
    );
}

#[tokio::test]
async fn replacing_the_api_pair_archives_the_pair_it_replaced() {
    let harness = start().await;
    let config = harness._config_dir.path().to_path_buf();
    let original = std::fs::read_to_string(config.join("api-key.pem")).unwrap();

    let first = server_pem(0, 30);
    harness
        .post_ok(
            "/api/v1/certificates/import",
            json!({ "cert_pem": first.cert, "key_pem": first.key }),
        )
        .await;

    let archive = config.join("api-archive");
    let stamped = std::fs::read_dir(&archive)
        .expect("the replaced API pair is archived")
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        std::fs::read_to_string(stamped.join("api-key.pem")).unwrap(),
        original,
        "the private key the import replaced must be recoverable"
    );
    assert_eq!(
        std::fs::read_to_string(config.join("api-key.pem")).unwrap(),
        first.key
    );
}

#[tokio::test]
async fn archived_previous_reports_the_disk_not_the_slot() {
    let harness = start().await;
    let config = harness._config_dir.path();

    let planted = server_pem(0, 30);
    std::fs::write(config.join("ca-cert.pem"), &planted.cert).unwrap();
    std::fs::write(config.join("ca-key.pem"), &planted.key).unwrap();

    let generated = generate_ca(&harness).await;
    assert_eq!(
        generated["archived_previous"], true,
        "a pair that was on disk was replaced, so it was archived"
    );
    assert!(config.join("ca-archive").exists());
}

#[tokio::test]
async fn a_full_archive_answers_409_and_leaves_the_authority_in_place() {
    let harness = start().await;
    for _ in 0..=fah_certs::MAX_ARCHIVES {
        generate_ca(&harness).await;
    }
    let before = harness.get_json("/api/v1/certificates").await;

    let refused = harness
        .post_json(
            "/api/v1/certificates/ca/generate",
            json!({ "confirm": true }),
        )
        .await;
    assert_eq!(refused.status(), 409);
    let payload: Value = refused.json().await.unwrap();
    assert_eq!(payload["error"]["code"], "conflict");
    let message = payload["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("archive_full:"), "{message}");
    assert!(!message.contains("/config/"), "{message}");

    let after = harness.get_json("/api/v1/certificates").await;
    assert_eq!(
        after["ca"]["fingerprint_sha256"],
        before["ca"]["fingerprint_sha256"]
    );
    assert_eq!(
        std::fs::read_dir(harness._config_dir.path().join("ca-archive"))
            .unwrap()
            .count(),
        fah_certs::MAX_ARCHIVES
    );
}

#[tokio::test]
async fn every_certificate_response_carries_no_store() {
    let harness = start().await;
    generate_ca(&harness).await;
    let pair = server_pem(0, 30);

    let responses = vec![
        harness.get("/api/v1/certificates").await,
        harness.get("/api/v1/certificates/ca/export").await,
        harness
            .post_json(
                "/api/v1/certificates/ca/generate",
                json!({ "confirm": true }),
            )
            .await,
        harness
            .post_json(
                "/api/v1/certificates/import",
                json!({ "cert_pem": pair.cert, "key_pem": pair.key }),
            )
            .await,
    ];

    for response in responses {
        let url = response.url().clone();
        assert!(response.status().is_success(), "{url}");
        assert_eq!(
            response.headers()["cache-control"].to_str().unwrap(),
            "no-store",
            "{url}"
        );
    }
}

#[tokio::test]
async fn a_store_that_did_not_open_answers_503_without_a_retry_after() {
    let harness = start_with(HarnessOptions {
        certs: false,
        ..Default::default()
    })
    .await;

    let responses = vec![
        harness.get("/api/v1/certificates").await,
        harness.get("/api/v1/certificates/ca/export").await,
        harness
            .post_json(
                "/api/v1/certificates/ca/generate",
                json!({ "confirm": true }),
            )
            .await,
        harness
            .post_json("/api/v1/certificates/import", json!({ "cert_pem": "x" }))
            .await,
    ];

    for response in responses {
        let path = response.url().path().to_string();
        assert_eq!(response.status(), 503, "{path}");
        assert!(
            response.headers().get("retry-after").is_none(),
            "{path} must not invite a timed retry"
        );
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            "unavailable"
        );
    }
}

impl Harness {
    async fn put_interception(&self, body: Value) -> reqwest::Response {
        self.client
            .put(self.url("/api/v1/interception"))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn get_interception_returns_the_document() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/interception").await;
    assert_eq!(body, json!({"clients": [], "exclude_domains": []}));
}

#[tokio::test]
async fn put_replaces_the_whole_document_and_returns_it() {
    let harness = start().await;
    let sent = json!({
        "clients": ["192.168.88.10", "192.168.88.0/24"],
        "exclude_domains": [" Bank.Example. "]
    });

    let response = harness.put_interception(sent.clone()).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body, sent,
        "the stored document keeps the spelling that was sent"
    );

    assert_eq!(harness.get_json("/api/v1/interception").await, sent);

    let on_disk = tokio::fs::read_to_string(&harness.document_path)
        .await
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&on_disk).unwrap(), sent);
    assert!(on_disk.ends_with('\n'));

    let active = harness.interception.current();
    assert_eq!(active.scope.client_count(), 2);
    assert!(active.scope.excludes("api.bank.example"));
}

#[tokio::test]
async fn the_put_response_carries_no_restart_required() {
    let harness = start().await;
    let body: Value = harness
        .put_interception(json!({"clients": ["10.0.0.1"]}))
        .await
        .json()
        .await
        .unwrap();
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["clients", "exclude_domains"]);
}

#[tokio::test]
async fn put_with_an_unknown_key_is_422_with_shape_details() {
    let harness = start().await;
    let response = harness
        .put_interception(json!({"clients": [], "client": []}))
        .await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    assert_eq!(body["error"]["details"], json!({"reason": "shape"}));
    assert!(!harness.document_path.exists());
}

#[tokio::test]
async fn put_with_a_non_json_body_is_400_in_the_envelope() {
    let harness = start().await;
    let before = harness.get_json("/api/v1/interception").await;

    for (content_type, body) in [
        ("application/json", "{"),
        ("text/plain", r#"{"clients":[]}"#),
    ] {
        let response = harness
            .client
            .put(harness.url("/api/v1/interception"))
            .bearer_auth(&harness.key)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{content_type}");
        let envelope: Value = response.json().await.unwrap();
        assert_eq!(envelope["error"]["code"], "bad_request", "{content_type}");
        assert!(envelope["error"]["details"].is_null(), "{envelope}");
    }

    assert_eq!(harness.get_json("/api/v1/interception").await, before);
}

#[tokio::test]
async fn put_over_cap_is_422_with_over_cap_details_and_get_is_unchanged() {
    let harness = start().await;
    let before = harness.get_json("/api/v1/interception").await;
    let clients: Vec<String> = (0..257)
        .map(|index| format!("10.0.{}.{}", index / 256, index % 256))
        .collect();

    let response = harness.put_interception(json!({"clients": clients})).await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["error"]["details"],
        json!({"reason": "over_cap", "list": "clients", "len": 257, "cap": 256})
    );
    assert_eq!(harness.get_json("/api/v1/interception").await, before);
}

#[tokio::test]
async fn put_with_a_duplicate_is_422_with_duplicate_details() {
    let harness = start().await;
    let response = harness
        .put_interception(json!({"exclude_domains": ["a.example", "Bank.ro", "bank.ro."]}))
        .await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["error"]["details"],
        json!({
            "reason": "duplicate",
            "list": "exclude_domains",
            "index": 2,
            "entry": "bank.ro.",
            "duplicate_of": 1
        })
    );
}

#[tokio::test]
async fn put_with_an_invalid_entry_is_422_with_invalid_entry_details() {
    let harness = start().await;
    let response = harness
        .put_interception(json!({"clients": ["10.0.0.1", "10.0.0.300"]}))
        .await;
    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["error"]["details"],
        json!({
            "reason": "invalid_entry",
            "list": "clients",
            "index": 1,
            "entry": "10.0.0.300"
        })
    );
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("is not an IP address or CIDR block"));
}

#[tokio::test]
async fn put_is_503_when_the_certificate_store_did_not_open() {
    let harness = start_with(HarnessOptions {
        interception_runtime: fah_api::InterceptionRuntime::StoreClosed,
        ..HarnessOptions::default()
    })
    .await;

    let response = harness
        .put_interception(json!({"clients": ["10.0.0.1"]}))
        .await;
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "unavailable"
    );
    assert!(!harness.document_path.exists());

    let response = harness
        .put_interception(json!({"exclude_domains": ["bank.example"]}))
        .await;
    assert_eq!(response.status(), 200, "an empty client list is accepted");
}

#[tokio::test]
async fn a_write_failure_is_500_and_get_is_unchanged() {
    let harness = start().await;
    let before = harness.get_json("/api/v1/interception").await;
    tokio::fs::create_dir(&harness.document_path).await.unwrap();

    let response = harness
        .put_interception(json!({"clients": ["10.0.0.1"]}))
        .await;
    assert_eq!(response.status(), 500);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "internal"
    );
    assert_eq!(harness.get_json("/api/v1/interception").await, before);
}

#[tokio::test]
async fn a_commit_panic_is_500_and_the_next_put_succeeds() {
    let harness = start().await;
    let before = harness.get_json("/api/v1/interception").await;

    harness.interception.set_panic_after_lock(true);
    let response = harness
        .put_interception(json!({"clients": ["10.0.0.1"]}))
        .await;
    assert_eq!(response.status(), 500);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "internal"
    );
    assert_eq!(harness.get_json("/api/v1/interception").await, before);

    harness.interception.set_panic_after_lock(false);
    let sent = json!({"clients": ["10.0.0.2"], "exclude_domains": []});
    let response = harness.put_interception(sent.clone()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(harness.get_json("/api/v1/interception").await, sent);
}

#[tokio::test]
async fn post_config_rejects_https_interception_naming_the_endpoint() {
    let harness = start().await;
    let before = tokio::fs::read_to_string(&harness.config_path)
        .await
        .unwrap();

    let response = harness
        .client
        .post(harness.url("/api/v1/config"))
        .bearer_auth(&harness.key)
        .json(&json!({"https": {"interception": {"clients": ["10.0.0.1"]}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(body["error"]["details"].is_null(), "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/interception"),
        "{body}"
    );
    assert_eq!(
        tokio::fs::read_to_string(&harness.config_path)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn get_config_omits_https_interception() {
    let harness = start().await;
    let body = harness.get_json("/api/v1/config").await;
    assert!(body["https"].get("interception").is_none(), "{body}");
}

async fn handed_over(server: &mut ApiServer) -> tokio::task::JoinHandle<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(handle) = server.take_finished_acceptor() {
            return handle;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "a closed admission must end the accept loop"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn poke(server: &ApiServer) {
    let _ = tokio::net::TcpStream::connect(server.local_addr()).await;
}

#[tokio::test]
async fn a_running_acceptor_is_not_handed_over() {
    let mut harness = start().await;
    assert!(
        harness.server.take_finished_acceptor().is_none(),
        "a live acceptor must keep its handle"
    );
}

#[tokio::test]
async fn a_returned_acceptor_is_handed_over_once() {
    let mut harness = start().await;
    harness.server.close_admission();
    poke(&harness.server).await;

    let handle = handed_over(&mut harness.server).await;
    assert!(
        handle.await.is_ok(),
        "the loop must end by returning, not by panicking or being cancelled"
    );
    assert!(
        harness.server.take_finished_acceptor().is_none(),
        "a death is handed over once, not on every supervision tick"
    );
}

#[tokio::test]
async fn shutdown_is_safe_after_the_handle_was_taken() {
    let mut harness = start().await;
    harness.server.close_admission();
    poke(&harness.server).await;
    let taken = handed_over(&mut harness.server).await;
    assert!(taken.await.is_ok());

    harness.server.shutdown();
}

#[tokio::test]
async fn the_api_binds_an_ipv6_literal_and_serves_on_it() {
    let harness = start_with(HarnessOptions {
        address: "::1",
        ..HarnessOptions::default()
    })
    .await;

    let bound = harness.server.local_addr();
    assert!(bound.is_ipv6(), "got {bound}");
    assert!(harness.base.contains("[::1]"), "got {}", harness.base);

    let response = harness.get("/api/v1/stats").await;
    assert!(response.status().is_success(), "got {}", response.status());
}

#[tokio::test]
async fn the_unspecified_ipv6_api_address_binds_one_dual_stack_socket() {
    let harness = start_with(HarnessOptions {
        address: "::",
        ..HarnessOptions::default()
    })
    .await;

    let bound = harness.server.local_addr();
    assert!(
        bound.is_ipv6() && bound.ip().is_unspecified(),
        "got {bound}"
    );

    let over_ipv4 = harness
        .client
        .get(format!("https://127.0.0.1:{}/health", bound.port()))
        .send()
        .await
        .expect("clearing IPV6_V6ONLY lets an IPv4 peer reach the same socket");
    assert!(over_ipv4.status().is_success());
}

#[tokio::test]
async fn an_api_bind_conflict_names_the_api_port_setting() {
    let first = start().await;
    let taken = first.server.local_addr();

    let err = match try_start_with(HarnessOptions {
        port: taken.port(),
        ..HarnessOptions::default()
    })
    .await
    {
        Ok(_) => panic!("the first server already holds {taken}"),
        Err(err) => err,
    };

    let text = err.to_string();
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse, "got {text}");
    assert!(text.contains(&taken.to_string()), "got {text}");
    assert!(text.contains("FAH__API__PORT"), "got {text}");
    assert!(!text.contains("CAP_NET_BIND_SERVICE"), "got {text}");
}

#[tokio::test]
async fn an_unparseable_api_address_names_the_api_section() {
    let err = match try_start_with(HarnessOptions {
        address: "not-an-ip",
        ..HarnessOptions::default()
    })
    .await
    {
        Ok(_) => panic!("an address that is not an IP cannot bind"),
        Err(err) => err,
    };

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("[api]"), "got {err}");
}
