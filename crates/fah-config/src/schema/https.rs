use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpsConfig {
    pub listen: HttpsListenConfig,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_hello_timeout_ms")]
    pub hello_timeout_ms: u64,
    #[serde(default = "default_idle_timeout_ms")]
    pub idle_timeout_ms: u64,
    pub sni: SniConfig,
}

impl Default for HttpsConfig {
    fn default() -> Self {
        Self {
            listen: HttpsListenConfig::default(),
            max_connections: default_max_connections(),
            hello_timeout_ms: default_hello_timeout_ms(),
            idle_timeout_ms: default_idle_timeout_ms(),
            sni: SniConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpsListenConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for HttpsListenConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SniConfig {
    pub no_sni: NoSni,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoSni {
    #[default]
    Pass,
    Block,
}

fn default_address() -> String {
    "::".to_string()
}

fn default_port() -> u16 {
    8444
}

fn default_max_connections() -> usize {
    1024
}

fn default_hello_timeout_ms() -> u64 {
    10_000
}

fn default_idle_timeout_ms() -> u64 {
    60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_unprivileged_dual_stack_and_clear_of_the_api_port() {
        let config = HttpsConfig::default();
        assert_eq!(config.listen.port, 8444);
        assert_ne!(config.listen.port, crate::ApiConfig::default().port);
        assert_eq!(config.listen.address, "::");
    }

    #[test]
    fn every_bound_has_a_default() {
        let config = HttpsConfig::default();
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.hello_timeout_ms, 10_000);
        assert_eq!(config.idle_timeout_ms, 60_000);
        assert_eq!(config.sni.no_sni, NoSni::Pass);
    }

    #[test]
    fn no_sni_spelling_is_lowercase_on_the_wire() {
        let config: HttpsConfig = toml::from_str("[sni]\nno_sni = \"block\"\n").unwrap();
        assert_eq!(config.sni.no_sni, NoSni::Block);
        assert_eq!(
            toml::to_string(&SniConfig {
                no_sni: NoSni::Block
            })
            .unwrap()
            .trim(),
            "no_sni = \"block\""
        );
    }
}
