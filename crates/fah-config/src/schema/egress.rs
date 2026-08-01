use serde::{Deserialize, Serialize};

/// `[egress]` (CONFIGURATION.md) — where the proxies may connect.
///
/// Deliberately **not** nested under `[http]`. The policy is shared: Phase 3's
/// HTTPS path derives its destination from SNI, an equally attacker-controlled
/// claim, and judges it with the same rules. A key under `[http]` would either
/// be read by the HTTPS engine (confusing) or duplicated (worse).
///
/// The derived `Default` is the safe one and that is deliberate: an empty
/// allow-list refuses every private destination, and IP-literal hosts are off.
/// `the_default_allow_list_is_empty` guards it, so a future convenience default
/// has to argue with a failing test rather than slip in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EgressConfig {
    /// Private or local destinations the proxies may nonetheless reach, as IP
    /// addresses or CIDR blocks (`"192.168.10.50"`, `"192.168.10.0/24"`).
    ///
    /// **Empty by default, and that is the safe value.** The proxy's
    /// destination comes from a header the client writes, so with no allow-list
    /// every private, loopback and link-local address is refused — which is what
    /// stops a LAN device from using the proxy to reach the router or our own
    /// API. Add an entry only for an internal HTTP service you deliberately
    /// want filtered.
    #[serde(default)]
    pub allow_destinations: Vec<String>,

    /// Whether a client may name a bare IP as its destination.
    ///
    /// A browser resolving a name never produces one, so an IP-literal `Host`
    /// is a client addressing an address directly — the shape of a probe, not
    /// of web traffic. Refused by default; the allow-list above is the
    /// supported way to reach a specific internal service, because it is
    /// checked against the *resolved* address and this is not.
    #[serde(default)]
    pub allow_ip_literal_hosts: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default-deny is a security property, not a preference — assert it so a
    /// future convenience default has to argue with a failing test.
    #[test]
    fn the_default_allow_list_is_empty() {
        assert!(EgressConfig::default().allow_destinations.is_empty());
    }
}
