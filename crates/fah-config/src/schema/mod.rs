mod api;
mod dns;
mod egress;
mod engine;
mod history;
mod http;
mod https;
mod log;
mod policy;
mod rules;
mod runtime;
mod stats;

pub use api::ApiConfig;
pub use dns::{
    BlockingMode, DnsBlockingConfig, DnsCacheConfig, DnsConfig, DnsListenConfig,
    DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy,
};
pub use egress::EgressConfig;
pub use engine::{EngineConfig, EngineMode};
pub use history::HistoryConfig;
pub use http::{HttpConfig, HttpListenConfig};
pub use https::{HttpsConfig, HttpsListenConfig, InterceptionConfig, NoSni, SniConfig};
pub use log::{LogConfig, LogFormat, LogLevel};
pub use policy::{parse_days, parse_time_of_day, AssignmentConfig, PolicyConfig, ScheduleConfig};
pub use rules::{RuleListConfig, RulesConfig};
pub use runtime::RuntimeConfig;
pub use stats::StatsConfig;

use serde::{Deserialize, Serialize};

/// Shared serde `default` for boolean fields that default to `true`. serde's
/// `#[serde(default = "…")]` needs a function, and `bool::default()` is `false`.
pub(crate) fn default_true() -> bool {
    true
}

/// The full typed configuration tree, mirroring CONFIGURATION.md's sections.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub engine: EngineConfig,
    pub runtime: RuntimeConfig,
    pub dns: DnsConfig,
    /// Inert unless `engine.mode` includes `http` (Phase 2).
    pub http: HttpConfig,
    #[serde(default)]
    pub https: HttpsConfig,
    /// Where the proxies may connect. Shared by HTTP and (Phase 3) HTTPS, so
    /// it is a section of its own rather than a key under `[http]`.
    pub egress: EgressConfig,
    pub rules: RulesConfig,
    /// `[schedule]` — the timezone every [`Self::policies`] window is read in.
    pub schedule: ScheduleConfig,
    /// `[[policies]]` (Phase 2). Empty is the zero-config case: every client
    /// gets the default policy, which is every enabled list, and the compiled
    /// ruleset carries no per-policy masks at all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<PolicyConfig>,
    pub stats: StatsConfig,
    pub history: HistoryConfig,
    pub api: ApiConfig,
    pub log: LogConfig,
}
