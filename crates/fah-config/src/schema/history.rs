use serde::{Deserialize, Serialize};

use crate::schema::default_true;

/// `[history]` (CONFIGURATION.md) — long-term observability persistence on
/// `/data/history`: hourly/daily aggregate rollups and the per-interval
/// perf/system/cache sample series a future dashboard charts from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HistoryConfig {
    /// Master switch. `false` stops both history writers and the perf sampler,
    /// mirroring `query_log.enabled` — see SECURITY.md (it narrows the
    /// retained-data window).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// How often the perf/system/cache series is sampled and persisted. Boot:
    /// the sampler's ticker is built once at startup, so a change takes effect
    /// on restart.
    #[serde(default = "default_sample_interval_seconds")]
    pub sample_interval_seconds: u32,
    /// Age cap on `/data/history` day-files. Runtime: a change applies live to
    /// the next prune via atomic swap (no restart, no reconstruction).
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            sample_interval_seconds: default_sample_interval_seconds(),
            retention_days: default_retention_days(),
        }
    }
}

fn default_sample_interval_seconds() -> u32 {
    60
}

/// 30 days by default: the shortest window the dashboard offers. The files are
/// kilobytes/day (rollups) and tens of MB over 90 days (perf), negligible on
/// the RB5009's 1 TB SSD — so the user can raise it to 60/90 freely.
fn default_retention_days() -> u32 {
    30
}
