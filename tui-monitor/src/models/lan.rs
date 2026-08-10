//! Addresses the appliance has no name for, resolved to a device label.
//!
//! Deliberately knows nothing about where the labels came from: a worker fills
//! it, the UI reads it. RouterOS is today's only provider, and none of its
//! vocabulary reaches this type.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use serde::Deserialize;

/// `GET /api/v1/clients` — only the two fields a label needs.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientList {
    pub items: Vec<ClientEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientEntry {
    pub ip: IpAddr,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LanNames {
    by_address: HashMap<IpAddr, Arc<str>>,
}

impl LanNames {
    /// Replaces the whole map. Providers rebuild rather than merge, so an
    /// address the source stopped reporting disappears instead of lingering.
    pub fn replace(&mut self, entries: HashMap<IpAddr, Arc<str>>) {
        self.by_address = entries;
    }

    pub fn get(&self, address: &IpAddr) -> Option<&str> {
        self.by_address.get(address).map(Arc::as_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<IpAddr, Arc<str>> {
        pairs
            .iter()
            .map(|(ip, name)| (ip.parse().unwrap(), Arc::from(*name)))
            .collect()
    }

    #[test]
    fn a_replaced_map_drops_addresses_the_provider_stopped_reporting() {
        let mut names = LanNames::default();
        names.replace(map(&[("fd6c::1", "laptop"), ("fd6c::2", "phone")]));
        assert_eq!(names.get(&"fd6c::2".parse().unwrap()), Some("phone"));

        names.replace(map(&[("fd6c::1", "laptop")]));
        assert_eq!(names.get(&"fd6c::1".parse().unwrap()), Some("laptop"));
        assert_eq!(names.get(&"fd6c::2".parse().unwrap()), None);
    }
}
