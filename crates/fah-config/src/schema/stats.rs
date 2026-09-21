use serde::{Deserialize, Serialize};

/// `[stats]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StatsConfig {
    #[serde(default = "default_snapshot_interval_seconds")]
    pub snapshot_interval_seconds: u32,
    #[serde(default = "default_client_idle_expiry_days")]
    pub client_idle_expiry_days: u32,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_seconds: default_snapshot_interval_seconds(),
            client_idle_expiry_days: default_client_idle_expiry_days(),
        }
    }
}

fn default_snapshot_interval_seconds() -> u32 {
    300
}

fn default_client_idle_expiry_days() -> u32 {
    7
}
