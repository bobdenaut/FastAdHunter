use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsListenConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_enabled")]
    pub dot_enabled: bool,
    #[serde(default = "default_dot_port")]
    pub dot_port: u16,
    #[serde(default = "default_enabled")]
    pub doh_enabled: bool,
}

impl Default for DnsListenConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            dot_enabled: default_enabled(),
            dot_port: default_dot_port(),
            doh_enabled: default_enabled(),
        }
    }
}

fn default_address() -> String {
    "::".to_string()
}

fn default_port() -> u16 {
    53
}

fn default_enabled() -> bool {
    true
}

fn default_dot_port() -> u16 {
    853
}
