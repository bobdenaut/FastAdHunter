use serde::{Deserialize, Serialize};

use crate::schema::default_true;

/// `[query_log]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct QueryLogConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ring_entries")]
    pub ring_entries: u32,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_retention_max_mb")]
    pub retention_max_mb: u32,
    #[serde(default = "default_flush_interval_seconds")]
    pub flush_interval_seconds: u32,
}

impl Default for QueryLogConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            ring_entries: default_ring_entries(),
            retention_days: default_retention_days(),
            retention_max_mb: default_retention_max_mb(),
            flush_interval_seconds: default_flush_interval_seconds(),
        }
    }
}

fn default_ring_entries() -> u32 {
    10_000
}

fn default_retention_days() -> u32 {
    7
}

fn default_retention_max_mb() -> u32 {
    500
}

fn default_flush_interval_seconds() -> u32 {
    5
}
