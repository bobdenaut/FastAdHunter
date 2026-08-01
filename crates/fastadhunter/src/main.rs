//! Thin binary wiring FastAdHunter's crates together (ARCHITECTURE.md L4).
//!
//! Boot order: parse CLI -> load config -> init logging -> run (or
//! healthcheck) -> shutdown. This is the crate ARCHITECTURE.md's layering
//! rules point at: the L3 siblings never import each other, so every edge
//! between them is made here — a `QueryEvent` channel from `fah-dns` fanned
//! out to `fah-stats`, `fah-metrics` and the API's event stream, and
//! `fah-api`'s port traits implemented over the sibling handles
//! ([`adapters`]).

mod adapters;
mod allocator;
mod privilege;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use fah_config::{Config, LogFormat as ConfigLogFormat, LogLevel};
use fah_logging::LogFormat;
use tracing_subscriber::filter::LevelFilter;

const DEFAULT_CONFIG_PATH: &str = "/config/fastadhunter.toml";
const DEFAULT_DATA_PATH: &str = "/data";

/// Bound on the `QueryEvent` channel between the DNS pipeline and the
/// observers. A full channel drops events rather than back-pressuring the
/// hot path (ARCHITECTURE.md §Runtime Model); the drops are counted and
/// exported as `fastadhunter_events_dropped_total`.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

/// How often the effective client → policy map is re-evaluated. Not
/// configurable: it is the precision of a schedule boundary, not a preference.
const POLICY_TICK: Duration = Duration::from_secs(20);

/// How often the observers' pull-based figures are refreshed: channel drops,
/// per-upstream counters, compiled-ruleset size. Cheap reads, but no reason
/// to do them per query.
const TELEMETRY_POLL: std::time::Duration = std::time::Duration::from_secs(10);

const USAGE: &str = "\
fastadhunter — network-wide ad blocker (DNS filtering)

USAGE:
    fastadhunter [OPTIONS]

OPTIONS:
    --config <PATH>    Config file path (default: /config/fastadhunter.toml)
    --data <PATH>      Data directory (default: /data)
    --healthcheck      Load and validate config, then exit 0/1 (no side effects)
    --version          Print version and exit
    --help, -h         Print this help and exit";

#[derive(Debug)]
struct Args {
    config_path: PathBuf,
    data_dir: PathBuf,
    healthcheck: bool,
    version: bool,
    help: bool,
}

fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<Args, String> {
    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    let mut data_dir = PathBuf::from(DEFAULT_DATA_PATH);
    let mut healthcheck = false;
    let mut version = false;
    let mut help = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--config requires a path argument".to_string())?;
                config_path = PathBuf::from(value);
            }
            "--data" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--data requires a path argument".to_string())?;
                data_dir = PathBuf::from(value);
            }
            "--healthcheck" => healthcheck = true,
            "--version" => version = true,
            "--help" | "-h" => help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        config_path,
        data_dir,
        healthcheck,
        version,
        help,
    })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("fastadhunter: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    if args.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    if args.version {
        println!("fastadhunter {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    if args.healthcheck {
        // Process-level by design: config loads and validates, exit 0/1
        // (ARCHITECTURE.md §Docker: distroless has no shell, so the container
        // `HEALTHCHECK` self-execs this binary). Read-only — a probe must not
        // create the config file whose absence would signal a problem.
        //
        // Deliberately *not* an HTTP probe of `GET /health`: the API listens
        // on TLS with a self-signed certificate, so a self-probe would have
        // to disable verification — turning the healthcheck into a second,
        // weaker path to the admin surface. Liveness of the process is what
        // Docker needs; `GET /health` is for operators and dashboards.
        return match Config::load_readonly(&args.config_path) {
            Ok(_) => {
                println!(
                    "fastadhunter: healthcheck ok ({})",
                    args.config_path.display()
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("fastadhunter: healthcheck failed: {err}");
                ExitCode::FAILURE
            }
        };
    }

    let config = match Config::load(&args.config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("fastadhunter: failed to load config: {err}");
            return ExitCode::FAILURE;
        }
    };

    run(config, &args.config_path, &args.data_dir)
}

fn run(config: Config, config_path: &Path, data_dir: &Path) -> ExitCode {
    let _logging = fah_logging::init(
        level_filter(config.log.level),
        log_format(config.log.format),
    );

    tracing::info!(
        config_path = %config_path.display(),
        data_dir = %data_dir.display(),
        mode = ?config.engine.mode,
        "fastadhunter starting"
    );

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("fastadhunter: failed to start Tokio runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async {
        let engine = Engine::start(config, config_path, data_dir).await?;
        await_shutdown().await;
        engine.shutdown();
        Ok::<_, Box<dyn std::error::Error>>(())
    });

    if let Err(err) = result {
        tracing::error!(error = %err, "fastadhunter failed to start");
        eprintln!("fastadhunter: {err}");
        return ExitCode::FAILURE;
    }

    tracing::info!("fastadhunter shutting down");
    ExitCode::SUCCESS
}

/// Everything running, kept together so shutdown can stop it all.
struct Engine {
    dns: fah_dns::Server,
    /// `None` in `dns` mode — the HTTP port is then never bound, not bound and
    /// left idle (CONTEXT.md §Operating Mode).
    http: Option<fah_http::Server>,
    api: fah_api::ApiServer,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Engine {
    /// Wires the whole system. The order matters: the Rule Engine has to be
    /// compiled from `/data` before the listeners bind, or the first queries
    /// through would be answered against an empty ruleset.
    async fn start(
        config: Config,
        config_path: &Path,
        data_dir: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = config_path.parent().unwrap_or(Path::new("."));

        // ── Upstreams (L3) ──
        // Built first because the Rule Engine's list fetcher resolves download
        // hosts through them (see `adapters::UpstreamResolver`). Cheap and
        // network-free to construct — the servers are IP literals, so nothing
        // here needs name resolution and there is no bootstrap cycle.
        let upstreams = fah_dns::UpstreamPool::from_config(&config.dns.upstreams)?;

        // ── Rule Engine (L2) ──
        let rules = Arc::new(fah_rules::ListManager::with_resolver(
            &config.rules,
            data_dir.to_path_buf(),
            Arc::new(adapters::UpstreamResolver::new(upstreams.clone())),
        )?);
        // Before `boot`, which compiles: the policy set decides the per-rule
        // masks the compiled ruleset carries, so setting it afterwards would
        // leave the first ruleset built for the wrong policies.
        //
        // `Config::validate` already rejected anything malformed, so a failure
        // here is a bug rather than operator error — logged and degraded to the
        // single default policy (every client, every enabled list), never a
        // silent partial policy set.
        match fah_rules::PolicySet::from_config(&config.schedule.timezone, &config.policies) {
            Ok(policies) => rules.set_policies(policies),
            Err(error) => tracing::error!(
                %error,
                "policies failed to compile after validation — serving every client the \
                 default policy"
            ),
        }
        rules.boot().await;
        tracing::info!(
            rules = rules.matcher().len(),
            policies = rules.policies().len(),
            "ruleset compiled from cache"
        );

        // The client → policy map both pipelines read. The binary owns it —
        // `fah-rules` compiles policies but does not hold the clock or the
        // client registry (p2-06); the tick below is what re-evaluates it.
        let policy_state = Arc::new(fah_rules::PolicyState::default());

        // ── Observers (L3) ──
        let stats = Arc::new(fah_stats::Stats::new(
            &config.stats,
            &config.query_log,
            &config.history,
            data_dir.to_path_buf(),
        ));
        stats.boot().await;
        // Captured before `config` is moved into the ConfigStore below; the
        // perf sampler's cadence is boot-class (see `spawn_perf_sampler`).
        let perf_sample_interval_seconds = config.history.sample_interval_seconds;
        let metrics = Arc::new(fah_metrics::Metrics::new());

        // ── DNS engine (L3) ──
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let pipeline = Arc::new(
            fah_dns::Pipeline::new(
                Arc::clone(&rules),
                upstreams.clone(),
                config.dns.blocking.ttl_seconds,
                &config.dns.cache,
                events_tx.clone(),
            )
            .with_policies(Arc::clone(&policy_state)),
        );
        let mut dns = fah_dns::Server::bind(&config.dns.listen).await?;
        tracing::info!(udp = %dns.udp_addr(), tcp = %dns.tcp_addr(), "DNS listeners bound");

        // ── HTTP engine (L3, p2-01) ──
        // Bound here, with DNS, so both privileged binds happen before the
        // drop below — port 8080 does not need privilege, but the ordering is
        // what lets an operator move it to 80 where a runtime permits it,
        // without the binary having to care which port it was given.
        //
        // `dns` mode must not bind the port at all. Binding and then not
        // serving would still hold the port against anything else on the host
        // and would still answer a connect(), which is indistinguishable from
        // a hung proxy.
        let mut http = if http_enabled(config.engine.mode) {
            let server = fah_http::Server::bind(&config.http).await?;
            tracing::info!(addr = %server.local_addr(), "HTTP listener bound");
            Some(server)
        } else {
            None
        };

        // The proxy itself (p2-02). Built here, before `config` is moved into
        // the ConfigStore below, and only in a mode that serves HTTP — the
        // upstream connection pool should not exist in `dns` mode.
        let http_proxy = if http.is_some() {
            Some(Arc::new(
                build_http_proxy(&config, upstreams.clone())?
                    // p2-04: the same compiled ruleset the DNS pipeline
                    // answers from, and the same event channel it writes to.
                    .with_rules(Arc::clone(&rules) as Arc<dyn fah_http::Ruleset>)
                    // The same snapshot the DNS pipeline reads, so a client is
                    // judged under one policy by both.
                    .with_policies(Arc::clone(&policy_state))
                    .with_events(events_tx.clone()),
            ))
        } else {
            None
        };

        // ── Privilege drop (ADR-0004) ──
        // Port 53 is the only thing here that needs root, and it is now bound.
        // Everything below runs unprivileged: the API listens on 8443, and the
        // API key, TLS certificate and every later write to /config and /data
        // are created as the service user rather than root. The DNS listeners
        // are spawned *after* this point, so no query is ever answered by a
        // privileged process.
        //
        // The state subdirectories are named explicitly, not just `/data`: the
        // writers above (`rules.boot`, `stats.boot`) already ran as root and
        // may have created `/data/history/*` and `/data/query_log/*` owned by
        // root. When `/data` itself is already the service user's — the seeded
        // image, or any later boot — `reown_if_needed` takes its top-level
        // shortcut and never descends, so those fresh root-owned subtrees would
        // stay unwritable after the drop. Passing them as their own roots
        // reowns each on the next boot (a no-op once already adopted).
        let history_dir = data_dir.join("history");
        let query_log_dir = data_dir.join("query_log");
        privilege::drop_to_service_user(&[config_dir, data_dir, &history_dir, &query_log_dir])?;

        // ── API (L3) ──
        let (keys, generated) = fah_api::ApiKeyStore::load_or_create(config_dir)?;
        if let Some(key) = generated {
            // Printed exactly once, on first boot (SECURITY.md §API access).
            tracing::info!(api_key = %key, "generated API key — store it now; it is not shown again");
        }
        let tls = if config.api.tls {
            Some(fah_api::load_or_generate_tls(config_dir)?)
        } else {
            tracing::warn!(
                "api.tls is disabled — the API key travels in plaintext; see SECURITY.md"
            );
            None
        };

        let api_address = config.api.address.clone();
        let api_port = config.api.port;
        let stats_adapter = Arc::new(adapters::StatsAdapter::new(Arc::clone(&stats)));
        let api = fah_api::ApiServer::bind(
            &api_address,
            api_port,
            tls,
            fah_api::AppStateBuilder {
                rules: Arc::clone(&rules),
                policies: Arc::clone(&policy_state),
                // One adapter, two ports: the live stats handles and the
                // `/data/history` reads both sit on the same `Arc<Stats>`.
                stats: Arc::clone(&stats_adapter) as Arc<dyn fah_api::StatsSource>,
                history: stats_adapter as Arc<dyn fah_api::HistorySource>,
                telemetry: Arc::new(adapters::TelemetryAdapter::new(
                    Arc::clone(&metrics),
                    upstreams.clone(),
                )),
                cache: Arc::new(adapters::CacheAdapter::new(Arc::clone(&pipeline))),
                config: Arc::new(fah_api::ConfigStore::new(config, config_path.to_path_buf())),
                keys: Arc::new(keys),
            },
        )
        .await?;
        tracing::info!(url = %api.base_url(), "API listening");

        // Unprivileged from here — start answering (ADR-0004).
        dns.serve(Arc::clone(&pipeline));
        if let (Some(http), Some(proxy)) = (http.as_mut(), http_proxy.as_ref()) {
            http.serve(Arc::clone(proxy));
        }

        // ── The edges between the siblings ──
        let mut tasks = vec![
            rules.spawn_scheduler(),
            stats.spawn_snapshot_scheduler(),
            stats.spawn_query_log_scheduler(),
            stats.spawn_history_scheduler(),
            spawn_event_fanout(
                events_rx,
                Arc::clone(&stats),
                Arc::clone(&metrics),
                api.events(),
            ),
            spawn_perf_sampler(
                Arc::clone(&stats),
                Arc::clone(&metrics),
                Arc::clone(&pipeline),
                perf_sample_interval_seconds,
            ),
            spawn_telemetry_poll(
                metrics,
                Arc::clone(&rules),
                Arc::clone(&pipeline),
                upstreams,
                Arc::clone(&stats),
            ),
            spawn_policy_ticker(policy_state, rules, Arc::clone(&stats)),
        ];

        // Stale-while-refresh (ADR-0005). Started here rather than in
        // `Pipeline::new` so every long-lived task is aborted from one place on
        // shutdown; empty when `[dns.cache] swr_workers = 0`.
        let swr_workers = pipeline.spawn_swr_workers();
        if !swr_workers.is_empty() {
            tracing::info!(
                workers = swr_workers.len(),
                "stale-while-refresh pool started"
            );
        }
        tasks.extend(swr_workers);

        // Scheduled cache sweep, spawned here for the same reason: the binary
        // owns every long-lived task's lifetime. `None` when
        // `[dns.cache] cleanup_interval_seconds = 0`.
        if let Some(cleanup) = pipeline.spawn_cache_cleanup() {
            tracing::info!("cache cleanup scheduler started");
            tasks.push(cleanup);
        }

        Ok(Self {
            dns,
            http,
            api,
            tasks,
        })
    }

    fn shutdown(&self) {
        self.dns.shutdown();
        if let Some(http) = &self.http {
            http.shutdown();
        }
        self.api.shutdown();
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Whether `engine.mode` includes the HTTP engine.
///
/// Matched exhaustively rather than with a `_ => false` catch-all: adding a
/// fourth mode should fail to compile until someone decides what it means for
/// HTTP, instead of silently defaulting to "off" and leaving an operator with
/// a mode that names http and a port nothing listens on.
fn http_enabled(mode: fah_config::EngineMode) -> bool {
    match mode {
        fah_config::EngineMode::Dns => false,
        fah_config::EngineMode::DnsHttp | fah_config::EngineMode::DnsHttpHttps => true,
    }
}

/// The port a transparent HTTP proxy is the intercepting party for.
///
/// Structural, not configurable: the router dst-nats the LAN's port 80 to the
/// container, so 80 is what clients believe they reached and what a `Host`
/// without an explicit port means (RFC 9110 §4.2.1). The container's own listen
/// port — `[http.listen] port`, 8080 — is a different number and irrelevant
/// here. Phase 3's HTTPS path uses 443 the same way.
const HTTP_ORIGIN_PORT: u16 = 80;

/// Idle upstream connections kept per origin. Bounded so the pool is a function
/// of configuration rather than of how many sites the LAN visits (hard rule 4);
/// a household reuses a handful of connections per site, and anything beyond
/// that is memory held against the 128 MB budget for no gain.
const MAX_IDLE_UPSTREAMS_PER_HOST: usize = 8;

/// Assembles the HTTP proxy from config: the injected resolver port, and the
/// egress policy that decides where it may connect.
///
/// The allow-list is re-parsed here rather than trusted from `fah-config` —
/// that crate is L1 and cannot import `fah_common::egress`, so it can only
/// check the shape. This is the authoritative parse, and it fails startup
/// rather than degrading to a policy the operator did not write.
fn build_http_proxy(
    config: &fah_config::Config,
    upstreams: fah_dns::UpstreamPool,
) -> Result<fah_http::Proxy, Box<dyn std::error::Error>> {
    let exceptions = config
        .egress
        .allow_destinations
        .iter()
        .map(|entry| {
            entry
                .parse::<fah_common::egress::AllowedNet>()
                .map_err(|err| format!("[egress] allow_destinations: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !exceptions.is_empty() {
        // Worth a line at startup: these are deliberate holes in the guard that
        // stops the proxy being an open relay into the LAN.
        tracing::info!(
            count = exceptions.len(),
            "egress allow-list active — private destinations permitted"
        );
    }

    Ok(fah_http::Proxy::new(
        Arc::new(adapters::UpstreamResolver::new(upstreams)),
        fah_common::egress::DestinationPolicy::new(HTTP_ORIGIN_PORT, exceptions),
        HTTP_ORIGIN_PORT,
        Duration::from_millis(config.http.header_timeout_ms),
        Duration::from_millis(config.http.idle_timeout_ms),
        MAX_IDLE_UPSTREAMS_PER_HOST,
        config.egress.allow_ip_literal_hosts,
    ))
}

/// The one consumer of the shared event channel, feeding all three observers.
/// A single channel plus this fan-out keeps each producer at one `try_send` per
/// event — the hot paths pay for one channel, not three, and since p2-04 both
/// pipelines share it so "we shed N events" stays one number.
fn spawn_event_fanout(
    mut events: tokio::sync::mpsc::Receiver<fah_model::Event>,
    stats: Arc<fah_stats::Stats>,
    metrics: Arc<fah_metrics::Metrics>,
    hub: fah_api::EventHub,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            // One channel carries both pipelines since p2-04, so this is the
            // single consumer for both — one shed counter, one fan-out, one
            // place where "a completed thing happened" turns into stats,
            // metrics and a dashboard message.
            let client_ip = event.client_ip();
            match &event {
                fah_model::Event::Dns(query) => metrics.record(query),
                fah_model::Event::Http(request) => metrics.record_http(request),
            }
            // The WS publish work (a clone, a boxed record, a client-name
            // lookup) is only bought when a dashboard is actually connected —
            // no-subscribers is the appliance's idle state ~24h/day.
            let publish = hub.has_subscribers();
            let for_hub = publish.then(|| event.clone());
            match event {
                fah_model::Event::Dns(query) => stats.record(*query),
                fah_model::Event::Http(request) => stats.record_http(*request),
            }
            if let Some(event) = for_hub {
                // Resolved after `record` so a first-ever client already
                // carries whatever name the registry has.
                hub.publish_query(event, stats.client_name(client_ip));
            }
        }
    })
}

/// Refreshes the metrics that are read rather than pushed: the pipeline's
/// channel-drop counter, per-upstream health, the compiled ruleset's size, and
/// the p2-07 memory breakdown.
fn spawn_telemetry_poll(
    metrics: Arc<fah_metrics::Metrics>,
    rules: Arc<fah_rules::ListManager>,
    pipeline: Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    upstreams: fah_dns::UpstreamPool,
    stats: Arc<fah_stats::Stats>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TELEMETRY_POLL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;

            metrics.set_dropped_events(pipeline.dropped_events());
            // Field-by-field rather than a shared type: `fah-dns` and
            // `fah-metrics` are L3 siblings and must not import each other
            // (ARCHITECTURE.md §Dependency Layering), so the binary is the one
            // place allowed to know both shapes.
            let swr = pipeline.swr_stats();
            metrics.set_swr(fah_metrics::SwrSnapshot {
                enqueued: swr.enqueued,
                deduplicated: swr.deduplicated,
                dropped: swr.dropped,
                completed: swr.completed,
                failed: swr.failed,
            });
            let cleanup = pipeline.cache_cleanup_stats();
            metrics.set_cleanup(fah_metrics::CleanupSnapshot {
                runs: cleanup.runs,
                entries_removed: cleanup.entries_removed,
                bytes_freed: cleanup.bytes_freed,
                last_duration_micros: cleanup.last_duration_micros,
            });
            metrics.set_upstreams(
                upstreams
                    .status()
                    .into_iter()
                    .map(|status| fah_metrics::UpstreamSnapshot {
                        address: status.address,
                        protocol: status.protocol,
                        attempts: status.attempts,
                        failures: status.failures,
                        consecutive_failures: status.consecutive_failures,
                        tls_handshakes: status.tls_handshakes,
                    })
                    .collect(),
            );

            // Memory breakdown (p2-07), gathered in ONE pass so
            // `Σ(components) + residual == rss` holds within this snapshot.
            // Reading RSS at a different instant from the components would
            // push the skew into the residual, which is precisely the signal
            // this exists to keep clean.
            //
            // This is also the only layer allowed to see all three sources:
            // fah-rules, fah-dns and fah-stats are L3 siblings that never
            // import each other, and fah-metrics never learns what any of
            // them is — it just receives the finished snapshot.
            // TEMPORARY (post-p2-07): times the WHOLE pass, not `stats.heap()`
            // alone — matcher, cache, stats heaps and the `/proc/self/status`
            // read — because the whole pass is what is paid every 10 s. The
            // 43 µs from `fah-stats/tests/heap_cost.rs` is an x86 figure that
            // excludes the RSS read entirely (no procfs there), so the ARM cost
            // has never actually been measured. Remove once it is known stable.
            //
            // **On the blocking pool, not a DNS worker.** Every read below is
            // synchronous, and on the RB5009 the pass has been measured between
            // 495 µs and 4.86 ms — a 10× spread that tracks cache occupancy.
            // Inline on a `tokio::spawn`ed task that was up to ~5 ms of one of
            // four workers not answering queries, every 10 s: an unbounded tail
            // on the query path, which golden rule 8 (PERFORMANCE.md) exists to
            // forbid. `spawn_blocking` makes the duration irrelevant to latency
            // however far it drifts — the same treatment, for the same reason,
            // that `ListManager::compile` already gets.
            //
            // The whole breakdown is built inside the closure so the "every
            // field sampled at the same instant" invariant above still holds:
            // moving the pass must not smear it across the two schedulers.
            let matcher = rules.matcher();
            let (memory, collection_micros) = {
                let pipeline = Arc::clone(&pipeline);
                let stats = Arc::clone(&stats);
                let matcher = Arc::clone(&matcher);
                tokio::task::spawn_blocking(move || {
                    let collection_started = std::time::Instant::now();
                    let memory = fah_model::MemoryBreakdown {
                        ruleset: matcher.heap_bytes() as u64,
                        cache: pipeline.cache_stats().bytes,
                        stats: stats.heap(),
                        // `0` is this accessor's "couldn't determine" — a
                        // non-Linux dev box, or an unreadable
                        // /proc/self/status. Mapped to `None` so the residual
                        // reports as absent rather than as a fabricated RSS of
                        // zero.
                        rss: match fah_metrics::resident_memory_bytes() {
                            0 => None,
                            bytes => Some(bytes),
                        },
                        // Same instant as the components above. Nothing is
                        // derived from these any more, but `minor_page_faults`
                        // is read as a rate against the query counters sampled
                        // in this pass, and skew between them would land in
                        // that rate.
                        allocator: allocator::stats(),
                    };
                    // Captured before the `warn!` below, so a logging call can
                    // never inflate the number this is meant to report.
                    let micros = collection_started.elapsed().as_micros() as u64;
                    (memory, micros)
                })
                .await
                .expect("memory accounting task panicked")
            };
            if memory.over_accounted() {
                // Impossible in reality: components cannot hold more than the
                // process resides. Means a `heap_bytes` double-counts, or
                // counts something not resident. Logged rather than silently
                // floored at zero, because a wrong instrument is worse than
                // no instrument.
                tracing::warn!(
                    accounted = memory.accounted(),
                    rss = ?memory.rss,
                    "memory accounting exceeds RSS — a component heap_bytes is over-reporting",
                );
            }
            metrics.set_memory(memory);
            metrics.set_memory_collection_micros(collection_micros);

            metrics.set_ruleset(fah_metrics::RulesetSnapshot {
                rules: matcher.len(),
                heap_bytes: matcher.heap_bytes(),
                duplicates_removed: matcher.duplicates_removed(),
                // Pulled from the lifecycle, not pushed on compile events. This
                // poll rewrites the whole snapshot every tick, so a pushed value
                // would be erased within 10 s — which is why this field read
                // `Duration::ZERO` until now. The manager holds the last
                // measurement durably, so re-reading it each tick is correct.
                compile_duration: rules.last_compile_duration(),
            });
        }
    })
}

/// Keeps the client → policy map current, so a schedule window opens and
/// closes without a query ever doing time arithmetic (p2-06).
///
/// [`POLICY_TICK`] is shorter than the minute a schedule is written in: a 60 s
/// tick can be a full minute late at a boundary. The work is a walk over the
/// configured assignments, so paying it three times a minute costs nothing.
fn spawn_policy_ticker(
    policies: Arc<fah_rules::PolicyState>,
    rules: Arc<fah_rules::ListManager>,
    stats: Arc<fah_stats::Stats>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLICY_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            // The first tick fires immediately, which is what publishes the
            // boot snapshot.
            ticker.tick().await;
            let published = policies.refresh(&rules.policies(), &stats.named_clients());
            if published {
                tracing::debug!(
                    active_assignments = policies.current().len(),
                    "policy assignments changed"
                );
            }
        }
    })
}

/// Samples the live perf/system/cache figures on [`PERF_SAMPLE_INTERVAL`] and
/// hands each [`fah_model::PerfSample`] to `fah-stats` to persist. Reads only
/// snapshots — RSS, [`fah_metrics::Metrics::snapshot`], the cache port — never
/// the per-query path (hard rule 3). Keeps the previous metrics snapshot so
/// lifetime-cumulative counters become per-interval rates and percentiles.
fn spawn_perf_sampler(
    stats: Arc<fah_stats::Stats>,
    metrics: Arc<fah_metrics::Metrics>,
    pipeline: Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    sample_interval_seconds: u32,
) -> tokio::task::JoinHandle<()> {
    // Boot-class: the ticker is built once here. `history.retention_days` and
    // `history.enabled` apply live (the latter is re-read each tick below), but
    // a cadence change needs a restart — documented in CONFIGURATION.md.
    let interval = std::time::Duration::from_secs(u64::from(sample_interval_seconds.max(1)));
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let interval_secs = interval.as_secs_f64();
        let mut prev: Option<fah_metrics::MetricsSnapshot> = None;
        loop {
            ticker.tick().await;
            // Re-read live: a disabled history skips even building the sample.
            if !stats.history_enabled() {
                prev = None; // resume with a fresh baseline when re-enabled
                continue;
            }
            let current = metrics.snapshot();
            let cache = pipeline.cache_stats();
            let rss = fah_metrics::resident_memory_bytes();
            let sample = build_perf_sample(&current, prev.as_ref(), &cache, rss, interval_secs);
            stats.persist_perf_sample(sample).await;
            prev = Some(current);
        }
    })
}

/// Assembles one [`fah_model::PerfSample`] from a metrics snapshot (deltaed
/// against the previous one for rates), the cache port, and RSS. Pure — the
/// timestamp is the only ambient read — so the delta/QPS math is unit-testable.
fn build_perf_sample(
    current: &fah_metrics::MetricsSnapshot,
    prev: Option<&fah_metrics::MetricsSnapshot>,
    cache: &fah_dns::CacheStats,
    rss_bytes: u64,
    interval_secs: f64,
) -> fah_model::PerfSample {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let total = |m: &fah_metrics::MetricsSnapshot| {
        m.queries_pass
            .saturating_add(m.queries_allow)
            .saturating_add(m.queries_block)
    };
    let (queries_delta, blocked_delta, allowed_delta) = match prev {
        Some(prev) => (
            total(current).saturating_sub(total(prev)),
            current.queries_block.saturating_sub(prev.queries_block),
            current.queries_allow.saturating_sub(prev.queries_allow),
        ),
        None => (0, 0, 0),
    };
    let qps = if interval_secs > 0.0 {
        queries_delta as f64 / interval_secs
    } else {
        0.0
    };

    fah_model::PerfSample {
        ts,
        rss_bytes,
        qps,
        queries_delta,
        blocked_delta,
        allowed_delta,
        cache: fah_model::CacheStatsSample {
            entries: cache.entries,
            capacity: cache.capacity,
            fresh: cache.fresh,
            stale: cache.stale,
            expired: cache.expired,
            hits: cache.hits,
            misses: cache.misses,
            evictions: cache.evictions,
            bytes: cache.bytes,
            max_bytes: cache.max_bytes,
        },
        latency: latency_summary(current, prev),
        upstreams: current
            .upstreams
            .iter()
            .map(|u| fah_model::UpstreamSample {
                address: u.address.clone(),
                protocol: u.protocol.to_string(),
                attempts: u.attempts,
                failures: u.failures,
                consecutive_failures: u.consecutive_failures,
                tls_handshakes: u.tls_handshakes,
            })
            .collect(),
    }
}

/// Per-stage p50/p99 over the interval since `prev` (or since boot for the
/// first sample), by diffing the cumulative histogram buckets so the
/// percentiles describe the interval rather than the whole process lifetime.
fn latency_summary(
    current: &fah_metrics::MetricsSnapshot,
    prev: Option<&fah_metrics::MetricsSnapshot>,
) -> fah_model::LatencySummary {
    let interval = |cur: &fah_metrics::StageHistogram,
                    prev: Option<&fah_metrics::StageHistogram>| {
        match prev {
            Some(prev) => cur.delta(prev),
            None => cur.clone(),
        }
    };
    let block = interval(&current.block, prev.map(|p| &p.block));
    let cache_hit = interval(&current.cache_hit, prev.map(|p| &p.cache_hit));
    let forward = interval(&current.forward, prev.map(|p| &p.forward));
    fah_model::LatencySummary {
        block_p50: block.quantile(0.5),
        block_p99: block.quantile(0.99),
        cache_hit_p50: cache_hit.quantile(0.5),
        cache_hit_p99: cache_hit.quantile(0.99),
        forward_p50: forward.quantile(0.5),
        forward_p99: forward.quantile(0.99),
    }
}

/// Waits for ctrl-c or, on Unix, SIGTERM — whichever arrives first.
async fn await_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn level_filter(level: LogLevel) -> LevelFilter {
    match level {
        LogLevel::Error => LevelFilter::ERROR,
        LogLevel::Warn => LevelFilter::WARN,
        LogLevel::Info => LevelFilter::INFO,
        LogLevel::Debug => LevelFilter::DEBUG,
        LogLevel::Trace => LevelFilter::TRACE,
    }
}

fn log_format(format: ConfigLogFormat) -> LogFormat {
    match format {
        ConfigLogFormat::Text => LogFormat::Text,
        ConfigLogFormat::Json => LogFormat::Json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dns` must not bring up the HTTP engine, and every mode that names http
    /// must. The acceptance criterion of p2-01 in one assertion pair.
    #[test]
    fn http_starts_only_in_modes_that_name_it() {
        assert!(!http_enabled(fah_config::EngineMode::Dns));
        assert!(http_enabled(fah_config::EngineMode::DnsHttp));
        assert!(http_enabled(fah_config::EngineMode::DnsHttpHttps));
    }

    fn empty_stage() -> fah_metrics::StageHistogram {
        fah_metrics::StageHistogram {
            cumulative: vec![0; 11],
            count: 0,
            sum_seconds: 0.0,
        }
    }

    fn snapshot(pass: u64, allow: u64, block: u64) -> fah_metrics::MetricsSnapshot {
        fah_metrics::MetricsSnapshot {
            queries_pass: pass,
            queries_allow: allow,
            queries_block: block,
            cache_hits: 0,
            cache_misses: 0,
            cache_stale: 0,
            dropped_events: 0,
            requests_pass: 0,
            requests_allow: 0,
            requests_block: 0,
            response_bytes: 0,
            swr: fah_metrics::SwrSnapshot::default(),
            cleanup: fah_metrics::CleanupSnapshot::default(),
            block: empty_stage(),
            cache_hit: empty_stage(),
            forward: empty_stage(),
            request_block: empty_stage(),
            request_forward: empty_stage(),
            upstreams: vec![],
        }
    }

    fn empty_cache() -> fah_dns::CacheStats {
        fah_dns::CacheStats {
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

    #[test]
    fn perf_sample_deltas_and_qps_across_two_snapshots() {
        let prev = snapshot(100, 5, 20);
        let current = snapshot(160, 6, 34); // +60 pass, +1 allow, +14 block = +75

        let sample = build_perf_sample(&current, Some(&prev), &empty_cache(), 1000, 60.0);
        assert_eq!(sample.queries_delta, 75);
        assert_eq!(sample.blocked_delta, 14);
        assert_eq!(sample.allowed_delta, 1);
        assert!((sample.qps - 75.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn perf_sample_first_reading_has_zero_deltas_and_qps() {
        let current = snapshot(160, 6, 34);
        let sample = build_perf_sample(&current, None, &empty_cache(), 1000, 60.0);
        assert_eq!(sample.queries_delta, 0);
        assert_eq!(sample.blocked_delta, 0);
        assert_eq!(sample.allowed_delta, 0);
        assert_eq!(sample.qps, 0.0);
    }

    #[test]
    fn parses_config_flag_and_healthcheck() {
        let args = parse_args(
            ["--config", "/tmp/x.toml", "--healthcheck"]
                .into_iter()
                .map(String::from),
        )
        .unwrap();
        assert_eq!(args.config_path, PathBuf::from("/tmp/x.toml"));
        assert!(args.healthcheck);
        assert!(!args.version);
    }

    #[test]
    fn defaults_to_documented_config_path() {
        let args = parse_args(std::iter::empty()).unwrap();
        assert_eq!(args.config_path, PathBuf::from(DEFAULT_CONFIG_PATH));
    }

    #[test]
    fn parses_help_flags() {
        for flag in ["--help", "-h"] {
            let args = parse_args([flag].into_iter().map(String::from)).unwrap();
            assert!(args.help);
        }
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = parse_args(["--bogus"].into_iter().map(String::from)).unwrap_err();
        assert!(err.contains("--bogus"));
    }

    #[test]
    fn rejects_config_flag_missing_value() {
        let err = parse_args(["--config"].into_iter().map(String::from)).unwrap_err();
        assert!(err.contains("--config"));
    }
}
