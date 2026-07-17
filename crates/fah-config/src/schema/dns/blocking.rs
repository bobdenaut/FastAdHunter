use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// `[dns.blocking]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsBlockingConfig {
    #[serde(default = "default_mode")]
    pub mode: BlockingMode,
    #[serde(default = "default_ttl_seconds")]
    pub ttl_seconds: u32,
}

impl Default for DnsBlockingConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            ttl_seconds: default_ttl_seconds(),
        }
    }
}

fn default_mode() -> BlockingMode {
    BlockingMode::NullIp
}

fn default_ttl_seconds() -> u32 {
    10
}

/// `[dns.blocking] mode`. Only `null_ip` is implemented today; `nxdomain`,
/// `refused` and `custom` are documented as future values (CONFIGURATION.md)
/// and deliberately absent here so an unimplemented mode fails loudly at
/// config-load time rather than silently falling back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockingMode {
    #[serde(rename = "null_ip")]
    NullIp,
}

impl FromStr for BlockingMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "null_ip" => Ok(BlockingMode::NullIp),
            _ => Err("null_ip"),
        }
    }
}
