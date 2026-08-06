// FastAdHunter TUI - Versiune Optimizată (Zero-Allocation UI, Parallel Async Fetch, RwLock)

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
    Terminal,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    fs, io,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use tokio_tungstenite::{
    connect_async_tls_with_config, tungstenite::client::IntoClientRequest,
    tungstenite::protocol::Message, Connector,
};

// --- STRUCTURI TIPIZATE PENTRU MEMORIE ȘI SPEED ---

#[derive(Deserialize, Clone, Default)]
pub struct QueryEvent {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub qtype: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(alias = "cached", default)]
    pub is_cached: bool,
    #[serde(default)]
    pub duration_ms: f64,
    #[serde(default)]
    pub ts: String,
}

#[derive(Deserialize, Clone, Default)]
pub struct TopDomain {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub count: u64,
}

#[derive(Deserialize, Clone, Default)]
pub struct TopClient {
    #[serde(alias = "client", default)]
    pub ip: String,
    #[serde(default)]
    pub count: u64,
}

#[derive(Deserialize, Clone, Default)]
pub struct CacheStats {
    #[serde(default)]
    pub hits: u64,
    #[serde(default)]
    pub misses: u64,
    #[serde(default)]
    pub bytes: f64,
    #[serde(default)]
    pub load_percent: f64,
    #[serde(default)]
    pub entries: u64,
    #[serde(default)]
    pub capacity: u64,
    #[serde(default)]
    pub fresh: u64,
    #[serde(default)]
    pub stale: u64,
}

#[derive(Deserialize, Clone, Default)]
pub struct MemoryStats {
    #[serde(default)]
    pub process_rss: f64,
    #[serde(default)]
    pub process_peak_rss: f64,
    #[serde(default)]
    pub ruleset_bytes: f64,
    #[serde(default)]
    pub allocator_committed_peak_bytes: f64,
}

#[derive(Deserialize, Clone, Default)]
struct HistoryPerf {
    #[serde(default)]
    items: Vec<HistoryItem>,
}

#[derive(Deserialize, Clone, Default)]
struct HistoryItem {
    #[serde(default)]
    rss_bytes: f64,
}

#[derive(Deserialize, Clone, Default)]
struct EndpointsConfig {
    ws_events: String,
    cache: String,
    memory: String,
    metrics: String,
    history_perf: String,
    history_summary: String,
}

#[derive(Deserialize, Clone, Default)]
struct RouterOsConfig {
    ros_base: Option<String>,
    ros_user: Option<String>,
    ros_pass: Option<String>,
    router_fetch_interval: Option<u64>,
}

#[derive(Deserialize, Clone)]
struct Config {
    host: String,
    port: u16,
    token: String,
    endpoints: EndpointsConfig,
    #[serde(default)]
    routeros: RouterOsConfig,
}

impl Config {
    fn load() -> Result<Self, Box<dyn Error>> {
        let contents = fs::read_to_string("config.toml")
            .map_err(|_| "Nu s-a putut citi fișierul config.toml!")?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}

#[derive(Default)]
struct AppState {
    ws_connected: bool,
    ws_status_msg: String,
    queries: VecDeque<QueryEvent>,
    queries_total: u64,
    blocked_total: u64,
    blocked_percent: f64,
    cache_hit_percent: f64,
    top_blocked_domains: Vec<TopDomain>,
    top_clients: Vec<TopClient>,
    top_queried_domains: Vec<TopDomain>,

    query_scroll: usize,
    stats_scroll: u16,

    // Popup Data
    popup_query: Option<QueryEvent>,

    // HTTP / Metrics Data
    cache_data: CacheStats,
    memory_data: MemoryStats,
    rules_count: u64,
    avg_block_str: String,
    avg_cache_str: String,
    perf_points: VecDeque<f64>,
    p_upstream_ip: String,
    p_dns_counter: u64,
    s_upstream_ip: String,
    s_dns_counter: u64,

    // RouterOS Data
    router_free_mem: String,
    router_cpu_freq: String,
    router_cpu_load: String,
    router_container_mem: String,

    // History Summary 24H Data
    summary_24h_total_queries: u64,
    summary_24h_blocked: u64,
    summary_24h_cache_hits: u64,
    summary_24h_blocked_percent: f64,
    summary_24h_cache_hit_percent: f64,
    summary_24h_a: u64,
    summary_24h_aaaa: u64,
    summary_24h_https: u64,
    summary_24h_a_percent: f64,
    summary_24h_aaaa_percent: f64,
    summary_24h_https_percent: f64,

    // History Summary 7 days Data
    summary_7d_total_queries: u64,
    summary_7d_blocked: u64,
    summary_7d_cache_hits: u64,
    summary_7d_blocked_percent: f64,
    summary_7d_cache_hit_percent: f64,
    summary_7d_a: u64,
    summary_7d_aaaa: u64,
    summary_7d_https: u64,
    summary_7d_a_percent: f64,
    summary_7d_aaaa_percent: f64,
    summary_7d_https_percent: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;

    let state = Arc::new(RwLock::new(AppState {
        ws_status_msg: "CONNECTING".to_string(),
        avg_block_str: "0.000 ms".to_string(),
        avg_cache_str: "0.000 ms".to_string(),
        router_free_mem: "N/A".to_string(),
        router_cpu_freq: "N/A".to_string(),
        router_cpu_load: "N/A".to_string(),
        router_container_mem: "N/A".to_string(),
        ..Default::default()
    }));

    // Start Async Workers
    tokio::spawn(background_worker(Arc::clone(&state), config.clone()));
    tokio::spawn(router_worker(Arc::clone(&state), config.clone()));
    tokio::spawn(history_summary_worker(Arc::clone(&state), config.clone()));
    tokio::spawn(history_perf_worker(Arc::clone(&state), config.clone()));

    // Shared HTTP client and common variables for the 60s workers
    let http_client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            // If client cannot be built, still continue with other workers
            reqwest::Client::new()
        }
    };

    let base_url = format!("https://{}:{}", config.host, config.port);
    let auth_header = format!("Bearer {}", config.token);

    // Build full endpoint URLs once and spawn dedicated HTTP workers
    let auth_arc: Arc<str> = Arc::from(auth_header.into_boxed_str());

    let cache_url: Arc<str> =
        Arc::from(format!("{base_url}{}", config.endpoints.cache).into_boxed_str());
    let memory_url: Arc<str> =
        Arc::from(format!("{base_url}{}", config.endpoints.memory).into_boxed_str());
    let metrics_url: Arc<str> =
        Arc::from(format!("{base_url}{}", config.endpoints.metrics).into_boxed_str());

    // URLs are owned by a single worker; the authorization header is shared.
    tokio::spawn(cache_worker(
        Arc::clone(&state),
        http_client.clone(),
        cache_url,
        Arc::clone(&auth_arc),
    ));
    tokio::spawn(memory_worker(
        Arc::clone(&state),
        http_client.clone(),
        memory_url,
        Arc::clone(&auth_arc),
    ));
    tokio::spawn(metrics_worker(
        Arc::clone(&state),
        http_client.clone(),
        metrics_url,
        auth_arc,
    ));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture,)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_ui(&mut terminal, state).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
    )?;
    terminal.show_cursor()?;

    res
}

async fn history_perf_worker(state: Arc<RwLock<AppState>>, cfg: Config) {
    let base_url = format!("https://{}:{}", cfg.host, cfg.port);
    let url = format!("{}{}", base_url, cfg.endpoints.history_perf);
    let auth_header = format!("Bearer {}", cfg.token);

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    const HISTORY_WINDOW_MINUTES: usize = 24 * 60;
    const SAMPLE_INTERVAL_MINUTES: usize = 5;
    const MAX_POINTS: usize = HISTORY_WINDOW_MINUTES / SAMPLE_INTERVAL_MINUTES;

    if let Ok(resp) = client
        .get(&url)
        .header("Authorization", &auth_header)
        .send()
        .await
    {
        if let Ok(json) = resp.json::<HistoryPerf>().await {
            if let Ok(mut st) = state.write() {
                let mut deque: VecDeque<f64> = json
                    .items
                    .iter()
                    .map(|it| it.rss_bytes / 1024.0 / 1024.0)
                    .collect();
                while deque.len() > MAX_POINTS {
                    deque.pop_front();
                }
                st.perf_points = deque;
            }
        }
    }
}

async fn history_summary_worker(state: Arc<RwLock<AppState>>, cfg: Config) {
    let base_url = format!("https://{}:{}", cfg.host, cfg.port);
    let url = format!("{}{}", base_url, cfg.endpoints.history_summary);
    let auth_header = format!("Bearer {}", cfg.token);

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    const HISTORY_REFRESH: Duration = Duration::from_secs(60 * 60);
    let mut interval = tokio::time::interval(HISTORY_REFRESH);

    loop {
        interval.tick().await;

        fetch_summary_24h(state.clone(), &client, &url, &auth_header).await;
        fetch_summary_last_7days(state.clone(), &client, &url, &auth_header).await;
    }
}

async fn fetch_summary_24h(
    state: Arc<RwLock<AppState>>,
    client: &reqwest::Client,
    url: &str,
    auth_header: &str,
) {
    if let Ok(resp) = client
        .get(url)
        .header("Authorization", auth_header)
        .send()
        .await
    {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            let (
                total_queries,
                total_blocked,
                total_cache_hits,
                sum_a,
                sum_aaaa,
                sum_https,
                blocked_percent,
                cache_hit_percent,
                a_percent,
                aaaa_percent,
                https_percent,
            ) = parse_summary_data(&val);

            if let Ok(mut st) = state.write() {
                st.summary_24h_total_queries = total_queries;
                st.summary_24h_blocked = total_blocked;
                st.summary_24h_cache_hits = total_cache_hits;
                st.summary_24h_blocked_percent = blocked_percent;
                st.summary_24h_cache_hit_percent = cache_hit_percent;
                st.summary_24h_a = sum_a;
                st.summary_24h_aaaa = sum_aaaa;
                st.summary_24h_https = sum_https;
                st.summary_24h_a_percent = a_percent;
                st.summary_24h_aaaa_percent = aaaa_percent;
                st.summary_24h_https_percent = https_percent;
            }
        }
    }
}

async fn fetch_summary_last_7days(
    state: Arc<RwLock<AppState>>,
    client: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
) {
    // Calculate timestamp for 7 days ago using standard library
    let from = chrono::Utc::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(7))
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT00:00:00Z")
        .to_string();

    let url = format!("{}?from={}&resolution=day", base_url, from);

    if let Ok(resp) = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
    {
        if let Ok(val) = resp.json::<serde_json::Value>().await {
            let (
                total_queries,
                total_blocked,
                total_cache_hits,
                sum_a,
                sum_aaaa,
                sum_https,
                blocked_percent,
                cache_hit_percent,
                a_percent,
                aaaa_percent,
                https_percent,
            ) = parse_summary_data(&val);

            if let Ok(mut st) = state.write() {
                st.summary_7d_total_queries = total_queries;
                st.summary_7d_blocked = total_blocked;
                st.summary_7d_cache_hits = total_cache_hits;
                st.summary_7d_blocked_percent = blocked_percent;
                st.summary_7d_cache_hit_percent = cache_hit_percent;
                st.summary_7d_a = sum_a;
                st.summary_7d_aaaa = sum_aaaa;
                st.summary_7d_https = sum_https;
                st.summary_7d_a_percent = a_percent;
                st.summary_7d_aaaa_percent = aaaa_percent;
                st.summary_7d_https_percent = https_percent;
            }
        }
    }
}

// Extracted the inner iteration logic here to prevent massive code duplication
fn parse_summary_data(
    val: &serde_json::Value,
) -> (u64, u64, u64, u64, u64, u64, f64, f64, f64, f64, f64) {
    let empty_vec = vec![];
    let items_arr = val
        .get("items")
        .or_else(|| val.get("data"))
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| val.as_array().unwrap_or(&empty_vec));

    let mut total_queries = 0;
    let mut total_blocked = 0;
    let mut total_cache_hits = 0;
    let mut sum_a = 0;
    let mut sum_aaaa = 0;
    let mut sum_https = 0;

    for item in items_arr {
        total_queries += item
            .get("queries")
            .or_else(|| item.get("total_queries"))
            .or_else(|| item.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        total_blocked += item
            .get("blocked")
            .or_else(|| item.get("blocked_queries"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        total_cache_hits += item
            .get("cache_hits")
            .or_else(|| item.get("cache_hit"))
            .or_else(|| item.get("hits"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if let Some(types) = item
            .get("per_type")
            .or_else(|| item.get("types"))
            .or_else(|| item.get("by_type"))
            .and_then(|v| v.as_object())
        {
            if let Some(v) = types.get("A").and_then(|v| v.as_u64()) {
                sum_a += v;
            }
            if let Some(v) = types.get("AAAA").and_then(|v| v.as_u64()) {
                sum_aaaa += v;
            }
            if let Some(v) = types.get("HTTPS").and_then(|v| v.as_u64()) {
                sum_https += v;
            }
        }
    }

    let blocked_percent = if total_queries > 0 {
        (total_blocked as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };
    let cache_hit_percent = if total_queries > 0 {
        (total_cache_hits as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };
    let a_percent = if total_queries > 0 {
        (sum_a as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };
    let aaaa_percent = if total_queries > 0 {
        (sum_aaaa as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };
    let https_percent = if total_queries > 0 {
        (sum_https as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };

    (
        total_queries,
        total_blocked,
        total_cache_hits,
        sum_a,
        sum_aaaa,
        sum_https,
        blocked_percent,
        cache_hit_percent,
        a_percent,
        aaaa_percent,
        https_percent,
    )
}

async fn router_worker(state: Arc<RwLock<AppState>>, cfg: Config) {
    let ros_base = match cfg.routeros.ros_base {
        Some(b) => b,
        None => return,
    };
    let ros_user = cfg
        .routeros
        .ros_user
        .unwrap_or_else(|| "monitor".to_string());
    let ros_pass = cfg
        .routeros
        .ros_pass
        .or_else(|| std::env::var("MP").ok())
        .unwrap_or_default();
    let interval = Duration::from_secs(cfg.routeros.router_fetch_interval.unwrap_or(15));

    let client = match reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    loop {
        let req_sys = client
            .get(format!("{}/system/resource", ros_base))
            .basic_auth(&ros_user, Some(&ros_pass))
            .send();

        let req_cnt = client
            .get(format!("{}/container", ros_base))
            .basic_auth(&ros_user, Some(&ros_pass))
            .send();

        let (res_sys, res_cnt) = tokio::join!(req_sys, req_cnt);

        let mut free_mem_val = "N/A".to_string();
        let mut cpu_freq_val = "N/A".to_string();
        let mut cpu_load_val = "N/A".to_string();
        let mut container_mem_val = "N/A".to_string();

        if let Ok(resp) = res_sys {
            if let Ok(res_data) = resp.json::<serde_json::Value>().await {
                let obj = res_data
                    .as_array()
                    .and_then(|a| a.first())
                    .unwrap_or(&res_data);

                if let Some(raw_free) = obj.get("free-memory").or_else(|| obj.get("free_memory")) {
                    let bytes = raw_free
                        .as_u64()
                        .or_else(|| raw_free.as_str().and_then(|s| s.parse().ok()))
                        .unwrap_or(0);
                    if bytes > 0 {
                        free_mem_val = format!("{:.1}MiB", bytes as f64 / 1024.0 / 1024.0);
                    }
                }
                if let Some(freq) = obj
                    .get("cpu-frequency")
                    .or_else(|| obj.get("cpu_frequency"))
                {
                    let f = freq
                        .as_u64()
                        .map(|n| n.to_string())
                        .or_else(|| freq.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    if !f.is_empty() {
                        cpu_freq_val = format!("{}MHz", f);
                    }
                }
                if let Some(load) = obj.get("cpu-load").or_else(|| obj.get("cpu_load")) {
                    let l = load
                        .as_u64()
                        .map(|n| n.to_string())
                        .or_else(|| load.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    if !l.is_empty() {
                        cpu_load_val = format!("{}%", l);
                    }
                }
            }
        }

        if let Ok(resp) = res_cnt {
            if let Ok(containers) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = containers.as_array() {
                    for item in arr {
                        if item.get("name").and_then(|v| v.as_str()) == Some("fastadhunter") {
                            if let Some(raw_mem) = item
                                .get("memory-current")
                                .or_else(|| item.get("memory_current"))
                            {
                                let bytes = raw_mem
                                    .as_u64()
                                    .or_else(|| raw_mem.as_str().and_then(|s| s.parse().ok()))
                                    .unwrap_or(0);
                                if bytes > 0 {
                                    container_mem_val =
                                        format!("{:.1}MiB", bytes as f64 / 1024.0 / 1024.0);
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }

        {
            if let Ok(mut st) = state.write() {
                st.router_free_mem = free_mem_val;
                st.router_cpu_freq = cpu_freq_val;
                st.router_cpu_load = cpu_load_val;
                st.router_container_mem = container_mem_val;
            }
        }

        tokio::time::sleep(interval).await;
    }
}

async fn background_worker(state: Arc<RwLock<AppState>>, cfg: Config) {
    let ws_url = format!(
        "wss://{}:{}{}?token={}",
        cfg.host, cfg.port, cfg.endpoints.ws_events, cfg.token
    );

    let _client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let native_tls_connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let connector = Connector::NativeTls(native_tls_connector);

    let ws_state = Arc::clone(&state);
    let ws_url_cl = ws_url.clone();
    let ws_token = cfg.token.clone();
    let connector_cl = connector.clone();

    tokio::spawn(async move {
        loop {
            let mut req = ws_url_cl.as_str().into_client_request().unwrap();
            req.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", ws_token).parse().unwrap(),
            );

            match connect_async_tls_with_config(req, None, false, Some(connector_cl.clone())).await
            {
                Ok((mut ws_stream, _)) => {
                    if let Ok(mut st) = ws_state.write() {
                        st.ws_connected = true;
                        st.ws_status_msg = "ONLINE (WS)".to_string();
                    }

                    while let Some(msg) = ws_stream.next().await {
                        if let Ok(Message::Text(text)) = msg {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                                let m_type =
                                    parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                let payload = parsed.get("data");

                                if let Some(p) = payload {
                                    let mut st = ws_state.write().unwrap();
                                    if m_type == "query" {
                                        if let Ok(q_evt) =
                                            serde_json::from_value::<QueryEvent>(p.clone())
                                        {
                                            st.queries.push_front(q_evt);
                                            if st.queries.len() > 200 {
                                                st.queries.pop_back();
                                            }
                                        }
                                    } else if m_type == "stats" {
                                        st.queries_total = p
                                            .get("queries_total")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0);
                                        st.blocked_total = p
                                            .get("blocked_total")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0);
                                        st.blocked_percent = p
                                            .get("blocked_percent")
                                            .and_then(|v| v.as_f64())
                                            .unwrap_or(0.0);
                                        st.cache_hit_percent = p
                                            .get("cache_hit_percent")
                                            .and_then(|v| v.as_f64())
                                            .unwrap_or(0.0);

                                        if let Some(arr) = p.get("top_blocked_domains") {
                                            if let Ok(list) = serde_json::from_value::<Vec<TopDomain>>(
                                                arr.clone(),
                                            ) {
                                                st.top_blocked_domains =
                                                    list.into_iter().take(10).collect();
                                            }
                                        }
                                        if let Some(arr) = p.get("top_clients") {
                                            if let Ok(list) = serde_json::from_value::<Vec<TopClient>>(
                                                arr.clone(),
                                            ) {
                                                st.top_clients =
                                                    list.into_iter().take(10).collect();
                                            }
                                        }
                                        if let Some(arr) = p.get("top_queried_domains") {
                                            if let Ok(list) = serde_json::from_value::<Vec<TopDomain>>(
                                                arr.clone(),
                                            ) {
                                                st.top_queried_domains =
                                                    list.into_iter().take(10).collect();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut st) = ws_state.write() {
                        st.ws_connected = false;
                        st.ws_status_msg = format!("OFFLINE ({})", e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    // Websocket handling is spawned above. Background worker no longer polls HTTP metrics here.
}

async fn cache_worker(
    state: Arc<RwLock<AppState>>,
    client: reqwest::Client,
    url: Arc<str>,
    auth_header: Arc<str>,
) {
    let url = url.as_ref();
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Ok(resp) = client
            .get(url)
            .header("Authorization", auth_header.as_ref())
            .send()
            .await
        {
            if let Ok(json) = resp.json::<CacheStats>().await {
                if let Ok(mut st) = state.write() {
                    st.cache_data = json;
                }
            }
        }
    }
}

async fn memory_worker(
    state: Arc<RwLock<AppState>>,
    client: reqwest::Client,
    url: Arc<str>,
    auth_header: Arc<str>,
) {
    let url = url.as_ref();
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        if let Ok(resp) = client
            .get(url)
            .header("Authorization", auth_header.as_ref())
            .send()
            .await
        {
            if let Ok(json) = resp.json::<MemoryStats>().await {
                if let Ok(mut st) = state.write() {
                    st.memory_data = json;
                }
            }
        }
    }
}

async fn metrics_worker(
    state: Arc<RwLock<AppState>>,
    client: reqwest::Client,
    url: Arc<str>,
    auth_header: Arc<str>,
) {
    let url = url.as_ref();
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;

        let mut rules = 0u64;
        let mut sum_block = 0.0f64;
        let mut cnt_block = 0u64;
        let mut sum_cache = 0.0f64;
        let mut cnt_cache = 0u64;
        let mut upstreams: BTreeMap<String, u64> = BTreeMap::new();

        if let Ok(resp) = client
            .get(url)
            .header("Authorization", auth_header.as_ref())
            .send()
            .await
        {
            if let Ok(text) = resp.text().await {
                for line in text.lines() {
                    if line.starts_with("fastadhunter_ruleset_rules") {
                        if let Some(val) = line.split_whitespace().nth(1) {
                            rules = val.parse().unwrap_or(0);
                        }
                    } else if line
                        .starts_with("fastadhunter_query_duration_seconds_sum{stage=\"block\"}")
                    {
                        if let Some(val) = line.split_whitespace().nth(1) {
                            sum_block = val.parse().unwrap_or(0.0);
                        }
                    } else if line
                        .starts_with("fastadhunter_query_duration_seconds_count{stage=\"block\"}")
                    {
                        if let Some(val) = line.split_whitespace().nth(1) {
                            cnt_block = val.parse().unwrap_or(0);
                        }
                    } else if line
                        .starts_with("fastadhunter_query_duration_seconds_sum{stage=\"cache_hit\"}")
                    {
                        if let Some(val) = line.split_whitespace().nth(1) {
                            sum_cache = val.parse().unwrap_or(0.0);
                        }
                    } else if line.starts_with(
                        "fastadhunter_query_duration_seconds_count{stage=\"cache_hit\"}",
                    ) {
                        if let Some(val) = line.split_whitespace().nth(1) {
                            cnt_cache = val.parse().unwrap_or(0);
                        }
                    } else if line.starts_with("fastadhunter_upstream_attempts_total{address=\"") {
                        if let Some(ip) = line
                            .split("address=\"")
                            .nth(1)
                            .and_then(|s| s.split('"').next())
                        {
                            let count = line
                                .split_whitespace()
                                .nth(1)
                                .unwrap_or("0")
                                .parse::<u64>()
                                .unwrap_or(0);
                            upstreams.insert(ip.to_string(), count);
                        }
                    }
                }
            }
        }

        if let Ok(mut st) = state.write() {
            st.rules_count = rules;

            let mut iter = upstreams.iter();
            if let Some((ip, count)) = iter.next() {
                st.p_upstream_ip = ip.clone();
                st.p_dns_counter = *count;
            }
            if let Some((ip, count)) = iter.next() {
                st.s_upstream_ip = ip.clone();
                st.s_dns_counter = *count;
            }

            if cnt_block > 0 {
                st.avg_block_str = format!("{:.3} ms", (sum_block / cnt_block as f64) * 1000.0);
            }
            if cnt_cache > 0 {
                st.avg_cache_str = format!("{:.3} ms", (sum_cache / cnt_cache as f64) * 1000.0);
            }
        }
    }
}

fn sexy_bar<'a>(p: f64, w: usize, fill_color: Color) -> Vec<Span<'a>> {
    let p = p.clamp(0.0, 100.0);
    let total_eighths = ((p / 100.0) * (w * 8) as f64).round() as usize;
    let full_blocks = total_eighths / 8;
    let remainder = total_eighths % 8;

    let partials = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

    let mut filled_str = "█".repeat(full_blocks);
    if remainder > 0 && full_blocks < w {
        filled_str.push_str(partials[remainder]);
    }

    let filled_chars = full_blocks + if remainder > 0 { 1 } else { 0 };
    let empty_str = "━".repeat(w.saturating_sub(filled_chars));

    vec![
        Span::styled(filled_str, Style::default().fg(fill_color)),
        Span::styled(empty_str, Style::default().fg(Color::Rgb(45, 50, 60))),
    ]
}

fn draw_multi_row_braille(data: &[f64], width: usize, height: usize) -> Vec<String> {
    if data.is_empty() || width == 0 {
        return vec![" ".repeat(width); height];
    }

    let min_v = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_v = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let total_pts = data.len();

    let total_sub_cols = width * 2;
    let mut sampled = Vec::with_capacity(total_sub_cols);
    for i in 0..total_sub_cols {
        let idx = (i * (total_pts - 1)) / (total_sub_cols - 1).max(1);
        sampled.push(data[idx.min(total_pts - 1)]);
    }

    let total_dots_y = height * 4;
    let dot_map: [[u32; 2]; 4] = [[0x1, 0x8], [0x2, 0x10], [0x4, 0x20], [0x40, 0x80]];

    let mut rows_text = Vec::with_capacity(height);

    for r in 0..height {
        let mut row_str = String::with_capacity(width);
        let y_offset = r * 4;

        for c in 0..width {
            let sub_x0 = c * 2;
            let sub_x1 = sub_x0 + 1;

            let val0 = sampled[sub_x0];
            let val1 = sampled[sub_x1];

            let h_dots0 = if (max_v - min_v).abs() < f64::EPSILON {
                1
            } else {
                (((val0 - min_v) / (max_v - min_v)) * (total_dots_y as f64 - 1.0) + 1.0) as usize
            }
            .clamp(1, total_dots_y);

            let h_dots1 = if (max_v - min_v).abs() < f64::EPSILON {
                1
            } else {
                (((val1 - min_v) / (max_v - min_v)) * (total_dots_y as f64 - 1.0) + 1.0) as usize
            }
            .clamp(1, total_dots_y);

            let mut char_code = 0x2800;

            for (dy, dots) in dot_map.iter().enumerate() {
                let gy = y_offset + dy;
                let target_y = total_dots_y - 1 - gy;

                if target_y < h_dots0 {
                    char_code |= dots[0];
                }
                if target_y < h_dots1 {
                    char_code |= dots[1];
                }
            }

            row_str.push(char::from_u32(char_code).unwrap_or(' '));
        }
        rows_text.push(row_str);
    }

    rows_text
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

async fn run_ui<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn Error>> {
    let bar_width = 12;
    let mut table_state = TableState::default();
    let mut last_perf_update = Instant::now();
    const PERF_UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60); // 5 minutes

    loop {
        terminal.draw(|f| {
            let st = state.read().unwrap();
            let size = f.size();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(7), Constraint::Min(10), Constraint::Length(4)])
                .split(size);

            let status_color = if st.ws_connected { Color::Green } else { Color::Red };
            let time_str = chrono::Local::now().format("%H:%M:%S").to_string();

            let rss = st.memory_data.process_rss / 1024.0 / 1024.0;
            let peak = st.memory_data.process_peak_rss / 1024.0 / 1024.0;
            let ruleset_mb = st.memory_data.ruleset_bytes / 1024.0 / 1024.0;
            let alloc_peak_mb = st.memory_data.allocator_committed_peak_bytes / 1024.0 / 1024.0;

            let hits = st.cache_data.hits;
            let misses = st.cache_data.misses;
            let hit_p = if hits + misses > 0 { (hits as f64 * 100.0) / ((hits + misses) as f64) } else { 0.0 };

            let c_bytes = st.cache_data.bytes / 1024.0 / 1024.0;
            let load = st.cache_data.load_percent;
            let entries = st.cache_data.entries;
            let capacity = st.cache_data.capacity;
            let fresh = st.cache_data.fresh;
            let stale = st.cache_data.stale;

            let mut display_pts: Vec<f64> = st.perf_points.iter().cloned().collect();
            if display_pts.is_empty() { display_pts.push(rss); }

            let min_rss = display_pts.iter().cloned().fold(f64::INFINITY, f64::min).round() as u64;
            let avg_rss = (display_pts.iter().sum::<f64>() / display_pts.len() as f64).round() as u64;
            let now_rss = rss.round() as u64;

            let col2_w = 23; let col3_w = 18; let col4_w = 20;
            let rss_percent_1024 = (rss / 1024.0) * 100.0;

            let col2_str_l3 = format!("Peak {:5.1} MB", peak);
            let col3_str_l3 = format!("Rules {:4.1} MB", ruleset_mb);
            let col4_str_l3 = format!("Alloc {:5.1} MB", alloc_peak_mb);

            let col2_str_l4 = format!("Hit: {} - Miss: {}", hits, misses);
            let col3_str_l4 = format!("DNS L: {}", st.avg_block_str);
            let col4_str_l4 = format!("Cache H: {}", st.avg_cache_str);

            let col2_str_l5 = format!("{}/{} ({:.1}MB)", entries, capacity, c_bytes);
            let col3_str_l5 = format!("Fresh {}", fresh);
            let col4_str_l5 = format!("Stale {}", stale);

            let plain_l3 = format!(
                " RSS   {} {:5.1} MB │ {:<width2$} │ {:<width3$} │ {:<width4$}",
                "█".repeat(bar_width), rss, col2_str_l3, col3_str_l3, col4_str_l3,
                width2 = col2_w, width3 = col3_w, width4 = col4_w
            );

            let graph_width = size.width.saturating_sub(plain_l3.chars().count() as u16 + 2) as usize;
            let graph_rows = draw_multi_row_braille(&display_pts, graph_width, 3);

            let lbl_graph = format!(" 24h RSS History (Min {} │ Avg {} │ Now {} MB) ", min_rss, avg_rss, now_rss);
            let graph_hdr = if graph_width > lbl_graph.len() {
                let p_left = (graph_width - lbl_graph.len()) / 2;
                let p_right = graph_width - lbl_graph.len() - p_left;
                format!("{}{}{}", "─".repeat(p_left), lbl_graph, "─".repeat(p_right))
            } else {
                "─".repeat(graph_width)
            };

            let header_lines = vec![
                Line::from(vec![
                    Span::styled(" FastAdHunter Monitor ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw("   "),
                    Span::styled(format!("● {}", st.ws_status_msg), Style::default().fg(status_color)),
                    Span::raw(format!(" │ Rules: {},{:03}", st.rules_count / 1000, st.rules_count % 1000)),
                    Span::raw(" ".repeat(size.width.saturating_sub(65) as usize)),
                    Span::styled(time_str, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::raw(format!(" {}", "─".repeat(plain_l3.chars().count() - 1))),
                    Span::raw("┬"),
                    Span::raw(graph_hdr),
                ]),
                Line::from({
                    let mut spans = vec![Span::raw(" RSS   ")];
                    spans.extend(sexy_bar(rss_percent_1024, bar_width, Color::Cyan));
                    spans.push(Span::raw(format!(" {:5.1} MB │ {:<width2$} │ {:<width3$} │ {:<width4$}│", rss, col2_str_l3, col3_str_l3, col4_str_l3, width2 = col2_w, width3 = col3_w, width4 = col4_w)));
                    spans.push(Span::styled(&graph_rows[0], Style::default().fg(Color::Red)));
                    spans
                }),
                Line::from({
                    let mut spans = vec![Span::raw(" Hit   ")];
                    spans.extend(sexy_bar(hit_p, bar_width, Color::Green));
                    spans.push(Span::raw(format!(" {:5.1}%   │ {:<width2$} │ {:<width3$} │ {:<width4$}│", hit_p, col2_str_l4, col3_str_l4, col4_str_l4, width2 = col2_w, width3 = col3_w, width4 = col4_w)));
                    spans.push(Span::styled(&graph_rows[1], Style::default().fg(Color::Yellow)));
                    spans
                }),
                Line::from({
                    let mut spans = vec![Span::raw(" Cache ")];
                    spans.extend(sexy_bar(load, bar_width, Color::Yellow));
                    spans.push(Span::raw(format!(" {:5.1}%   │ {:<width2$} │ {:<width3$} │ {:<width4$}│", load, col2_str_l5, col3_str_l5, col4_str_l5, width2 = col2_w, width3 = col3_w, width4 = col4_w)));
                    spans.push(Span::styled(&graph_rows[2], Style::default().fg(Color::Green)));
                    spans
                }),
            ];

            f.render_widget(Paragraph::new(header_lines).block(Block::default().borders(Borders::ALL).title("Status")), chunks[0]);

            let body_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(45), Constraint::Min(40)])
                .split(chunks[1]);

            let left_panel_inner_width: usize = 45_usize.saturating_sub(2);
            let bar_w = left_panel_inner_width.saturating_sub(20).max(12);

            let mut stats_text = vec![
                Line::from({
                    let mut spans = vec![Span::styled("Blocked Rate: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))];
                    spans.extend(sexy_bar(st.blocked_percent, bar_w, Color::Red));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(format!("{:5.1}%", st.blocked_percent), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                    spans
                }),
                Line::from({
                    let mut spans = vec![Span::styled("Cache Hit:    ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))];
                    spans.extend(sexy_bar(st.cache_hit_percent, bar_w, Color::Green));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(format!("{:5.1}%", st.cache_hit_percent), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
                    spans
                }),
                Line::from(format!("Total: {} │ Blocked: {}", st.queries_total, st.blocked_total)),
                Line::from(""),
                Line::from(Span::styled("── Today ──", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(format!(" • Total Queries   = {:>10}", st.summary_24h_total_queries)),
                Line::from(format!(" • Blocked Queries = {:>10} ({:>6.2}%)", st.summary_24h_blocked, st.summary_24h_blocked_percent)),
                Line::from(format!(" • Cache Hits      = {:>10} ({:>6.2}%)", st.summary_24h_cache_hits, st.summary_24h_cache_hit_percent)),
                Line::from(format!(" • A               = {:>10} ({:>6.2}%)", st.summary_24h_a, st.summary_24h_a_percent)),
                Line::from(format!(" • AAAA            = {:>10} ({:>6.2}%)", st.summary_24h_aaaa, st.summary_24h_aaaa_percent)),
                Line::from(format!(" • HTTPS           = {:>10} ({:>6.2}%)", st.summary_24h_https, st.summary_24h_https_percent)),

                Line::from(""),
                Line::from(Span::styled("── Last 7Days ──", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(format!(" • Total Queries   = {:>10}", st.summary_7d_total_queries)),
                Line::from(format!(" • Blocked Queries = {:>10} ({:>6.2}%)", st.summary_7d_blocked, st.summary_7d_blocked_percent)),
                Line::from(format!(" • Cache Hits      = {:>10} ({:>6.2}%)", st.summary_7d_cache_hits, st.summary_7d_cache_hit_percent)),
                Line::from(format!(" • A               = {:>10} ({:>6.2}%)", st.summary_7d_a, st.summary_7d_a_percent)),
                Line::from(format!(" • AAAA            = {:>10} ({:>6.2}%)", st.summary_7d_aaaa, st.summary_7d_aaaa_percent)),
                Line::from(format!(" • HTTPS           = {:>10} ({:>6.2}%)", st.summary_7d_https, st.summary_7d_https_percent)),
            ];

            stats_text.push(Line::from(""));
            stats_text.push(Line::from(Span::styled("── Top Blocked Domains (Top 10) ──", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));

            for dom in &st.top_blocked_domains {
                let name = if dom.domain.len() > 28 { &dom.domain[..28] } else { &dom.domain };
                stats_text.push(Line::from(format!(" • {:<28} {:>6}", name, dom.count)));
            }

            stats_text.push(Line::from(""));
            stats_text.push(Line::from(Span::styled("── Top Clients (Top 10) ──", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));

            for client in &st.top_clients {
                let ip = if client.ip.len() > 28 { &client.ip[..28] } else { &client.ip };
                stats_text.push(Line::from(format!(" • {:<28} {:>6}", ip, client.count)));
            }

            stats_text.push(Line::from(""));
            stats_text.push(Line::from(Span::styled("── Top Queried Domains (Top 10) ──", Style::default().fg(Color::Cyan))));

            for dom in &st.top_queried_domains {
                let name = if dom.domain.len() > 28 { &dom.domain[..28] } else { &dom.domain };
                stats_text.push(Line::from(format!(" • {:<28} {:>6}", name, dom.count)));
            }

            let stats_len = stats_text.len() as u16;
            let stats_view_height = body_chunks[0].height.saturating_sub(2);
            let max_stats_scroll = stats_len.saturating_sub(stats_view_height);
            let current_stats_scroll = st.stats_scroll.min(max_stats_scroll);

            let stats_paragraph = Paragraph::new(stats_text)
                .block(Block::default().borders(Borders::ALL).title("Metrics & Top Stats (24h)"))
                .scroll((current_stats_scroll, 0));

            f.render_widget(stats_paragraph, body_chunks[0]);

            let stats_scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));
            let mut stats_scrollbar_state = ScrollbarState::new(max_stats_scroll as usize)
                .position(current_stats_scroll as usize);

            f.render_stateful_widget(stats_scrollbar, body_chunks[0], &mut stats_scrollbar_state);

            let client_w = 38;
            let type_w = 8;
            let verdict_w = 10;
            let cache_col_w = 8;
            let time_w = 9;

            let header = Row::new(vec![
                Cell::from("Client"),
                Cell::from("Domain"),
                Cell::from("Type"),
                Cell::from("Verdict"),
                Cell::from("Cache"),
                Cell::from("Time (ms)"),
            ])
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED));

            let mut rows = Vec::new();
            for q in &st.queries {
                let cache_str = if q.is_cached { "Yes" } else { "No" };
                let time_val = format!("{:.3}", q.duration_ms);
                let color = if q.verdict.to_uppercase().contains("BLOCK") { Color::Red } else { Color::Green };

                rows.push(
                    Row::new(vec![
                        Cell::from(q.client.clone()),
                        Cell::from(q.domain.clone()),
                        Cell::from(q.qtype.clone()),
                        Cell::from(q.verdict.clone()),
                        Cell::from(cache_str),
                        Cell::from(time_val),
                    ])
                    .style(Style::default().fg(color))
                );
            }

            let table = Table::new(
                rows,
                [
                    Constraint::Length(client_w as u16),
                    Constraint::Min(10),
                    Constraint::Length(type_w as u16),
                    Constraint::Length(verdict_w as u16),
                    Constraint::Length(cache_col_w as u16),
                    Constraint::Length(time_w as u16),
                ]
            )
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Live Queries Feed"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

            if !st.queries.is_empty() {
                let max_scroll = st.queries.len().saturating_sub(1);
                table_state.select(Some(st.query_scroll.min(max_scroll)));
            }

            f.render_stateful_widget(table, body_chunks[1], &mut table_state);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));
            let mut scrollbar_state = ScrollbarState::new(st.queries.len())
                .position(st.query_scroll);
            f.render_stateful_widget(scrollbar, body_chunks[1], &mut scrollbar_state);

            let footer_text = vec![
                Line::from(format!(
                    " RouterOS: Free Mem: {} │ CPU Freq: {} │ CPU Load: {} │ Container Mem: {} │ {}: {} │ {}: {} │ ↑/↓: Queries ",
                    st.router_free_mem,
                    st.router_cpu_freq,
                    st.router_cpu_load,
                    st.router_container_mem,
                    st.p_upstream_ip, st.p_dns_counter, st.s_upstream_ip, st.s_dns_counter
                )),
            ];
            f.render_widget(Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL)), chunks[2]);

            // --- RENDER POPUP CENTER ---
            if let Some(ref q) = st.popup_query {
                let area = centered_rect(60, 40, size);
                f.render_widget(Clear, area);

                let color = if q.verdict.to_uppercase().contains("BLOCK") { Color::Red } else { Color::Green };
                let popup_text = vec![
                    Line::from(vec![Span::styled("Client:    ", Style::default().fg(Color::DarkGray)), Span::raw(&q.client)]),
                    Line::from(vec![Span::styled("Domain:    ", Style::default().fg(Color::DarkGray)), Span::styled(&q.domain, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
                    Line::from(vec![Span::styled("Type:      ", Style::default().fg(Color::DarkGray)), Span::raw(&q.qtype)]),
                    Line::from(vec![Span::styled("Verdict:   ", Style::default().fg(Color::DarkGray)), Span::styled(&q.verdict, Style::default().fg(color).add_modifier(Modifier::BOLD))]),
                    Line::from(vec![Span::styled("Cached:    ", Style::default().fg(Color::DarkGray)), Span::raw(if q.is_cached { "Yes" } else { "No" })]),
                    Line::from(vec![Span::styled("Duration:  ", Style::default().fg(Color::DarkGray)), Span::raw(format!("{:.3} ms", q.duration_ms))]),
                    Line::from(vec![Span::styled("Timestamp: ", Style::default().fg(Color::DarkGray)), Span::raw(&q.ts)]),
                ];

                let popup = Paragraph::new(popup_text)
                    .block(Block::default().borders(Borders::ALL).title(" Query Details ").style(Style::default().fg(Color::Yellow)))
                    .wrap(ratatui::widgets::Wrap { trim: true });

                f.render_widget(popup, area);
            }
            drop(st);  // Release read lock
        })?;

        // Bootstrap history is loaded once; keep the graph moving with live RSS samples.
        if last_perf_update.elapsed() >= PERF_UPDATE_INTERVAL {
            if let Ok(mut state_lock) = state.write() {
                let rss_val = state_lock.memory_data.process_rss / 1024.0 / 1024.0;
                if state_lock.perf_points.len() >= 288 {
                    state_lock.perf_points.pop_front();
                }
                state_lock.perf_points.push_back(rss_val);
            }
            last_perf_update = Instant::now();
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => {
                        if let Ok(mut st) = state.write() {
                            st.popup_query = None;
                        }
                    }
                    KeyCode::Enter => {
                        if let Ok(mut st) = state.write() {
                            if st.popup_query.is_none() {
                                let idx = st.query_scroll;
                                if let Some(q) = st.queries.get(idx).cloned() {
                                    st.query_scroll = idx;
                                    st.popup_query = Some(q);
                                }
                            }
                        }
                    }
                    KeyCode::Char('w') => {
                        if let Ok(mut st) = state.write() {
                            st.stats_scroll = st.stats_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Ok(mut st) = state.write() {
                            st.stats_scroll = st.stats_scroll.saturating_add(1);
                        }
                    }
                    KeyCode::Up => {
                        if let Ok(mut st) = state.write() {
                            st.query_scroll = st.query_scroll.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if let Ok(mut st) = state.write() {
                            st.query_scroll = st.query_scroll.saturating_add(1);
                        }
                    }
                    KeyCode::PageUp => {
                        if let Ok(mut st) = state.write() {
                            st.query_scroll = st.query_scroll.saturating_sub(10);
                        }
                    }
                    KeyCode::PageDown => {
                        if let Ok(mut st) = state.write() {
                            st.query_scroll = st.query_scroll.saturating_add(10);
                        }
                    }
                    _ => {}
                },

                Event::Mouse(mouse) => {
                    if let Ok(mut st) = state.write() {
                        let is_left_panel = mouse.column < 45;

                        match mouse.kind {
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                if st.popup_query.is_some() {
                                    // Popup is open: close only if clicking outside
                                    let size = terminal.size().unwrap_or_default();
                                    let popup_area = centered_rect(
                                        60,
                                        40,
                                        ratatui::layout::Rect {
                                            x: 0,
                                            y: 0,
                                            width: size.width,
                                            height: size.height,
                                        },
                                    );

                                    let click_in_popup = mouse.column >= popup_area.x
                                        && mouse.column < popup_area.x + popup_area.width
                                        && mouse.row >= popup_area.y
                                        && mouse.row < popup_area.y + popup_area.height;

                                    if !click_in_popup {
                                        st.popup_query = None;
                                    }
                                } else if !is_left_panel {
                                    // No popup: open query details
                                    let table_y = 9;
                                    if mouse.row >= table_y {
                                        let click_offset = (mouse.row - table_y) as usize;
                                        let selected_idx =
                                            table_state.offset().saturating_add(click_offset);
                                        if let Some(q) = st.queries.get(selected_idx).cloned() {
                                            st.query_scroll = selected_idx;
                                            st.popup_query = Some(q);
                                        }
                                    }
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                if is_left_panel {
                                    st.stats_scroll = st.stats_scroll.saturating_sub(1);
                                } else {
                                    st.query_scroll = st.query_scroll.saturating_sub(1);
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                if is_left_panel {
                                    st.stats_scroll = st.stats_scroll.saturating_add(1);
                                } else {
                                    st.query_scroll = st.query_scroll.saturating_add(1);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}
