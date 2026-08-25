//! The FastAdHunter HTTP API — one client, one connection pool, one token,
//! shared by every worker that polls it.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::config::{Config, TimeoutConfig};
use crate::models::config::AppliedConfig;
use crate::models::history::{HistoryPerf, HistorySummary};
use crate::models::lan::ClientList;
use crate::models::telemetry::Telemetry;

/// The paths this monitor reads, as API.md documents them.
pub mod paths {
    pub const CONFIG: &str = "/api/v1/config";
    pub const TELEMETRY: &str = "/api/v1/telemetry";
    pub const HISTORY_SUMMARY: &str = "/api/v1/history/summary";
    pub const HISTORY_PERF: &str = "/api/v1/history/perf";
    pub const EVENTS: &str = "/api/v1/events";
    pub const CLIENTS: &str = "/api/v1/clients";
}

/// The `max_points` asked of `/history/perf`, meaning "do not decimate": the
/// server thins a series by dropping whole rows (API.md §History), and a
/// dropped row is a spike that never happened. Over-asking is safe — the
/// endpoint clamps to its own ceiling rather than rejecting — so this is a
/// floor on what arrives, never a number that has to track the server's.
const UNDECIMATED: usize = 5_000;

/// The wire budget must strictly exceed what the graph retains, or the client
/// keeps room for samples the server was never asked to send. Strictly, because
/// the half-open window can hold one more than the day it covers.
const _: () = assert!(UNDECIMATED > crate::config::DAY_OF_SAMPLES);

#[derive(Debug)]
pub enum Error {
    /// Connect, TLS or timeout — the appliance was not reached.
    Unreachable(String),
    /// Reached and answered, but not with success. `401` means the token is
    /// wrong, which is worth saying out loud rather than rendering as no data.
    Status(reqwest::StatusCode),
    /// Answered, but the body is not the shape this build expects.
    Decode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(detail) => write!(f, "unreachable: {detail}"),
            Self::Status(status) => match *status {
                reqwest::StatusCode::UNAUTHORIZED => write!(f, "unauthorized (check the token)"),
                other => write!(f, "HTTP {}", other.as_u16()),
            },
            Self::Decode(detail) => write!(f, "unexpected response: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone)]
pub struct ApiClient {
    /// Cloning shares the connection pool: `reqwest::Client` is a handle, so
    /// the four API workers reuse one TLS session rather than opening four.
    http: reqwest::Client,
    base: String,
    bearer: String,
    timeout: TimeoutConfig,
}

impl ApiClient {
    pub fn new(config: &Config, tls: &super::TlsPolicy) -> Result<Self, reqwest::Error> {
        Ok(Self {
            http: tls.apply(reqwest::Client::builder()).build()?,
            base: config.base_url(),
            bearer: format!("Bearer {}", config.token),
            timeout: config.timeout,
        })
    }

    pub async fn telemetry(&self) -> Result<Telemetry, Error> {
        self.get(paths::TELEMETRY, self.timeout.telemetry()).await
    }

    pub async fn config(&self) -> Result<AppliedConfig, Error> {
        self.get(paths::CONFIG, self.timeout.telemetry()).await
    }

    /// The endpoint's default window: a **rolling** last-24 h at hour
    /// resolution, which is not the same thing as since-midnight.
    pub async fn history_last_24h(&self) -> Result<HistorySummary, Error> {
        self.get(paths::HISTORY_SUMMARY, self.timeout.history())
            .await
    }

    /// One point per UTC day since `from` (RFC 3339). `resolution=day` is what
    /// keeps a week's worth to seven rows instead of 168.
    pub async fn history_since(&self, from: &str) -> Result<HistorySummary, Error> {
        let path = format!("{}?from={from}&resolution=day", paths::HISTORY_SUMMARY);
        self.get(&path, self.timeout.history()).await
    }

    pub async fn history_perf(&self) -> Result<HistoryPerf, Error> {
        self.get(&perf_query(), self.timeout.history()).await
    }

    /// Every observed client and the name it carries — the source the router
    /// lookup resolves *to*, so a device shows the label already curated here
    /// rather than a DHCP host-name.
    pub async fn clients(&self) -> Result<ClientList, Error> {
        self.get(paths::CLIENTS, self.timeout.telemetry()).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, timeout: Duration) -> Result<T, Error> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .header(reqwest::header::AUTHORIZATION, &self.bearer)
            .timeout(timeout)
            .send()
            .await
            .map_err(|err| Error::Unreachable(err.to_string()))?;

        // Checked before decoding: an error body is JSON too, and decoding it
        // as the success shape reports "unexpected response" for what is
        // really a wrong token.
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Status(status));
        }

        response
            .json()
            .await
            .map_err(|err| Error::Decode(err.to_string()))
    }
}

/// The RSS series request. `fields=` drops every key but the one charted — a
/// day of full samples is otherwise ~1 MB of JSON.
fn perf_query() -> String {
    format!(
        "{}?fields=rss_bytes&max_points={UNDECIMATED}",
        paths::HISTORY_PERF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The budget itself is checked at compile time beside the constant; this
    /// covers the request actually carrying it, alongside the field filter.
    #[test]
    fn the_perf_query_asks_for_a_full_day_undecimated() {
        let query = perf_query();
        assert!(query.starts_with("/api/v1/history/perf?"), "{query}");
        assert!(query.contains("fields=rss_bytes"), "{query}");
        assert!(
            query.contains(&format!("max_points={UNDECIMATED}")),
            "{query}"
        );
    }

    /// One `?`, however many parameters: the path constants carry none of their
    /// own, so appending a second query string cannot produce `??`.
    #[test]
    fn every_path_constant_is_a_bare_path() {
        for path in [
            paths::TELEMETRY,
            paths::HISTORY_SUMMARY,
            paths::HISTORY_PERF,
            paths::EVENTS,
        ] {
            assert!(!path.contains('?'), "{path}");
        }
        assert_eq!(perf_query().matches('?').count(), 1);
    }
}
