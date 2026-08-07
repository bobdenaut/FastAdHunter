//! `config.toml` — where the appliance is, how often to ask it, and how long to
//! wait. Every key has a compiled-in default, so a minimal file works.
//!
//! Endpoint paths are not configurable: they are API.md's, held in
//! [`crate::client::api::paths`]. Unknown tables are ignored, not rejected.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

const CONFIG_FILE: &str = "config.toml";

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
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            telemetry_seconds: 5,
            history_seconds: 15,
            routeros_seconds: 2,
            reconnect_seconds: 2,
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
}

/// Bounds on what the display retains. Both are memory ceilings as much as
/// display choices — neither may grow with uptime.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Rows kept in the live feed. One screen is ~40; the rest is scrollback.
    pub feed_rows: usize,
    /// Points the RSS graph holds — 24 h at a 5-minute perf cadence.
    pub rss_points: usize,
    /// Redraw budget: the longest the screen may go without repainting when
    /// no key or mouse event arrives.
    pub redraw_millis: u64,
    /// RSS in MB at or above which the graph turns amber.
    pub rss_warn_mb: f64,
    /// RSS in MB at or above which it turns red.
    pub rss_alert_mb: f64,
    /// A feed query slower than this is boxed in the Time column.
    pub slow_query_ms: f64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            feed_rows: 200,
            rss_points: 288,
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
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn Error>> {
        Self::from_path(Path::new(CONFIG_FILE))
    }

    fn from_path(path: &Path) -> Result<Self, Box<dyn Error>> {
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
        assert_eq!(config.ui.limits().rss_points, 288, "defaulted");
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

    #[test]
    fn a_zero_interval_is_clamped_rather_than_spinning() {
        let poll = PollConfig {
            telemetry_seconds: 0,
            ..Default::default()
        };
        assert_eq!(poll.telemetry(), Duration::from_secs(1));
    }
}
