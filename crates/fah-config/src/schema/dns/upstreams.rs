use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// `[dns.upstreams]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsUpstreamsConfig {
    #[serde(default = "default_strategy")]
    pub strategy: UpstreamStrategy,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
    #[serde(default = "default_penalty_failures")]
    pub penalty_failures: u32,
    #[serde(default = "default_servers")]
    pub servers: Vec<UpstreamServerConfig>,
}

impl Default for DnsUpstreamsConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            timeout_ms: default_timeout_ms(),
            penalty_failures: default_penalty_failures(),
            servers: default_servers(),
        }
    }
}

fn default_strategy() -> UpstreamStrategy {
    UpstreamStrategy::Fallback
}

fn default_timeout_ms() -> u32 {
    // Per-upstream attempt timeout. Kept well below a typical client's own
    // timeout (1–5 s) so that when the primary drops a packet, the fallback
    // attempt still answers inside the client's budget instead of arriving too
    // late to help — on-device measurement showed 2000 ms burned the whole
    // budget before failover even started. Still comfortably above a
    // legitimately slow recursive lookup (cold cache, distant TLD, DNSSEC), so
    // a healthy-but-slow answer is not abandoned prematurely.
    800
}

fn default_penalty_failures() -> u32 {
    2
}

fn default_servers() -> Vec<UpstreamServerConfig> {
    vec![
        UpstreamServerConfig {
            address: "1.1.1.1".to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        },
        UpstreamServerConfig {
            address: "9.9.9.9".to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        },
    ]
}

/// `[dns.upstreams] strategy`. Only `fallback` (ordered parallel fallback) is
/// implemented today; more strategies are documented as future work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpstreamStrategy {
    #[serde(rename = "fallback")]
    Fallback,
    #[serde(rename = "adaptive")]
    Adaptive,
}

impl FromStr for UpstreamStrategy {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fallback" => Ok(UpstreamStrategy::Fallback),
            "adaptive" => Ok(UpstreamStrategy::Adaptive),
            _ => Err("one of: fallback, adaptive"),
        }
    }
}

/// One `[[dns.upstreams.servers]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamServerConfig {
    pub address: String,
    #[serde(default)]
    pub protocol: UpstreamProtocol,
    #[serde(default)]
    pub hostname: Option<String>,
}

/// `[[dns.upstreams.servers]] protocol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UpstreamProtocol {
    #[default]
    #[serde(rename = "udp")]
    Udp,
    #[serde(rename = "dot")]
    Dot,
    #[serde(rename = "doh")]
    Doh,
}

impl FromStr for UpstreamProtocol {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "udp" => Ok(UpstreamProtocol::Udp),
            "dot" => Ok(UpstreamProtocol::Dot),
            "doh" => Ok(UpstreamProtocol::Doh),
            _ => Err("one of: udp, dot, doh"),
        }
    }
}
