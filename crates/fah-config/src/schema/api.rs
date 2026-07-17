use serde::{Deserialize, Serialize};

/// `[api]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ApiConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub tls: bool,
    #[serde(default = "default_true")]
    pub metrics_public: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            tls: default_true(),
            metrics_public: default_true(),
        }
    }
}

fn default_address() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8443
}

fn default_true() -> bool {
    true
}
