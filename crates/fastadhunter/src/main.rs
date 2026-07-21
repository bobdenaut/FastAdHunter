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
mod privilege;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

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
        rules.boot().await;
        tracing::info!(rules = rules.matcher().len(), "ruleset compiled from cache");

        // ── Observers (L3) ──
        let stats = Arc::new(fah_stats::Stats::new(
            &config.stats,
            &config.query_log,
            data_dir.to_path_buf(),
        ));
        stats.boot().await;
        let metrics = Arc::new(fah_metrics::Metrics::new());

        // ── DNS engine (L3) ──
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let pipeline = Arc::new(fah_dns::Pipeline::new(
            Arc::clone(&rules),
            upstreams.clone(),
            config.dns.blocking.ttl_seconds,
            &config.dns.cache,
            events_tx,
        ));
        let mut dns = fah_dns::Server::bind(&config.dns.listen).await?;
        tracing::info!(udp = %dns.udp_addr(), tcp = %dns.tcp_addr(), "DNS listeners bound");

        // ── Privilege drop (ADR-0004) ──
        // Port 53 is the only thing here that needs root, and it is now bound.
        // Everything below runs unprivileged: the API listens on 8443, and the
        // API key, TLS certificate and every later write to /config and /data
        // are created as the service user rather than root. The DNS listeners
        // are spawned *after* this point, so no query is ever answered by a
        // privileged process.
        privilege::drop_to_service_user(&[config_dir, data_dir])?;

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
        let api = fah_api::ApiServer::bind(
            &api_address,
            api_port,
            tls,
            fah_api::AppStateBuilder {
                rules: Arc::clone(&rules),
                stats: Arc::new(adapters::StatsAdapter::new(Arc::clone(&stats))),
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

        // ── The edges between the siblings ──
        let tasks = vec![
            rules.spawn_scheduler(),
            stats.spawn_snapshot_scheduler(),
            stats.spawn_query_log_scheduler(),
            spawn_event_fanout(
                events_rx,
                Arc::clone(&stats),
                Arc::clone(&metrics),
                api.events(),
            ),
            spawn_telemetry_poll(metrics, rules, pipeline, upstreams),
        ];

        Ok(Self { dns, api, tasks })
    }

    fn shutdown(&self) {
        self.dns.shutdown();
        self.api.shutdown();
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// The one consumer of the pipeline's `QueryEvent` channel, feeding all three
/// observers. A single channel plus this fan-out keeps the producer side at
/// one `try_send` per query — the hot path pays for one channel, not three.
fn spawn_event_fanout(
    mut events: tokio::sync::mpsc::Receiver<fah_model::QueryEvent>,
    stats: Arc<fah_stats::Stats>,
    metrics: Arc<fah_metrics::Metrics>,
    hub: fah_api::EventHub,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            metrics.record(&event);
            // The WS publish work (a clone, a boxed record, a client-name
            // lookup) is only bought when a dashboard is actually connected —
            // no-subscribers is the appliance's idle state ~24h/day.
            if hub.has_subscribers() {
                let client_ip = event.query.client_ip;
                stats.record(event.clone());
                // Resolved after `record` so a first-ever query already
                // carries whatever name the registry has.
                let client_name = stats.client_name(client_ip);
                hub.publish_query(event, client_name);
            } else {
                stats.record(event);
            }
        }
    })
}

/// Refreshes the metrics that are read rather than pushed: the pipeline's
/// channel-drop counter, per-upstream health, and the compiled ruleset's size.
fn spawn_telemetry_poll(
    metrics: Arc<fah_metrics::Metrics>,
    rules: Arc<fah_rules::ListManager>,
    pipeline: Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    upstreams: fah_dns::UpstreamPool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TELEMETRY_POLL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;

            metrics.set_dropped_events(pipeline.dropped_events());
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

            let matcher = rules.matcher();
            metrics.set_ruleset(fah_metrics::RulesetSnapshot {
                rules: matcher.len(),
                heap_bytes: matcher.heap_bytes(),
                // Compile timing belongs to the lifecycle, which does not
                // report it yet — and because this poll overwrites the whole
                // snapshot every tick, wiring it up later must give compile
                // duration its own setter written on compile events, not a
                // field here (anything set here is erased within 10 s).
                compile_duration: std::time::Duration::ZERO,
            });
        }
    })
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
