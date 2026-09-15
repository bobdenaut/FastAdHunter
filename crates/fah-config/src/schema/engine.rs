use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// `[engine]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EngineConfig {
    #[serde(default = "default_mode")]
    pub mode: EngineMode,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
        }
    }
}

fn default_mode() -> EngineMode {
    EngineMode::Dns
}

/// `[engine] mode` — the engine's filtering scope, fixed at container start.
///
/// Duplicated locally rather than reusing `fah_model::OperatingMode`: sibling
/// L1 crates never import each other (CLAUDE.md hard rule); the binary wires
/// this into the DNS engine's own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineMode {
    #[serde(rename = "dns")]
    Dns,
    #[serde(rename = "dns+http")]
    DnsHttp,
    #[serde(rename = "dns+http+https")]
    DnsHttpHttps,
}

impl EngineMode {
    pub fn serves_http(self) -> bool {
        match self {
            EngineMode::Dns => false,
            EngineMode::DnsHttp | EngineMode::DnsHttpHttps => true,
        }
    }

    pub fn serves_https(self) -> bool {
        match self {
            EngineMode::Dns | EngineMode::DnsHttp => false,
            EngineMode::DnsHttpHttps => true,
        }
    }
}

impl FromStr for EngineMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dns" => Ok(EngineMode::Dns),
            "dns+http" => Ok(EngineMode::DnsHttp),
            "dns+http+https" => Ok(EngineMode::DnsHttpHttps),
            _ => Err("one of: dns, dns+http, dns+http+https"),
        }
    }
}
