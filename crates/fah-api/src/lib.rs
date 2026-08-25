//! Axum REST and WebSocket API (ARCHITECTURE.md L3) — the whole API.md
//! surface, served over rustls with bearer-key auth (SECURITY.md).
//!
//! **Layering.** `fah-stats`, `fah-metrics` and `fah-dns` are L3 siblings, so
//! this crate never imports them. It declares what it needs as the
//! [`ports::StatsSource`] / [`ports::HistorySource`] /
//! [`ports::TelemetrySource`] / [`ports::CacheSource`]
//! traits and the binary implements them — ARCHITECTURE.md's "the binary wires
//! them together via channels and handles". Only `fah-rules` (L2) and
//! `fah-config`/`fah-model` (L1) are held directly.
//!
//! ```text
//! ApiServer::bind ─ routes::router ─ auth::require_api_key ─┬─ /health
//!                                                           └─ /api/v1/* ─ ports
//! ```

mod auth;
mod config_store;
mod error;
mod events;
mod keys;
mod ports;
mod routes;
mod server;
mod state;
mod telemetry;
mod timestamp;
mod tls;
mod web;
mod wire;

pub use config_store::{ConfigStore, ConfigStoreError, UpdateOutcome};
pub use error::ApiError;
pub use events::{Event, EventHub};
pub use keys::ApiKeyStore;
pub use ports::{
    BucketCount, CacheClean, CacheSource, CacheStats, ClientCount, ClientEntry, DomainCount,
    HistorySource, PolicyCount, QueryRecord, StatsOverview, StatsSource, TelemetrySource,
};
pub use server::ApiServer;
pub use state::AppStateBuilder;
pub use tls::{install_crypto_provider, load_or_generate as load_or_generate_tls, TlsError};
