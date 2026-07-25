use serde::{Deserialize, Serialize};

/// `[http]` (CONFIGURATION.md). The section is inert unless `engine.mode`
/// includes `http` — there is no `enabled` flag, because two switches for one
/// decision is how a deployment ends up believing HTTP is on while nothing
/// listens (CONTEXT.md §Operating Mode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpConfig {
    pub listen: HttpListenConfig,
    /// Hard ceiling on concurrent proxied connections (CLAUDE.md hard rule 4:
    /// memory must not grow with traffic). Enforced from the scaffold onward —
    /// accepted connections beyond it wait for a permit rather than being
    /// dropped, so a burst queues instead of multiplying task memory.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// How long a connection may sit idle before it is closed.
    ///
    /// **No consumer until p2-02** — the scaffold closes every connection
    /// immediately, so nothing can be idle. Declared here so the section lands
    /// complete in CONFIGURATION.md, and deliberately classified `boot`: a
    /// `runtime` claim the engine cannot honour is the exact defect p1.5-07
    /// found across ~16 keys.
    #[serde(default = "default_idle_timeout_ms")]
    pub idle_timeout_ms: u64,
    /// Deadline for a client to finish sending its request headers — the
    /// slowloris bound. Also **no consumer until p2-02**.
    #[serde(default = "default_header_timeout_ms")]
    pub header_timeout_ms: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen: HttpListenConfig::default(),
            max_connections: default_max_connections(),
            idle_timeout_ms: default_idle_timeout_ms(),
            header_timeout_ms: default_header_timeout_ms(),
        }
    }
}

/// `[http.listen]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpListenConfig {
    #[serde(default = "default_address")]
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for HttpListenConfig {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
        }
    }
}

fn default_address() -> String {
    "::".to_string()
}

/// 8080, not 80. The router dst-nats 80 here (docs/deploy-rb5009.md), so the
/// container never needs a privileged port for HTTP — unlike DNS, which has no
/// equivalent escape and is why ADR-0004 exists at all.
fn default_port() -> u16 {
    8080
}

/// 1024 concurrent connections. A household LAN peaks in the low hundreds;
/// this leaves room without letting a misbehaving client multiply task memory
/// against the 128 MB budget (PERFORMANCE.md).
fn default_max_connections() -> usize {
    1024
}

fn default_idle_timeout_ms() -> u64 {
    60_000
}

fn default_header_timeout_ms() -> u64 {
    10_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The port must not default to 80: the container runs unprivileged after
    /// ADR-0004's drop, and the router redirects instead.
    #[test]
    fn defaults_are_unprivileged_and_dual_stack() {
        let config = HttpConfig::default();
        assert_eq!(config.listen.port, 8080);
        assert_eq!(config.listen.address, "::");
    }

    #[test]
    fn every_bound_has_a_default() {
        let config = HttpConfig::default();
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.idle_timeout_ms, 60_000);
        assert_eq!(config.header_timeout_ms, 10_000);
    }
}
