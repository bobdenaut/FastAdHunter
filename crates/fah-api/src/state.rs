//! Everything the route handlers reach through. Cloneable and cheap — every
//! field is an `Arc`, matching how `fah-dns`'s `Pipeline` is shared across
//! listener tasks.

use std::sync::Arc;
use std::time::Instant;

use fah_rules::{ListManager, PolicyState};

use crate::config_store::ConfigStore;
use crate::events::EventHub;
use crate::keys::ApiKeyStore;
use crate::ports::{CacheSource, HistorySource, StatsSource, TelemetrySource};

pub struct AppState {
    pub rules: Arc<ListManager>,
    /// The client → policy map the pipelines read (p2-06). The API republishes
    /// it after an edit so a reassignment is live before the response returns.
    pub policies: Arc<PolicyState>,
    pub stats: Arc<dyn StatsSource>,
    pub history: Arc<dyn HistorySource>,
    pub telemetry: Arc<dyn TelemetrySource>,
    pub cache: Arc<dyn CacheSource>,
    pub config: Arc<ConfigStore>,
    pub keys: Arc<ApiKeyStore>,
    pub events: EventHub,
    pub started_at: Instant,
    /// Serializes `POST`/`PATCH`/`DELETE /api/v1/lists`. Each of those reads
    /// the current list set, writes the resulting set back to
    /// `fastadhunter.toml`, then mutates the `ListManager` — three steps that
    /// must not interleave, or two concurrent writers persist list sets that
    /// each omit the other's change. Admin-plane only; nothing on the DNS hot
    /// path ever takes it.
    pub list_mutations: tokio::sync::Mutex<()>,
    /// Same read-modify-write reason, for `/policies` and client assignments.
    pub policy_mutations: tokio::sync::Mutex<()>,
}

impl AppState {
    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

/// The handles the binary supplies; `started_at` and the event hub are the
/// server's own.
pub struct AppStateBuilder {
    pub rules: Arc<ListManager>,
    pub policies: Arc<PolicyState>,
    pub stats: Arc<dyn StatsSource>,
    pub history: Arc<dyn HistorySource>,
    pub telemetry: Arc<dyn TelemetrySource>,
    pub cache: Arc<dyn CacheSource>,
    pub config: Arc<ConfigStore>,
    pub keys: Arc<ApiKeyStore>,
}

impl AppStateBuilder {
    pub fn build(self, events: EventHub) -> Arc<AppState> {
        Arc::new(AppState {
            rules: self.rules,
            policies: self.policies,
            stats: self.stats,
            history: self.history,
            telemetry: self.telemetry,
            cache: self.cache,
            config: self.config,
            keys: self.keys,
            events,
            started_at: Instant::now(),
            list_mutations: tokio::sync::Mutex::new(()),
            policy_mutations: tokio::sync::Mutex::new(()),
        })
    }
}
