//! Thin binary wiring FastAdHunter's crates together (ARCHITECTURE.md L4).
//!
//! Boot order: parse CLI -> load config -> init logging -> run (or
//! healthcheck) -> shutdown. No siblings are wired yet (ARCHITECTURE.md
//! §Runtime Model) — Phase 0 only proves the binary starts, loads config, and
//! shuts down cleanly.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fah_config::{Config, LogFormat as ConfigLogFormat, LogLevel};
use fah_logging::LogFormat;
use tracing_subscriber::filter::LevelFilter;

const DEFAULT_CONFIG_PATH: &str = "/config/fastadhunter.toml";

const USAGE: &str = "\
fastadhunter — network-wide ad blocker (DNS filtering)

USAGE:
    fastadhunter [OPTIONS]

OPTIONS:
    --config <PATH>    Config file path (default: /config/fastadhunter.toml)
    --healthcheck      Load and validate config, then exit 0/1 (no side effects)
    --version          Print version and exit
    --help, -h         Print this help and exit";

#[derive(Debug)]
struct Args {
    config_path: PathBuf,
    healthcheck: bool,
    version: bool,
    help: bool,
}

fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<Args, String> {
    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
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
            "--healthcheck" => healthcheck = true,
            "--version" => version = true,
            "--help" | "-h" => help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        config_path,
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
        // Phase 0 healthcheck is process-level: config loads and validates, exit
        // 0/1 (ARCHITECTURE.md §Docker: distroless has no shell, so the container
        // `HEALTHCHECK` self-execs this binary). Read-only — a probe must not
        // create the config file whose absence would signal a problem.
        // TODO(phase1): switch to probing `GET /health` once fah-api exists.
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

    run(config, &args.config_path)
}

fn run(config: Config, config_path: &Path) -> ExitCode {
    let _logging = fah_logging::init(
        level_filter(config.log.level),
        log_format(config.log.format),
    );

    tracing::info!(
        config_path = %config_path.display(),
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

    runtime.block_on(await_shutdown());

    tracing::info!("fastadhunter shutting down");
    ExitCode::SUCCESS
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
