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
    #[serde(default, skip_serializing_if = "InterceptionConfig::is_absent")]
    pub interception: InterceptionConfig,
}

impl Default for HttpsConfig {
    fn default() -> Self {
        Self {
            listen: HttpsListenConfig::default(),
            max_connections: default_max_connections(),
            hello_timeout_ms: default_hello_timeout_ms(),
            idle_timeout_ms: default_idle_timeout_ms(),
            sni: SniConfig::default(),
            interception: InterceptionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct InterceptionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
}

impl InterceptionConfig {
    pub fn is_absent(&self) -> bool {
        self.clients.is_none() && self.exclude_domains.is_none()
    }

    pub fn take(&mut self) -> Self {
        std::mem::take(self)
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
    fn the_interception_keys_are_absent_by_default() {
        let config = HttpsConfig::default();
        assert!(config.interception.is_absent());
        let parsed: HttpsConfig = toml::from_str("[listen]\nport = 8444\n").unwrap();
        assert!(parsed.interception.is_absent());
    }

    #[test]
    fn the_interception_section_parses_clients_and_exclusions() {
        let config: HttpsConfig = toml::from_str(
            "[interception]\nclients = [\"192.168.88.10\", \"192.168.88.0/24\"]\n\
             exclude_domains = [\"bank.example\"]\n",
        )
        .unwrap();
        assert_eq!(
            config.interception.clients.as_deref(),
            Some(&["192.168.88.10".to_string(), "192.168.88.0/24".to_string()][..])
        );
        assert_eq!(
            config.interception.exclude_domains.as_deref(),
            Some(&["bank.example".to_string()][..])
        );
        assert!(toml::from_str::<HttpsConfig>("[interception]\nenabled = true\n").is_err());
    }

    #[test]
    fn interception_keys_still_parse_as_present_when_empty() {
        let config: HttpsConfig = toml::from_str("[interception]\nclients = []\n").unwrap();
        assert_eq!(config.interception.clients, Some(Vec::new()));
        assert_eq!(config.interception.exclude_domains, None);
        assert!(!config.interception.is_absent());
    }

    #[test]
    fn an_absent_interception_section_serializes_to_nothing() {
        let text = toml::to_string_pretty(&HttpsConfig::default()).unwrap();
        assert!(!text.contains("interception"), "{text}");
    }

    #[test]
    fn a_present_interception_section_round_trips() {
        let mut config = HttpsConfig::default();
        config.interception.clients = Some(vec!["192.168.88.10".to_string()]);
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("[interception]"), "{text}");
        assert!(!text.contains("exclude_domains"), "{text}");
        assert_eq!(toml::from_str::<HttpsConfig>(&text).unwrap(), config);
    }

    #[test]
    fn take_clears_the_keys_and_hands_them_over() {
        let mut config = HttpsConfig::default();
        config.interception.clients = Some(vec!["10.0.0.1".to_string()]);
        config.interception.exclude_domains = Some(vec!["bank.example".to_string()]);
        let taken = config.interception.take();
        assert_eq!(taken.clients, Some(vec!["10.0.0.1".to_string()]));
        assert_eq!(
            taken.exclude_domains,
            Some(vec!["bank.example".to_string()])
        );
        assert!(config.interception.is_absent());
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
