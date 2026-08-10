//! `config.toml` — where the appliance is, how often to ask it, and how long to
//! wait. Every key has a compiled-in default, so a minimal file works.
//!
//! Endpoint paths are not configurable: they are API.md's, held in
//! [`crate::client::api::paths`]. Unknown tables are ignored, not rejected.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::BoxError;

const CONFIG_FILE: &str = "config.toml";

pub const USAGE: &str = "usage: fah-tui-monitor [--config <path>]";

/// What the command line asked for. The config path is the program's only
/// dependency on the working directory, so making it an argument is what lets
/// the binary be started from anywhere.
pub enum Startup {
    Run(PathBuf),
    Help,
}

impl Startup {
    /// Unknown arguments are rejected rather than ignored: a typo must not
    /// silently fall back to a different config than the one intended.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Self, BoxError> {
        let mut args = args.into_iter();
        let mut path = None;

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--help" | "-h") => return Ok(Self::Help),
                Some("--config") => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("--config needs a path\n{USAGE}"))?;
                    path = Some(PathBuf::from(value));
                }
                Some(other) if other.starts_with("--config=") => {
                    path = Some(PathBuf::from(&other["--config=".len()..]));
                }
                _ => {
                    let arg = arg.to_string_lossy();
                    return Err(format!("unexpected argument {arg:?}\n{USAGE}").into());
                }
            }
        }
        Ok(Self::Run(
            path.unwrap_or_else(|| PathBuf::from(CONFIG_FILE)),
        ))
    }
}

/// Perf samples a day holds at the appliance's 60 s cadence. The half-open
/// window the server serves can straddle the sampler's phase and hold one more,
/// so anything sizing itself against a day must clear this *strictly*.
pub const DAY_OF_SAMPLES: usize = 1_440;

/// Environment variable consulted for the API token, so a checked-in
/// `config.toml` need not carry one.
const TOKEN_ENV: &str = "FAH_TOKEN";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Overridden by `$FAH_TOKEN` when that is set and non-empty.
    #[serde(default)]
    pub token: String,
    /// Accept the appliance's self-signed certificate. Default `true`: the
    /// container serves a cert generated at first boot that no store trusts.
    #[serde(default = "yes")]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub poll: PollConfig,
    #[serde(default)]
    pub timeout: TimeoutConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub routeros: RouterOsConfig,
}

/// How often each worker asks.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct PollConfig {
    /// `GET /api/v1/telemetry`, which drives the header and most of the stats
    /// panel. One request, so 15–30 s is affordable if 60 s feels stale.
    pub telemetry_seconds: u64,
    /// `GET /api/v1/history/summary`, for the Today and 7-day panels.
    pub history_seconds: u64,
    /// `GET /api/v1/history/perf`, the header's RSS graph. Its own cadence:
    /// the appliance persists a sample every 60 s, and re-reading the series
    /// is the only way the graph advances — nothing is appended locally.
    pub rss_history_seconds: u64,
    pub routeros_seconds: u64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            telemetry_seconds: 60,
            history_seconds: 60,
            rss_history_seconds: 300,
            routeros_seconds: 60,
        }
    }
}

impl PollConfig {
    pub fn telemetry(&self) -> Duration {
        seconds(self.telemetry_seconds)
    }

    pub fn history(&self) -> Duration {
        seconds(self.history_seconds)
    }

    pub fn rss_history(&self) -> Duration {
        seconds(self.rss_history_seconds)
    }

    pub fn routeros(&self) -> Duration {
        seconds(self.routeros_seconds)
    }
}

/// Budgets rather than cadences: how long a request may take before it counts
/// as unreachable. A history scan walks files on the appliance's flash and is
/// legitimately slower than a counter read.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    pub telemetry_seconds: u64,
    pub history_seconds: u64,
    pub routeros_seconds: u64,
    /// Backoff between events-socket reconnection attempts.
    pub reconnect_seconds: u64,
    /// Silence on the events socket that counts as a dead connection. The
    /// server pushes `stats` every ~2 s, so this is ~15 missed pushes — long
    /// enough never to fire on a healthy link, short enough that a half-open
    /// TCP does not freeze the feed until someone notices.
    pub events_idle_seconds: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            telemetry_seconds: 5,
            history_seconds: 15,
            routeros_seconds: 2,
            reconnect_seconds: 2,
            events_idle_seconds: 30,
        }
    }
}

impl TimeoutConfig {
    pub fn telemetry(&self) -> Duration {
        seconds(self.telemetry_seconds)
    }

    pub fn history(&self) -> Duration {
        seconds(self.history_seconds)
    }

    pub fn routeros(&self) -> Duration {
        seconds(self.routeros_seconds)
    }

    pub fn reconnect(&self) -> Duration {
        seconds(self.reconnect_seconds)
    }

    pub fn events_idle(&self) -> Duration {
        seconds(self.events_idle_seconds)
    }
}

/// Bounds on what the display retains. Both are memory ceilings as much as
/// display choices — neither may grow with uptime.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Rows kept in the live feed. One screen is ~40; the rest is scrollback.
    pub feed_rows: usize,
    /// Samples the RSS graph holds — [`DAY_OF_SAMPLES`] by default. A memory
    /// bound, not a display choice: the series arrives undecimated and the
    /// chart reduces it to the terminal's width by peak when it draws.
    pub rss_points: usize,
    /// Redraw budget: the longest the screen may go without repainting when
    /// no key or mouse event arrives.
    pub redraw_millis: u64,
    /// RSS in MiB at or above which the graph turns amber. The `_mb` suffix
    /// stays for config compatibility; the unit it compares against is MiB.
    pub rss_warn_mb: f64,
    /// RSS in MiB at or above which it turns red.
    pub rss_alert_mb: f64,
    /// A feed query slower than this is boxed in the Time column.
    pub slow_query_ms: f64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            feed_rows: 200,
            rss_points: DAY_OF_SAMPLES,
            redraw_millis: 250,
            rss_warn_mb: 60.0,
            rss_alert_mb: 100.0,
            slow_query_ms: 50.0,
        }
    }
}

impl UiConfig {
    pub fn redraw(&self) -> Duration {
        // Milliseconds here, so the floor is a frame rather than a second.
        Duration::from_millis(self.redraw_millis.max(16))
    }

    /// Neither ring may be zero-length: the feed would drop every row it was
    /// handed, and the graph would have nothing to draw.
    pub fn limits(&self) -> Limits {
        Limits {
            feed_rows: self.feed_rows.max(1),
            rss_points: self.rss_points.max(1),
        }
    }

    /// Amber and red cut-offs, ordered even if the file has them backwards.
    pub fn rss_thresholds(&self) -> RssThresholds {
        RssThresholds {
            warn_mb: self.rss_warn_mb.min(self.rss_alert_mb),
            alert_mb: self.rss_warn_mb.max(self.rss_alert_mb),
        }
    }
}

/// Where the RSS graph changes colour.
#[derive(Debug, Clone, Copy)]
pub struct RssThresholds {
    pub warn_mb: f64,
    pub alert_mb: f64,
}

impl Default for RssThresholds {
    fn default() -> Self {
        UiConfig::default().rss_thresholds()
    }
}

/// The two ring bounds, as [`crate::state::AppState`] holds them.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub feed_rows: usize,
    pub rss_points: usize,
}

impl Default for Limits {
    fn default() -> Self {
        UiConfig::default().limits()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RouterOsConfig {
    /// Absent disables the footer's router figures entirely; nothing else
    /// depends on them.
    pub base_url: Option<String>,
    pub user: String,
    /// Falls back to `$MP`.
    pub password: Option<String>,
    /// Name of the container whose `memory-current` the footer reports.
    pub container: String,
    /// Resolve client addresses the appliance has no name for by asking the
    /// router who owns them. Costs two extra GETs per router tick and is the
    /// only way to label an IPv6 client: the appliance sees dst-natted traffic,
    /// so the source MAC reaching it is the router's, never the client's.
    pub auto_name: bool,
}

/// Hand-written rather than derived: `#[serde(default = "…")]` on a field only
/// fires when the table is present, so an absent `[routeros]` would otherwise
/// take empty strings for the user and container names.
impl Default for RouterOsConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            user: "monitor".to_string(),
            password: None,
            container: "fastadhunter".to_string(),
            auto_name: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, BoxError> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let mut config: Self = toml::from_str(&text)?;

        if let Some(token) = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty()) {
            config.token = token;
        }
        if config.token.is_empty() {
            return Err(
                format!("no API token: set `token` in {CONFIG_FILE} or ${TOKEN_ENV}").into(),
            );
        }
        Ok(config)
    }

    /// `https://host:port` — the origin every API request is built on.
    pub fn base_url(&self) -> String {
        format!("https://{}:{}", self.host, self.port)
    }

    /// The same origin as a websocket scheme.
    pub fn websocket_url(&self, path: &str) -> String {
        format!("wss://{}:{}{path}", self.host, self.port)
    }
}

impl RouterOsConfig {
    /// The configured password, or `$MP`.
    pub fn resolved_password(&self) -> String {
        self.password
            .clone()
            .or_else(|| std::env::var("MP").ok())
            .unwrap_or_default()
    }
}

fn seconds(value: u64) -> Duration {
    // A zero interval is a busy loop against the appliance, not a valid ask.
    Duration::from_secs(value.max(1))
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_file_takes_every_default() {
        let config: Config = toml::from_str(
            r#"
            host = "172.17.0.2"
            port = 8443
            token = "deadbeef"
            "#,
        )
        .unwrap();

        assert_eq!(config.base_url(), "https://172.17.0.2:8443");
        assert!(config.accept_invalid_certs);
        assert_eq!(config.poll.telemetry(), Duration::from_secs(60));
        assert_eq!(config.poll.history(), Duration::from_secs(60));
        assert_eq!(config.poll.routeros(), Duration::from_secs(60));
        assert_eq!(config.poll.rss_history(), Duration::from_secs(300));
        assert_eq!(config.timeout.telemetry(), Duration::from_secs(5));
        assert_eq!(config.ui.limits().feed_rows, 200);
        assert!(config.routeros.base_url.is_none());
    }

    /// An absent `[routeros]` table must still yield the compiled-in user and
    /// container names, not empty strings.
    #[test]
    fn an_absent_routeros_table_keeps_its_named_defaults() {
        let config: Config = toml::from_str("host = \"h\"\nport = 1\ntoken = \"t\"\n").unwrap();

        assert_eq!(config.routeros.user, "monitor");
        assert_eq!(config.routeros.container, "fastadhunter");
    }

    /// Every timer is a key, and one key may be overridden without restating
    /// the rest of its table.
    #[test]
    fn each_timer_can_be_set_on_its_own() {
        let config: Config = toml::from_str(
            r#"
            host = "h"
            port = 1
            token = "t"
            [poll]
            telemetry_seconds = 15
            [timeout]
            history_seconds = 30
            [ui]
            feed_rows = 500
            "#,
        )
        .unwrap();

        assert_eq!(config.poll.telemetry(), Duration::from_secs(15));
        assert_eq!(config.poll.routeros(), Duration::from_secs(60), "defaulted");
        assert_eq!(config.timeout.history(), Duration::from_secs(30));
        assert_eq!(
            config.timeout.reconnect(),
            Duration::from_secs(2),
            "defaulted"
        );
        assert_eq!(config.ui.limits().feed_rows, 500);
        assert_eq!(config.ui.limits().rss_points, 1440, "defaulted");
    }

    /// A zero ring would silently discard everything handed to it.
    #[test]
    fn zero_sized_rings_are_clamped_to_something_drawable() {
        let ui = UiConfig {
            feed_rows: 0,
            rss_points: 0,
            redraw_millis: 0,
            ..Default::default()
        };
        assert_eq!(ui.limits().feed_rows, 1);
        assert_eq!(ui.limits().rss_points, 1);
        assert_eq!(ui.redraw(), Duration::from_millis(16));
    }

    /// Thresholds are read in whichever order the file gives them, so a
    /// transposed pair widens the green band instead of inverting the graph.
    #[test]
    fn rss_thresholds_come_back_ordered_however_they_were_written() {
        let ui = UiConfig {
            rss_warn_mb: 100.0,
            rss_alert_mb: 60.0,
            ..Default::default()
        };
        let thresholds = ui.rss_thresholds();

        assert_eq!(thresholds.warn_mb, 60.0);
        assert_eq!(thresholds.alert_mb, 100.0);
    }

    /// A table this build does not read must be ignored, not rejected at
    /// startup — an owner's file may carry keys from any release.
    #[test]
    fn an_unknown_table_does_not_fail_the_load() {
        let config: Config = toml::from_str(
            r#"
            host = "h"
            port = 1
            token = "t"
            [endpoints]
            cache = "/api/v1/cache"
            metrics = "/metrics"
            "#,
        )
        .unwrap();
        assert_eq!(config.port, 1);
    }

    /// The file shipped beside the binary is the one the owner actually runs,
    /// and a key renamed in this module leaves it parsing into silent defaults
    /// — or failing at start-up, where the only reader is a terminal that has
    /// already been cleared.
    #[test]
    fn the_shipped_config_file_parses_against_this_build() {
        let config: Config = toml::from_str(include_str!("../config.toml")).unwrap();

        assert_eq!(config.timeout.events_idle(), Duration::from_secs(30));
        assert_eq!(config.ui.limits().rss_points, DAY_OF_SAMPLES);
        assert!(config.routeros.base_url.is_some());
    }

    fn startup(args: &[&str]) -> Result<Startup, BoxError> {
        Startup::from_args(args.iter().map(OsString::from))
    }

    /// The default has to stay the working directory, or every existing way of
    /// starting this binary breaks at once.
    #[test]
    fn the_config_path_defaults_to_the_working_directory() {
        let Ok(Startup::Run(path)) = startup(&[]) else {
            panic!("no arguments must be valid");
        };
        assert_eq!(path, Path::new("config.toml"));
    }

    #[test]
    fn the_config_path_can_be_given_in_either_spelling() {
        for args in [
            vec!["--config", "E:\\FastAdHunter\\tui-monitor\\config.toml"],
            vec!["--config=E:\\FastAdHunter\\tui-monitor\\config.toml"],
        ] {
            let Ok(Startup::Run(path)) = startup(&args) else {
                panic!("{args:?} must parse");
            };
            assert_eq!(
                path,
                Path::new("E:\\FastAdHunter\\tui-monitor\\config.toml")
            );
        }
    }

    /// Ignoring a bad argument would start the monitor against a different
    /// config than the one the caller named — silently, and on a screen that
    /// is about to be cleared.
    #[test]
    fn a_bad_argument_is_rejected_with_the_usage_line() {
        for args in [vec!["--config"], vec!["--cofnig", "x"], vec!["config.toml"]] {
            let error = startup(&args).err().expect("must be rejected").to_string();
            assert!(error.contains(USAGE), "{args:?}: {error}");
        }
    }

    #[test]
    fn help_is_a_request_rather_than_an_error() {
        assert!(matches!(startup(&["--help"]), Ok(Startup::Help)));
        assert!(matches!(startup(&["-h"]), Ok(Startup::Help)));
    }

    #[test]
    fn a_zero_interval_is_clamped_rather_than_spinning() {
        let poll = PollConfig {
            telemetry_seconds: 0,
            ..Default::default()
        };
        assert_eq!(poll.telemetry(), Duration::from_secs(1));
    }
}
