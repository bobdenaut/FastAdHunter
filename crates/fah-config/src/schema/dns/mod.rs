mod blocking;
mod cache;
mod listen;
mod upstreams;

pub use blocking::{BlockingMode, DnsBlockingConfig};
pub use cache::DnsCacheConfig;
pub use listen::DnsListenConfig;
pub use upstreams::{DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy};

use serde::{Deserialize, Serialize};

/// `[dns.*]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsConfig {
    #[serde(default = "default_tcp_max_connections")]
    pub tcp_max_connections: usize,
    pub listen: DnsListenConfig,
    pub blocking: DnsBlockingConfig,
    pub cache: DnsCacheConfig,
    pub upstreams: DnsUpstreamsConfig,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            tcp_max_connections: default_tcp_max_connections(),
            listen: DnsListenConfig::default(),
            blocking: DnsBlockingConfig::default(),
            cache: DnsCacheConfig::default(),
            upstreams: DnsUpstreamsConfig::default(),
        }
    }
}

fn default_tcp_max_connections() -> usize {
    1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_max_connections_defaults_to_the_http_ceiling() {
        assert_eq!(DnsConfig::default().tcp_max_connections, 1024);
        let parsed: DnsConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.tcp_max_connections, 1024);
    }
}
