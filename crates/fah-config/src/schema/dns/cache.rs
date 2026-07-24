use serde::{Deserialize, Serialize};

use crate::schema::default_true;

/// `[dns.cache]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsCacheConfig {
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
    /// Byte ceiling on what the cached answers themselves hold, enforced by
    /// the same FIFO eviction as `max_entries` — whichever bound binds first
    /// evicts. Entry count alone cannot bound memory: the ~91h soak
    /// (docs/code-review/p1-11-soak.md) filled a bounded cache with
    /// large TXT/SOA/NXDOMAIN answers and reached ~230 MiB, 80% over the
    /// 128 MB budget, while real traffic sat at ~55 MiB.
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
    #[serde(default)]
    pub min_ttl_seconds: u32,
    #[serde(default = "default_max_ttl_seconds")]
    pub max_ttl_seconds: u32,
    #[serde(default = "default_negative_ttl_max_seconds")]
    pub negative_ttl_max_seconds: u32,
    #[serde(default = "default_true")]
    pub serve_stale: bool,
}

impl Default for DnsCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            max_bytes: default_max_bytes(),
            min_ttl_seconds: 0,
            max_ttl_seconds: default_max_ttl_seconds(),
            negative_ttl_max_seconds: default_negative_ttl_max_seconds(),
            serve_stale: default_true(),
        }
    }
}

fn default_max_entries() -> u32 {
    10_000
}

/// 64 MiB — half of PERFORMANCE.md's 128 MB steady-state budget, which leaves
/// room for the compiled ruleset (≤40 MB at 1M domains) and the process base
/// underneath the ceiling even when the cache is completely full of
/// adversarially large answers. Non-binding at the default `max_entries`
/// (10 000 real answers are a few MB); it starts mattering exactly where
/// CONFIGURATION.md suggests raising entries "to 100k+ if RAM allows".
fn default_max_bytes() -> u64 {
    64 * 1024 * 1024
}

fn default_max_ttl_seconds() -> u32 {
    86_400
}

fn default_negative_ttl_max_seconds() -> u32 {
    60
}
