mod api;
mod dns;
mod engine;
mod log;
mod query_log;
mod rules;
mod stats;

pub use api::ApiConfig;
pub use dns::{
    BlockingMode, DnsBlockingConfig, DnsCacheConfig, DnsConfig, DnsListenConfig,
    DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy,
};
pub use engine::{EngineConfig, EngineMode};
pub use log::{LogConfig, LogFormat, LogLevel};
pub use query_log::QueryLogConfig;
pub use rules::{RuleListConfig, RulesConfig};
pub use stats::StatsConfig;

use serde::{Deserialize, Serialize};

/// The full typed configuration tree, mirroring CONFIGURATION.md's sections.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub engine: EngineConfig,
    pub dns: DnsConfig,
    pub rules: RulesConfig,
    pub query_log: QueryLogConfig,
    pub stats: StatsConfig,
    pub api: ApiConfig,
    pub log: LogConfig,
}
