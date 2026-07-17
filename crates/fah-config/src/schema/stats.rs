use serde::{Deserialize, Serialize};

/// `[stats]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StatsConfig {
    #[serde(default = "default_snapshot_interval_seconds")]
    pub snapshot_interval_seconds: u32,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_seconds: default_snapshot_interval_seconds(),
        }
    }
}

fn default_snapshot_interval_seconds() -> u32 {
    300
}
