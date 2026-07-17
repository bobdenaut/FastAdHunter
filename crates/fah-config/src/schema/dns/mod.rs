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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsConfig {
    pub listen: DnsListenConfig,
    pub blocking: DnsBlockingConfig,
    pub cache: DnsCacheConfig,
    pub upstreams: DnsUpstreamsConfig,
}
