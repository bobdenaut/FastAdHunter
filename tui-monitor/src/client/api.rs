//! The FastAdHunter HTTP API — one client, one connection pool, one token,
//! shared by every worker that polls it.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::config::{Config, TimeoutConfig};
use crate::models::history::{HistoryPerf, HistorySummary};
use crate::models::telemetry::Telemetry;

/// The paths this monitor reads, as API.md documents them.
pub mod paths {
    pub const TELEMETRY: &str = "/api/v1/telemetry";
    pub const HISTORY_SUMMARY: &str = "/api/v1/history/summary";
    /// `?fields=` drops every key but the one charted; a day of full samples
    /// is otherwise ~1 MB of JSON.
    pub const HISTORY_PERF: &str = "/api/v1/history/perf?fields=rss_bytes";
    pub const EVENTS: &str = "/api/v1/events";
}

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

    /// The default window — the last 24 h at hour resolution.
    pub async fn history_today(&self) -> Result<HistorySummary, Error> {
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
        self.get(paths::HISTORY_PERF, self.timeout.history()).await
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
