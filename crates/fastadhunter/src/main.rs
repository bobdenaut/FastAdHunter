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
mod process;
mod supervisor;

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use fah_config::{Config, LogFormat as ConfigLogFormat, LogLevel};
use fah_logging::LogFormat;
use fah_rules::interception::InterceptionState;
use supervisor::Supervised;
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

const HEALTHCHECK_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

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
        return healthcheck(&args.config_path);
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

fn healthcheck(config_path: &Path) -> ExitCode {
    let config = match Config::load_readonly(config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("fastadhunter: healthcheck failed: config: {err}");
            return ExitCode::FAILURE;
        }
    };

    let listen = match fah_common::listen::listen_addr(
        &config.dns.listen.address,
        config.dns.listen.port,
        "dns.listen",
    ) {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("fastadhunter: healthcheck failed: config: {err}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = probe_dns(probe_target(listen)) {
        eprintln!(
            "fastadhunter: healthcheck failed: dns-probe: {err} — [dns.listen] is a boot \
             setting, so an address or port persisted since the last start applies only \
             after a restart"
        );
        return ExitCode::FAILURE;
    }

    println!("fastadhunter: healthcheck ok ({})", config_path.display());
    ExitCode::SUCCESS
}

fn probe_target(listen: SocketAddr) -> SocketAddr {
    if listen.ip().is_unspecified() {
        SocketAddr::from((Ipv4Addr::LOCALHOST, listen.port()))
    } else {
        listen
    }
}

fn probe_dns(target: SocketAddr) -> io::Result<()> {
    let mut probe = hickory_proto::op::Message::query();
    probe.metadata.op_code = hickory_proto::op::OpCode::Status;
    let id = probe.metadata.id;
    let request = probe
        .to_vec()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    let local = if target.is_ipv6() {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    };
    let socket = UdpSocket::bind(local)?;
    socket.set_read_timeout(Some(HEALTHCHECK_PROBE_TIMEOUT))?;
    socket.send_to(&request, target)?;

    let mut buf = [0u8; 512];
    let (len, _from) = socket
        .recv_from(&mut buf)
        .map_err(|err| io::Error::new(err.kind(), format!("no reply from {target}: {err}")))?;

    if len < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("reply from {target} is {len} bytes, not a DNS message"),
        ));
    }
    if u16::from_be_bytes([buf[0], buf[1]]) != id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("reply from {target} carries another query's id"),
        ));
    }
    Ok(())
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
        let mut engine = Engine::start(config, config_path, data_dir).await?;
        let died = engine.run().await;
        engine.shutdown().await;
        Ok::<_, Box<dyn std::error::Error>>(died)
    });

    match result {
        Err(err) => {
            tracing::error!(error = %err, "fastadhunter failed to start");
            eprintln!("fastadhunter: {err}");
            ExitCode::FAILURE
        }
        Ok(Some(died)) => {
            tracing::error!(
                error = %died.last_error,
                "a DNS listener gave up after repeated socket errors — exiting so the \
                 process is restarted rather than serving nothing"
            );
            eprintln!("fastadhunter: DNS listener died: {}", died.last_error);
            ExitCode::FAILURE
        }
        Ok(None) => {
            tracing::info!("fastadhunter shutting down");
            ExitCode::SUCCESS
        }
    }
}

/// Everything running, kept together so shutdown can stop it all.
struct Engine {
    dns: fah_dns::Server,
    /// `None` in `dns` mode — the HTTP port is then never bound, not bound and
    /// left idle (CONTEXT.md §Operating Mode).
    http: Option<fah_http::Server>,
    https: Option<fah_http::TlsServer>,
    api: fah_api::ApiServer,
    tasks: Vec<Supervised>,
    stats_schedulers: Vec<Supervised>,
    stats: Arc<fah_stats::Stats>,
    metrics: Arc<fah_metrics::Metrics>,
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
                refresh_claim_lease(&config.dns.upstreams),
                events_tx.clone(),
            )
            .with_policies(Arc::clone(&policy_state)),
        );
        let mut dns = fah_dns::Server::bind(&config.dns).await?;
        tracing::info!(
            udp = %dns.udp_addr(),
            tcp = %dns.tcp_addr(),
            dot = ?dns.dot_addr(),
            "DNS listeners bound"
        );

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

        let mut https = if https_enabled(config.engine.mode) {
            let server = fah_http::TlsServer::bind(&config.https).await?;
            tracing::info!(addr = %server.local_addr(), "HTTPS SNI listener bound");
            Some(server)
        } else {
            None
        };

        // The proxy itself (p2-02). Built here, before `config` is moved into
        // the ConfigStore below, and only in a mode that serves HTTP — the
        // upstream connection pool should not exist in `dns` mode.
        let http_runtimes = config.runtime.http_runtimes;
        let http_proxy = if http.is_some() {
            let counters = Arc::new(fah_http::ProxyCounters::default());
            let make_proxy = http_proxy_factory(
                &config,
                upstreams.clone(),
                Arc::clone(&rules) as Arc<dyn fah_http::Ruleset>,
                Arc::clone(&policy_state),
                events_tx.clone(),
                Arc::clone(&counters),
            )?;
            Some((counters, make_proxy))
        } else {
            None
        };
        let (proxy_counters, make_proxy) = http_proxy.unzip();

        // ── Privilege drop (ADR-0004) ──
        // Port 53 is the only thing here that needs root, and it is now bound.
        // Everything below runs unprivileged: the API listens on 8443, and the
        // API key, TLS certificate and every later write to /config and /data
        // are created as the service user rather than root. The DNS listeners
        // are spawned *after* this point, so no query is ever answered by a
        // privileged process.
        //
        // The state subdirectory is named explicitly, not just `/data`: the
        // writers above (`rules.boot`, `stats.boot`) already ran as root and
        // may have created `/data/history/*` owned by root. When `/data` itself
        // is already the service user's — the seeded image, or any later boot —
        // `reown_if_needed` takes its top-level shortcut and never descends, so
        // that fresh root-owned subtree would stay unwritable after the drop.
        // Passing it as its own root reowns it on the next boot (a no-op once
        // already adopted).
        let history_dir = data_dir.join("history");
        privilege::drop_to_service_user(&[config_dir, data_dir, &history_dir])?;

        // ── API (L3) ──
        let (config, loaded) = {
            let dir = config_dir.to_path_buf();
            let path = config_path.to_path_buf();
            tokio::task::spawn_blocking(move || {
                let mut config = config;
                let loaded = fah_api::load_or_migrate(&dir, &mut config, &path);
                (config, loaded)
            })
            .await?
        };
        let interception_state = Arc::new(InterceptionState::new(loaded?.active));

        let (keys, generated) = fah_api::ApiKeyStore::load_or_create(config_dir)?;
        if let Some(key) = generated {
            // Printed exactly once, on first boot (SECURITY.md §API access).
            tracing::info!(api_key = %key, "generated API key — store it now; it is not shown again");
        }

        let (auth, generated_password) = {
            let config_dir = config_dir.to_path_buf();
            let data_dir = data_dir.to_path_buf();
            tokio::task::spawn_blocking(move || {
                #[cfg(feature = "test-harness")]
                {
                    tracing::warn!(
                        "built with the p5-04 measurement feature: login rate limiting is \
                         relaxed. This build must never be shipped"
                    );
                    fah_api::AuthState::load_or_create_with_limits(
                        &config_dir,
                        &data_dir,
                        fah_api::AuthState::relaxed_limits(),
                    )
                }
                #[cfg(not(feature = "test-harness"))]
                fah_api::AuthState::load_or_create(&config_dir, &data_dir)
            })
            .await??
        };
        if let Some(password) = generated_password {
            tracing::info!(
                dashboard_password = %password,
                "generated dashboard password — store it now; it is not shown again"
            );
        }
        let api_pair = if config.api.tls || config.dns.listen.dot_enabled {
            match fah_api::load_or_generate_tls(
                config_dir,
                &config.api.address,
                fah_api::probe_local_address(),
            ) {
                Ok(pair) => Some(pair),
                Err(error) if config.api.tls => return Err(error.into()),
                Err(error) => {
                    tracing::error!(
                        %error,
                        "the API certificate pair did not load; [dns.listen] dot_enabled = true \
                         needs it as the DoT fallback certificate — the DoT listener is closed \
                         until /config is repaired and the container restarted"
                    );
                    None
                }
            }
        } else {
            None
        };
        let dot_pair_loaded = api_pair.is_some();
        let tls = if config.api.tls {
            api_pair
        } else {
            tracing::warn!(
                "api.tls is disabled — the API key travels in plaintext, and dashboard \
                 session login is unavailable because the session cookie requires a \
                 Secure __Host- prefix; bearer-key authentication is unaffected and the \
                 other three /api/v1/auth routes stay usable. See SECURITY.md"
            );
            None
        };

        let certs = {
            let config_dir = config_dir.to_path_buf();
            match tokio::task::spawn_blocking(move || fah_api::CertStore::open(&config_dir)).await?
            {
                Ok(store) => Some(Arc::new(store)),
                Err(error) => {
                    tracing::error!(
                        %error,
                        "the certificate store did not open; /api/v1/certificates is \
                         unavailable until /config is repaired and the container restarted"
                    );
                    None
                }
            }
        };

        let interception_runtime = match (https.is_some(), certs.is_some()) {
            (false, _) => fah_api::InterceptionRuntime::NoListener,
            (true, true) => fah_api::InterceptionRuntime::Live,
            (true, false) => fah_api::InterceptionRuntime::StoreClosed,
        };

        let tls_proxy = if https.is_some() {
            let mut proxy = build_tls_proxy(&config, upstreams.clone())?
                .with_rules(Arc::clone(&rules) as Arc<dyn fah_http::Ruleset>)
                .with_policies(Arc::clone(&policy_state))
                .with_events(events_tx.clone());
            if let Some(interception) =
                interception(certs.as_ref(), Arc::clone(&interception_state))?
            {
                proxy = proxy.with_interception(interception);
            }
            Some(Arc::new(proxy))
        } else {
            None
        };

        let dot = if dot_pair_loaded {
            let listen = config.dns.listen.clone();
            let certs = certs.clone();
            tokio::task::spawn_blocking(move || dot_tls(&listen, certs.as_ref())).await?
        } else if config.dns.listen.dot_enabled {
            Err(
                "the API certificate pair did not load — the DoT listener is closed until \
                 /config is repaired and the container restarted"
                    .to_string(),
            )
        } else {
            Ok(None)
        };
        let dot_listener = match (&dot, dns.dot_addr()) {
            (Ok(Some(_)), Some(address)) => fah_api::DotListener::Listening { address },
            (Ok(Some(_)), None) => fah_api::DotListener::Closed {
                reason: "the DoT socket was never bound".to_string(),
            },
            (Ok(None), _) => fah_api::DotListener::Closed {
                reason: "[dns.listen] dot_enabled = false".to_string(),
            },
            (Err(reason), _) => fah_api::DotListener::Closed {
                reason: reason.clone(),
            },
        };
        let dot = dot.unwrap_or(None);
        let doh = config.dns.listen.doh_enabled.then(|| {
            Arc::new(adapters::DnsWireAdapter::new(Arc::clone(&pipeline)))
                as Arc<dyn fah_api::DnsWireSource>
        });
        if config.dns.listen.doh_enabled && !config.api.tls {
            tracing::warn!(
                "[dns.listen] doh_enabled = true, but [api] tls = false: /dns-query is not \
                 served — DoH is HTTPS-only and needs the API listener's TLS"
            );
        }

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
                    proxy_counters.clone(),
                    tls_proxy.as_ref().map(|proxy| proxy.counters()),
                )),
                cache: Arc::new(adapters::CacheAdapter::new(Arc::clone(&pipeline))),
                config: Arc::new(fah_api::ConfigStore::new(config, config_path.to_path_buf())),
                interception: Arc::new(fah_api::InterceptionStore::new(
                    Arc::clone(&interception_state),
                    config_dir.join(fah_api::DOCUMENT_FILE),
                    interception_runtime,
                )),
                keys: Arc::new(keys),
                auth: Arc::new(auth),
                certs,
                doh,
                dot: dot_listener,
            },
        )
        .await?;
        tracing::info!(url = %api.base_url(), "API listening");

        // Unprivileged from here — start answering (ADR-0004).
        dns.serve(Arc::clone(&pipeline), dot);
        let domains = NonZeroUsize::new(http_runtimes);
        if let (Some(http), Some(make_proxy)) = (http.as_mut(), make_proxy) {
            match domains {
                None => {
                    tracing::info!(
                        http_runtimes = 0,
                        "HTTP proxy serving on the shared runtime"
                    );
                    http.serve(Arc::new(make_proxy()));
                }
                Some(domains) => {
                    http.serve_domains(domains, HTTP_DRAIN_TIMEOUT, make_proxy)?;
                    tracing::info!(
                        http_runtimes = domains.get(),
                        "HTTP proxy serving on its own single-thread runtimes"
                    );
                }
            }
        }
        if let (Some(https), Some(proxy)) = (https.as_mut(), tls_proxy.as_ref()) {
            match (domains, http.as_ref()) {
                (Some(_), Some(http)) => {
                    https.serve_domains(Arc::clone(proxy), http)?;
                    tracing::info!("HTTPS proxy serving on the HTTP allocation domains");
                }
                _ => {
                    https.serve(Arc::clone(proxy));
                    tracing::info!("HTTPS proxy serving on the shared runtime");
                }
            }
        }

        // ── The edges between the siblings ──
        let stats_schedulers = vec![
            Supervised::new("stats snapshot scheduler", stats.spawn_snapshot_scheduler()),
            Supervised::new("stats history scheduler", stats.spawn_history_scheduler()),
        ];
        let mut tasks = vec![
            Supervised::new("rules scheduler", rules.spawn_scheduler()),
            Supervised::new(
                "event fan-out",
                spawn_event_fanout(
                    events_rx,
                    Arc::clone(&stats),
                    Arc::clone(&metrics),
                    api.events(),
                ),
            ),
            Supervised::new(
                "perf sampler",
                spawn_perf_sampler(
                    Arc::clone(&stats),
                    Arc::clone(&metrics),
                    Arc::clone(&pipeline),
                    Arc::clone(&rules),
                    http.as_ref().map(fah_http::Server::connections),
                    https.as_ref().map(fah_http::TlsServer::connections),
                    perf_sample_interval_seconds,
                ),
            ),
            Supervised::new(
                "telemetry poll",
                spawn_telemetry_poll(
                    Arc::clone(&metrics),
                    Arc::clone(&rules),
                    Arc::clone(&pipeline),
                    upstreams,
                    ProxyCounterSources {
                        http: proxy_counters,
                        https: tls_proxy.as_ref().map(|proxy| proxy.counters()),
                    },
                    dns.tcp_connections(),
                    dns.udp_inflight(),
                ),
            ),
            Supervised::new(
                "policy ticker",
                spawn_policy_ticker(policy_state, rules, Arc::clone(&stats)),
            ),
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
        tasks.extend(
            swr_workers
                .into_iter()
                .map(|worker| Supervised::new("swr worker", worker)),
        );

        // Scheduled cache sweep, spawned here for the same reason: the binary
        // owns every long-lived task's lifetime. `None` when
        // `[dns.cache] cleanup_interval_seconds = 0`.
        if let Some(cleanup) = pipeline.spawn_cache_cleanup() {
            tracing::info!("cache cleanup scheduler started");
            tasks.push(Supervised::new("cache cleanup", cleanup));
        }

        Ok(Self {
            dns,
            http,
            https,
            api,
            tasks,
            stats_schedulers,
            stats,
            metrics,
        })
    }

    async fn run(&mut self) -> Option<fah_dns::ListenerDied> {
        let shutdown = await_shutdown();
        tokio::pin!(shutdown);
        let mut supervision = tokio::time::interval(TELEMETRY_POLL);
        supervision.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                () = &mut shutdown => return None,
                died = self.dns.fatal() => return Some(died),
                _ = supervision.tick() => self.reap_dead_tasks().await,
            }
        }
    }

    async fn reap_dead_tasks(&mut self) {
        let mut deaths = supervisor::reap(&mut self.tasks).await;
        deaths.extend(supervisor::reap(&mut self.stats_schedulers).await);
        for death in deaths {
            self.metrics.record_task_death();
            tracing::error!(
                task = death.name,
                cause = %death.cause,
                "a supervised task died; the resolver keeps answering but that task's work has stopped"
            );
        }
    }

    async fn shutdown(&mut self) {
        if let Some(https) = &self.https {
            https.shutdown();
        }
        if let Some(http) = &mut self.http {
            http.shutdown();
        }
        self.dns.shutdown();
        self.api.shutdown();
        for task in &self.tasks {
            task.handle.abort();
        }
        let schedulers = std::mem::take(&mut self.stats_schedulers);
        for scheduler in &schedulers {
            scheduler.handle.abort();
        }
        let flush = async {
            for scheduler in schedulers {
                let _ = scheduler.handle.await;
            }
            self.stats.save_snapshot().await;
            self.stats.flush_history(std::time::SystemTime::now()).await;
        };
        if tokio::time::timeout(STATS_FLUSH_TIMEOUT, flush)
            .await
            .is_err()
        {
            tracing::warn!(
                timeout_seconds = STATS_FLUSH_TIMEOUT.as_secs(),
                "stats flush at shutdown timed out; up to one snapshot interval of aggregates lost"
            );
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

fn https_enabled(mode: fah_config::EngineMode) -> bool {
    match mode {
        fah_config::EngineMode::Dns | fah_config::EngineMode::DnsHttp => false,
        fah_config::EngineMode::DnsHttpHttps => true,
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

const HTTPS_ORIGIN_PORT: u16 = 443;

/// Idle upstream connections kept per origin. Bounded so the pool is a function
/// of configuration rather than of how many sites the LAN visits (hard rule 4);
/// a household reuses a handful of connections per site, and anything beyond
/// that is memory held against the 128 MB budget for no gain.
const MAX_IDLE_UPSTREAMS_PER_HOST: usize = 8;

const HTTP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

const STATS_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Assembles the HTTP proxy from config: the injected resolver port, and the
/// egress policy that decides where it may connect.
///
/// The allow-list is re-parsed here rather than trusted from `fah-config` —
/// that crate is L1 and cannot import `fah_common::egress`, so it can only
/// check the shape. This is the authoritative parse, and it fails startup
/// rather than degrading to a policy the operator did not write.
fn egress_exceptions(
    config: &fah_config::Config,
) -> Result<Vec<fah_common::egress::AllowedNet>, Box<dyn std::error::Error>> {
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
        tracing::info!(
            count = exceptions.len(),
            "egress allow-list active — private destinations permitted"
        );
    }
    Ok(exceptions)
}

fn dot_tls(
    listen: &fah_config::DnsListenConfig,
    certs: Option<&Arc<fah_api::CertStore>>,
) -> Result<Option<fah_dns::DotTls>, String> {
    if !listen.dot_enabled {
        return Ok(None);
    }
    let Some(store) = certs else {
        let reason = "[dns.listen] dot_enabled = true, but the certificate store did not open — \
                      the DoT listener is closed until /config is repaired and the container \
                      restarted"
            .to_string();
        tracing::error!("{reason}");
        return Err(reason);
    };
    let fallback = match store.api_certified_key() {
        Ok(key) => key,
        Err(error) => {
            let reason = format!(
                "the API certificate pair did not load ({error}) — the DoT listener is closed \
                 until /config is repaired and the container restarted"
            );
            tracing::error!("{reason}");
            return Err(reason);
        }
    };
    match fah_dns::DotTls::new(Arc::clone(store), fallback) {
        Ok(tls) => {
            match store.has_ca() {
                true => tracing::info!(
                    port = listen.dot_port,
                    "DoT serves a CA-minted certificate for the hostname each client sends, \
                     the API certificate when a hello carries no SNI"
                ),
                false => tracing::info!(
                    port = listen.dot_port,
                    "DoT serves the API certificate; generate or import a CA via \
                     /api/v1/certificates for Android Private DNS hostname mode"
                ),
            }
            Ok(Some(tls))
        }
        Err(error) => {
            let reason = format!(
                "the DoT TLS configuration did not build ({error}); the listener is closed"
            );
            tracing::error!("{reason}");
            Err(reason)
        }
    }
}

fn interception(
    certs: Option<&Arc<fah_api::CertStore>>,
    state: Arc<InterceptionState>,
) -> Result<Option<fah_http::Interception>, Box<dyn std::error::Error>> {
    let active = state.current();
    let clients = active.scope.client_count();
    let Some(store) = certs else {
        if clients > 0 {
            tracing::warn!(
                count = clients,
                "the certificate store did not open — listed clients are spliced, not \
                 intercepted, until /config is repaired and the container restarted"
            );
        }
        return Ok(None);
    };
    if !store.has_ca() && clients > 0 {
        tracing::warn!(
            count = clients,
            "clients are listed, but no CA is installed — their connections close until one \
             is generated or imported via /api/v1/certificates"
        );
    }
    let server = fah_http::server_config(Arc::clone(store))?;
    #[cfg(not(feature = "test-harness"))]
    let client = fah_http::client_config()?;
    #[cfg(feature = "test-harness")]
    let client = test_harness_upstream_client_config()?;
    tracing::info!(
        clients,
        exclusions = active.scope.exclusion_count(),
        "HTTPS interception machinery ready — each listed client must hold a static lease"
    );
    drop(active);
    Ok(Some(fah_http::Interception::new(
        server,
        client,
        Arc::clone(store),
        state,
    )))
}

#[cfg(feature = "test-harness")]
fn test_harness_upstream_client_config(
) -> Result<Arc<rustls::ClientConfig>, Box<dyn std::error::Error>> {
    const ROOT_ENV: &str = "FAH_TEST_UPSTREAM_ROOT";
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(path) = std::env::var_os(ROOT_ENV) {
        let der = std::fs::read(&path)
            .map_err(|err| format!("{ROOT_ENV}: reading {}: {err}", path.to_string_lossy()))?;
        roots.add(rustls::pki_types::CertificateDer::from(der))?;
        tracing::warn!(
            path = %path.to_string_lossy(),
            "built with the test-harness feature: an extra upstream trust anchor was loaded \
             from {ROOT_ENV}. This build must never be shipped"
        );
    }
    Ok(fah_http::client_config_with_roots(roots)?)
}

fn build_tls_proxy(
    config: &fah_config::Config,
    upstreams: fah_dns::UpstreamPool,
) -> Result<fah_http::TlsProxy, Box<dyn std::error::Error>> {
    Ok(fah_http::TlsProxy::new(
        Arc::new(adapters::UpstreamResolver::new(upstreams)),
        fah_common::egress::DestinationPolicy::new(HTTPS_ORIGIN_PORT, egress_exceptions(config)?),
        HTTPS_ORIGIN_PORT,
        Duration::from_millis(config.https.hello_timeout_ms),
        Duration::from_millis(config.https.idle_timeout_ms),
        config.https.sni.no_sni,
    )
    .with_ip_literal_hosts(config.egress.allow_ip_literal_hosts))
}

fn http_proxy_factory(
    config: &fah_config::Config,
    upstreams: fah_dns::UpstreamPool,
    rules: Arc<dyn fah_http::Ruleset>,
    policies: Arc<fah_rules::PolicyState>,
    events: tokio::sync::mpsc::Sender<fah_model::Event>,
    counters: Arc<fah_http::ProxyCounters>,
) -> Result<impl Fn() -> fah_http::Proxy + Send + Sync + 'static, Box<dyn std::error::Error>> {
    let exceptions = egress_exceptions(config)?;

    let resolver: Arc<dyn fah_common::resolve::HostResolver> =
        Arc::new(adapters::UpstreamResolver::new(upstreams));
    let header_timeout = Duration::from_millis(config.http.header_timeout_ms);
    let idle_timeout = Duration::from_millis(config.http.idle_timeout_ms);
    let allow_ip_literal_hosts = config.egress.allow_ip_literal_hosts;
    Ok(move || {
        fah_http::Proxy::new(
            Arc::clone(&resolver),
            fah_common::egress::DestinationPolicy::new(HTTP_ORIGIN_PORT, exceptions.clone()),
            HTTP_ORIGIN_PORT,
            header_timeout,
            idle_timeout,
            MAX_IDLE_UPSTREAMS_PER_HOST,
            allow_ip_literal_hosts,
        )
        .with_counters(Arc::clone(&counters))
        .with_rules(Arc::clone(&rules))
        .with_policies(Arc::clone(&policies))
        .with_events(events.clone())
    })
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
                fah_model::Event::Http(request)
                | fah_model::Event::HttpsSni(request)
                | fah_model::Event::Https(request) => metrics.record_http(request),
            }
            let publish = hub.has_query_subscribers();
            let for_hub = publish.then(|| event.clone());
            match event {
                fah_model::Event::Dns(query) => stats.record(*query),
                fah_model::Event::Http(request) | fah_model::Event::HttpsSni(request) => {
                    stats.record_http(*request)
                }
                fah_model::Event::Https(request) => stats.record_https(*request),
            }
            if let Some(event) = for_hub {
                // Resolved after `record` so a first-ever client already
                // carries whatever name the registry has.
                hub.publish_query(event, stats.client_name(client_ip));
            }
        }
    })
}

fn refusals_of(proxy: &fah_http::ProxyStats) -> fah_metrics::RefusalSnapshot {
    fah_metrics::RefusalSnapshot {
        claim: proxy.refused_claim,
        destination: proxy.refused_destination,
    }
}

struct ProxyCounterSources {
    http: Option<Arc<fah_http::ProxyCounters>>,
    https: Option<Arc<fah_http::ProxyCounters>>,
}

/// Refreshes the metrics that are read rather than pushed: the pipeline's
/// channel-drop counter, per-upstream health and the compiled ruleset's size.
///
/// The p2-07 memory breakdown is **not** here. It has exactly one consumer, the
/// perf sampler, so collecting it on this 10 s tick discarded 35 of every 36
/// passes at up to 4.86 ms each. `/telemetry` and `/debug/memory` collect their
/// own on demand and never read a stored one.
fn spawn_telemetry_poll(
    metrics: Arc<fah_metrics::Metrics>,
    rules: Arc<fah_rules::ListManager>,
    pipeline: Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    upstreams: fah_dns::UpstreamPool,
    proxies: ProxyCounterSources,
    dns_tcp: Arc<fah_dns::TcpConnectionGauge>,
    dns_udp: Arc<fah_dns::UdpInflightGauge>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TELEMETRY_POLL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;

            metrics.set_dropped_events(pipeline.dropped_events());
            metrics.set_dns_tcp_connections(dns_tcp.snapshot());
            metrics.set_dns_udp_inflight(dns_udp.snapshot());
            let mut refusals = fah_metrics::RefusalSnapshot::default();
            for counters in [proxies.http.as_ref(), proxies.https.as_ref()]
                .into_iter()
                .flatten()
            {
                let split = refusals_of(&counters.snapshot());
                refusals.claim += split.claim;
                refusals.destination += split.destination;
            }
            metrics.set_requests_refused(refusals);
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
            metrics.set_upstreams(lifetime_rtt(upstreams.status()));
            let fetches = rules.fetch_stats();
            metrics.set_lists(fah_model::ListFetchCounters {
                bodies: fetches.bodies,
                not_modified: fetches.not_modified,
                bytes_fetched: fetches.bytes_fetched,
            });

            // `len` and `duplicates_removed` are field reads. The ruleset's
            // *size* is deliberately not read here: `heap_bytes()` is a walk,
            // and this is a runtime worker. The perf sampler reads it on the
            // blocking pool, where its duration cannot reach query latency.
            let matcher = rules.matcher();
            metrics.set_ruleset(fah_metrics::RulesetSnapshot {
                rules: matcher.len(),
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

/// Where the process's memory is, gathered in **one pass** so
/// `Σ(components) + residual == rss` holds within the snapshot. Reading RSS at a
/// different instant from the components would push the skew into the residual,
/// which is precisely the signal this exists to keep clean.
///
/// This is also the only layer allowed to see all three sources: `fah-rules`,
/// `fah-dns` and `fah-stats` are L3 siblings that never import each other.
///
/// **On the blocking pool, not a runtime worker.** Every read is synchronous and
/// the pass measures 495 µs – 4.86 ms on the RB5009, a 10× spread tracking cache
/// occupancy. `spawn_blocking` makes that duration irrelevant to query latency
/// however far it drifts — the same treatment `ListManager::compile` gets, for
/// the same reason (PERFORMANCE.md golden rule 8).
///
/// Returns the cache read alongside the breakdown because the caller needs both
/// and the walk is the expensive half. Reading it twice would put a second
/// bounded walk on a runtime worker *and* let a row's `cache.bytes` disagree
/// with its own `memory.cache`.
/// Takes the matcher itself rather than the `ListManager` that owns it: sizing
/// a ruleset needs one `heap_bytes()` walk, not the lifecycle around it. The
/// caller loads the live matcher — an atomic refcount bump — and hands it over,
/// which also pins *which* ruleset the row describes when a refresh swaps one
/// in mid-pass.
async fn collect_memory(
    matcher: Arc<fah_rules::Matcher>,
    pipeline: &Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    stats: &Arc<fah_stats::Stats>,
) -> (fah_model::MemoryBreakdown, fah_dns::CacheStats) {
    let pipeline = Arc::clone(pipeline);
    let stats = Arc::clone(stats);
    let (memory, cache) = tokio::task::spawn_blocking(move || {
        // One read for the total and its parts, so the split cannot skew
        // against the total it partitions.
        let resident = fah_common::process::resident();
        let cache = pipeline.cache_stats();
        let memory = fah_model::MemoryBreakdown {
            components: fah_model::MemoryComponents {
                ruleset: matcher.heap_bytes() as u64,
                cache: cache.bytes,
                stats: stats.heap(),
            },
            // `None` where RSS cannot be read (a non-Linux dev box, or an
            // unreadable /proc/self/status), so the residual reports as absent
            // rather than as a fabricated RSS of zero.
            rss: resident.map(|resident| resident.total),
            rss_anon: resident.and_then(|resident| resident.anon),
            rss_file: resident.and_then(|resident| resident.file),
            // Same instant as the components above: `minor_page_faults` is read
            // as a rate against the query counters sampled in this pass, and
            // skew would land in that rate.
            process: process::stats(),
            allocator: allocator::stats(),
        };
        (memory, cache)
    })
    .await
    .expect("memory accounting task panicked");

    if memory.over_accounted() {
        // Impossible in reality: components cannot hold more than the process
        // resides. Means a `heap_bytes` double-counts, or counts something not
        // resident. Logged rather than silently floored at zero, because a
        // wrong instrument is worse than no instrument.
        tracing::warn!(
            accounted = memory.accounted(),
            rss = ?memory.rss,
            "memory accounting exceeds RSS — a component heap_bytes is over-reporting",
        );
    }
    (memory, cache)
}

/// Samples the live perf/system/cache figures on [`PERF_SAMPLE_INTERVAL`] and
/// hands each [`fah_model::PerfSample`] to `fah-stats` to persist. Reads only
/// snapshots — the memory breakdown, [`fah_metrics::Metrics::snapshot`], the
/// cache port — never the per-query path (hard rule 3). Keeps the previous
/// metrics snapshot so lifetime-cumulative counters become per-interval rates
/// and percentiles.
fn spawn_perf_sampler(
    stats: Arc<fah_stats::Stats>,
    metrics: Arc<fah_metrics::Metrics>,
    pipeline: Arc<fah_dns::Pipeline<fah_dns::UpstreamPool>>,
    rules: Arc<fah_rules::ListManager>,
    http_connections: Option<Arc<fah_http::ConnectionGauge>>,
    https_connections: Option<Arc<fah_http::ConnectionGauge>>,
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
            // Collected here, once per persisted row, because this is the only
            // thing that reads it (p2-07). One walk serves both the row's cache
            // block and the breakdown's cache component, so they cannot
            // disagree inside a single sample.
            //
            // The matcher is loaded per tick, not captured once: a refresh
            // swaps in a new ruleset and the row must size the one serving now.
            let (memory, cache) = collect_memory(rules.matcher(), &pipeline, &stats).await;
            // `PerfSample::rss_bytes` is a plain `u64`, so an unreadable RSS
            // persists as 0 — which the history reader already charts as "not
            // recorded" rather than as a real measurement.
            let rss = memory.rss.unwrap_or(0);
            let concurrent_connections = fah_model::ConcurrentConnections {
                http: http_connections
                    .as_ref()
                    .map_or(0, |gauge| gauge.take_peak()),
                https: https_connections
                    .as_ref()
                    .map_or(0, |gauge| gauge.take_peak()),
            };
            let sample = build_perf_sample(
                &current,
                prev.as_ref(),
                &cache,
                rss,
                &memory,
                interval_secs,
                concurrent_connections,
            );
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
    memory: &fah_model::MemoryBreakdown,
    interval_secs: f64,
    concurrent_connections: fah_model::ConcurrentConnections,
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
    let answers_delta = match prev {
        Some(prev) => fah_model::AnswerCounters {
            servfail_synthesized: current
                .answers_servfail_synthesized
                .saturating_sub(prev.answers_servfail_synthesized),
            servfail_relayed: current
                .answers_servfail_relayed
                .saturating_sub(prev.answers_servfail_relayed),
            refused_relayed: current
                .answers_refused_relayed
                .saturating_sub(prev.answers_refused_relayed),
        },
        None => fah_model::AnswerCounters::default(),
    };
    let qps = if interval_secs > 0.0 {
        queries_delta as f64 / interval_secs
    } else {
        0.0
    };

    fah_model::PerfSample {
        ts,
        rss_bytes,
        peak_rss: memory.process.map_or(0, |p| p.peak_rss),
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
        memory: memory.components,
        minor_page_faults: memory.process.map_or(0, |p| p.minor_page_faults),
        rss_anon_bytes: memory.rss_anon.unwrap_or(0),
        rss_file_bytes: memory.rss_file.unwrap_or(0),
        answers_delta,
        allocator_committed_bytes: memory.allocator.map_or(0, |a| a.current_commit),
        list_fetch: current.lists,
        concurrent_connections,
        latency: latency_summary(current, prev),
        // One type end to end (`fah_model::UpstreamSample`), so this is a clone
        // rather than a field-by-field remap into a structurally identical
        // struct — which is what the pool status, the registry and this row
        // used to each have their own of.
        upstreams: interval_rtt(
            &current.upstreams,
            prev.map(|prev| prev.upstreams.as_slice()),
        ),
    }
}

fn rtt_percentiles(rtt: &fah_model::UpstreamRtt) -> (f64, f64) {
    let quantile = |q| {
        fah_common::histogram::quantile(
            &fah_model::UPSTREAM_RTT_BUCKETS_SECONDS,
            &rtt.buckets,
            rtt.count,
            q,
        )
    };
    (quantile(0.5), quantile(0.99))
}

fn lifetime_rtt(mut upstreams: Vec<fah_model::UpstreamSample>) -> Vec<fah_model::UpstreamSample> {
    for upstream in &mut upstreams {
        (upstream.rtt.p50, upstream.rtt.p99) = rtt_percentiles(&upstream.rtt);
    }
    upstreams
}

fn interval_rtt(
    current: &[fah_model::UpstreamSample],
    prev: Option<&[fah_model::UpstreamSample]>,
) -> Vec<fah_model::UpstreamSample> {
    let mut rows = current.to_vec();
    for row in &mut rows {
        let previous = prev.and_then(|prev| prev.iter().find(|prev| prev.address == row.address));
        let interval = match previous {
            Some(previous) => {
                let mut buckets = [0u64; fah_model::UPSTREAM_RTT_BUCKETS_SECONDS.len()];
                for (slot, value) in
                    buckets
                        .iter_mut()
                        .zip(fah_common::histogram::saturating_delta(
                            &row.rtt.buckets,
                            &previous.rtt.buckets,
                        ))
                {
                    *slot = value;
                }
                fah_model::UpstreamRtt {
                    count: row.rtt.count.saturating_sub(previous.rtt.count),
                    buckets,
                    ..row.rtt
                }
            }
            None => row.rtt,
        };
        (row.rtt.p50, row.rtt.p99) = rtt_percentiles(&interval);
    }
    rows
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

fn refresh_claim_lease(upstreams: &fah_config::DnsUpstreamsConfig) -> Duration {
    (fah_dns::worst_case_walk(upstreams) * 2).max(Duration::from_secs(5))
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

    #[test]
    fn https_starts_only_in_the_mode_that_names_it() {
        assert!(!https_enabled(fah_config::EngineMode::Dns));
        assert!(!https_enabled(fah_config::EngineMode::DnsHttp));
        assert!(https_enabled(fah_config::EngineMode::DnsHttpHttps));
    }

    #[test]
    fn the_telemetry_poll_publishes_each_proxy_refusal_cause_on_its_own_field() {
        let counters = fah_http::ProxyCounters::default();
        for _ in 0..3 {
            counters
                .refused_claim
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        for _ in 0..11 {
            counters
                .refused_destination
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        let metrics = fah_metrics::Metrics::new();
        metrics.set_requests_refused(refusals_of(&counters.snapshot()));

        let http = metrics.engine_telemetry().counters.http;
        assert_eq!(
            (http.refused_claim, http.refused_destination),
            (3, 11),
            "the hop from ProxyCounters to /telemetry must neither swap the two \
             causes nor fold them back into one figure"
        );
    }

    fn upstreams(servers: usize, timeout_ms: u32) -> fah_config::DnsUpstreamsConfig {
        fah_config::DnsUpstreamsConfig {
            timeout_ms,
            servers: (0..servers)
                .map(|index| fah_config::UpstreamServerConfig {
                    address: format!("10.0.0.{index}"),
                    protocol: fah_config::UpstreamProtocol::Udp,
                    hostname: None,
                })
                .collect(),
            ..fah_config::DnsUpstreamsConfig::default()
        }
    }

    #[test]
    fn the_lease_doubles_the_worst_case_walk_above_a_five_second_floor() {
        assert_eq!(
            fah_dns::worst_case_walk(&upstreams(2, 800)),
            Duration::from_millis(4_800)
        );
        assert_eq!(
            refresh_claim_lease(&upstreams(2, 800)),
            Duration::from_millis(9_600)
        );
        assert_eq!(
            refresh_claim_lease(&upstreams(0, 800)),
            Duration::from_secs(5)
        );
        assert_eq!(
            refresh_claim_lease(&upstreams(8, 800)),
            Duration::from_millis(38_400)
        );
        assert_eq!(
            refresh_claim_lease(&upstreams(1, 100)),
            Duration::from_secs(5)
        );
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
            answers_servfail_synthesized: 0,
            answers_servfail_relayed: 0,
            answers_refused_relayed: 0,
            dropped_events: 0,
            requests_pass: 0,
            requests_allow: 0,
            requests_block: 0,
            response_bytes: 0,
            swr: fah_metrics::SwrSnapshot::default(),
            cleanup: fah_metrics::CleanupSnapshot::default(),
            lists: fah_model::ListFetchCounters::default(),
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

    fn breakdown() -> fah_model::MemoryBreakdown {
        fah_model::MemoryBreakdown {
            components: fah_model::MemoryComponents {
                ruleset: 100,
                cache: 10,
                stats: fah_model::StatsHeap {
                    aggregates: 5,
                    clients: 4,
                },
            },
            rss: Some(1000),
            rss_anon: Some(700),
            rss_file: Some(300),
            process: Some(fah_model::ProcessStats {
                minor_page_faults: 7,
                peak_rss: 3000,
                ..Default::default()
            }),
            allocator: Some(fah_model::AllocatorStats::default()),
        }
    }

    #[test]
    fn perf_sample_deltas_and_qps_across_two_snapshots() {
        let prev = snapshot(100, 5, 20);
        let current = snapshot(160, 6, 34); // +60 pass, +1 allow, +14 block = +75

        let sample = build_perf_sample(
            &current,
            Some(&prev),
            &empty_cache(),
            1000,
            &breakdown(),
            60.0,
            fah_model::ConcurrentConnections { http: 7, https: 2 },
        );
        assert_eq!(sample.queries_delta, 75);
        assert_eq!(sample.concurrent_connections.http, 7);
        assert_eq!(sample.concurrent_connections.https, 2);
        assert_eq!(sample.blocked_delta, 14);
        assert_eq!(sample.allowed_delta, 1);
        assert!((sample.qps - 75.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn perf_sample_first_reading_has_zero_deltas_and_qps() {
        let current = snapshot(160, 6, 34);
        let sample = build_perf_sample(
            &current,
            None,
            &empty_cache(),
            1000,
            &breakdown(),
            60.0,
            fah_model::ConcurrentConnections::default(),
        );
        assert_eq!(sample.queries_delta, 0);
        assert_eq!(sample.blocked_delta, 0);
        assert_eq!(sample.allowed_delta, 0);
        assert_eq!(sample.qps, 0.0);
    }

    #[test]
    fn perf_sample_carries_the_components_and_the_fault_counter_but_no_rss_copy() {
        let memory = breakdown();
        let sample = build_perf_sample(
            &snapshot(160, 6, 34),
            None,
            &empty_cache(),
            1000,
            &memory,
            60.0,
            fah_model::ConcurrentConnections::default(),
        );

        assert_eq!(sample.memory, memory.components);
        assert_eq!(sample.minor_page_faults, 7);
        // The row's RSS is `rss_bytes` alone, so the residual is derivable and
        // never stored twice.
        assert_eq!(sample.rss_bytes, 1000);
        assert_eq!(sample.memory.accounted(), 119);
        // The peak is the kernel's high-water mark, not this instant's RSS.
        assert_eq!(sample.peak_rss, 3000);
    }

    /// `getrusage` unavailable reads back 0 — which the field documents as "not
    /// recorded", never as a peak of zero.
    #[test]
    fn perf_sample_peak_is_zero_when_the_process_stats_are_absent() {
        let memory = fah_model::MemoryBreakdown {
            process: None,
            ..breakdown()
        };
        let sample = build_perf_sample(
            &snapshot(160, 6, 34),
            None,
            &empty_cache(),
            1000,
            &memory,
            60.0,
            fah_model::ConcurrentConnections::default(),
        );
        assert_eq!(sample.peak_rss, 0);
        assert_eq!(sample.minor_page_faults, 0);
    }

    fn responder(mutate: fn(&[u8]) -> Vec<u8>) -> SocketAddr {
        let server = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let target = server.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            if let Ok((len, from)) = server.recv_from(&mut buf) {
                let _ = server.send_to(&mutate(&buf[..len]), from);
            }
        });
        target
    }

    #[test]
    fn a_wildcard_bind_is_probed_on_loopback() {
        for address in ["0.0.0.0", "::"] {
            let listen = fah_common::listen::listen_addr(address, 5300, "dns.listen").unwrap();
            assert_eq!(
                probe_target(listen),
                SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
                "{address} must be probed on loopback"
            );
        }
    }

    #[test]
    fn a_concrete_bind_is_probed_at_its_own_address() {
        for address in ["192.168.88.2", "::1", "127.0.0.1"] {
            let listen = fah_common::listen::listen_addr(address, 5300, "dns.listen").unwrap();
            assert_eq!(
                probe_target(listen),
                listen,
                "{address} serves only itself, so only itself can be probed"
            );
        }
    }

    #[test]
    fn the_probe_rejects_a_reply_too_short_to_be_a_dns_message() {
        let err = probe_dns(responder(|_| vec![0u8; 4])).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("not a DNS message"), "got: {err}");
    }

    #[test]
    fn the_probe_rejects_a_reply_carrying_another_querys_id() {
        let err = probe_dns(responder(|request| {
            let mut reply = request.to_vec();
            reply[0] ^= 0xff;
            reply[1] ^= 0xff;
            reply
        }))
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("another query's id"), "got: {err}");
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

#[cfg(test)]
mod upstream_rtt_tests {
    use super::*;

    fn endpoint(address: &str, rtt: fah_model::UpstreamRtt) -> fah_model::UpstreamSample {
        fah_model::UpstreamSample {
            address: address.to_string(),
            protocol: fah_model::Protocol::Udp,
            attempts: 0,
            failures: 0,
            consecutive_failures: 0,
            tls_handshakes: 0,
            failure_runs: [0; 4],
            state: fah_model::UpstreamState::Healthy,
            penalty_round: 0,
            penalties: 0,
            penalized_seconds_total: 0,
            probes: 0,
            probe_successes: 0,
            family: Some(fah_model::AddressFamily::V4),
            rtt,
        }
    }

    fn rtt(count: u64, buckets: [u64; 11]) -> fah_model::UpstreamRtt {
        fah_model::UpstreamRtt {
            count,
            sum_seconds: 0.0,
            p50: 0.0,
            p99: 0.0,
            buckets,
        }
    }

    #[test]
    fn lifetime_percentiles_read_the_whole_histogram() {
        let rows = lifetime_rtt(vec![endpoint(
            "1.1.1.1",
            rtt(10, [5, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10]),
        )]);
        assert_eq!(rows[0].rtt.p50, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[0]);
        assert_eq!(rows[0].rtt.p99, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[1]);
        assert_eq!(rows[0].rtt.count, 10);
    }

    #[test]
    fn percentiles_describe_the_interval_not_the_process_lifetime() {
        let prev = vec![endpoint("1.1.1.1", rtt(100, [100; 11]))];
        let current = vec![endpoint(
            "1.1.1.1",
            rtt(110, [100, 100, 100, 100, 100, 100, 100, 100, 100, 110, 110]),
        )];
        let rows = interval_rtt(&current, Some(&prev));
        assert_eq!(rows[0].rtt.p50, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[9]);
        assert_eq!(rows[0].rtt.p99, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[9]);
        assert_eq!(rows[0].rtt.count, 110);
    }

    #[test]
    fn an_endpoint_idle_over_the_interval_reports_zero_not_a_bucket_bound() {
        let prev = vec![endpoint("1.1.1.1", rtt(10, [10; 11]))];
        let current = vec![endpoint("1.1.1.1", rtt(10, [10; 11]))];
        let rows = interval_rtt(&current, Some(&prev));
        assert_eq!(rows[0].rtt.p50, 0.0);
        assert_eq!(rows[0].rtt.p99, 0.0);
    }

    #[test]
    fn a_reordered_config_is_matched_by_address_rather_than_by_index() {
        let prev = vec![
            endpoint("1.1.1.1", rtt(10, [10; 11])),
            endpoint("9.9.9.9", rtt(0, [0; 11])),
        ];
        let current = vec![
            endpoint("9.9.9.9", rtt(0, [0; 11])),
            endpoint("1.1.1.1", rtt(10, [10; 11])),
        ];
        let rows = interval_rtt(&current, Some(&prev));
        assert_eq!(rows[0].rtt.p50, 0.0);
        assert_eq!(rows[1].rtt.p50, 0.0);
        assert_eq!(rows[1].rtt.p99, 0.0);
    }

    #[test]
    fn a_replaced_endpoint_reads_its_own_lifetime_not_a_strangers_delta() {
        let prev = vec![endpoint("8.8.8.8", rtt(50, [50; 11]))];
        let current = vec![endpoint("1.1.1.1", rtt(10, [10; 11]))];
        let rows = interval_rtt(&current, Some(&prev));
        assert_eq!(rows[0].rtt.p50, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[0]);
    }

    #[test]
    fn the_first_sample_after_boot_reads_the_whole_lifetime() {
        let current = vec![endpoint("1.1.1.1", rtt(10, [10; 11]))];
        let rows = interval_rtt(&current, None);
        assert_eq!(rows[0].rtt.p50, fah_model::UPSTREAM_RTT_BUCKETS_SECONDS[0]);
    }
}
