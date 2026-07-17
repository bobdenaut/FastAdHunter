use serde::{Deserialize, Serialize};

/// `[dns.cache]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsCacheConfig {
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
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

fn default_max_ttl_seconds() -> u32 {
    86_400
}

fn default_negative_ttl_max_seconds() -> u32 {
    60
}

fn default_true() -> bool {
    true
}
