//! The router's own figures for the footer, and — when `auto_name` is on — the
//! only source that can say who an IPv6 client is.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use crate::client::{ApiClient, RouterOsClient};
use crate::config::PollConfig;
use crate::models::lan::ClientEntry;
use crate::models::routeros::{Container, DhcpLease, Neighbor};
use crate::state::{LinkStatus, SharedState};

pub async fn run(
    client: RouterOsClient,
    api: ApiClient,
    auto_name: bool,
    poll: PollConfig,
    state: SharedState,
) {
    let mut ticker = tokio::time::interval(poll.routeros());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        // Concurrent: two independent reads of the same device, and the tick
        // budget is for both together.
        let (resource, container) = tokio::join!(client.system_resource(), client.container());

        state.update(|app| {
            let router = &mut app.router;
            router.link = match (&resource, &container) {
                (Err(error), _) | (_, Err(error)) => LinkStatus::Down(error.clone()),
                _ => LinkStatus::Online,
            };
            // Each half is applied only when it succeeded, so one failing read
            // does not blank the figures the other one just refreshed.
            if let Ok(resource) = resource {
                router.free_memory = resource.free_memory;
                router.total_memory = resource.total_memory;
                router.cpu_load = resource.cpu_load;
                router.cpu_frequency = resource.cpu_frequency;
                router.uptime = resource.uptime;
            }
            if let Ok(container) = container {
                // `memory_current` is `Copy`, so it reads through the borrow
                // and the label consumes what is about to be dropped.
                router.container_memory = container.as_ref().and_then(|c| c.memory_current);
                router.container_status = container.and_then(Container::status_label);
            }
        });

        if !auto_name {
            continue;
        }
        // A failure here leaves the previous map in place: stale labels beat no
        // labels, and the footer's own link status already reports the outage.
        let (neighbors, leases, clients) =
            tokio::join!(client.ipv6_neighbors(), client.dhcp_leases(), api.clients());
        if let (Ok(neighbors), Ok(leases), Ok(clients)) = (neighbors, leases, clients) {
            let resolved = resolve(&neighbors, &leases, &clients.items);
            state.update(|app| app.lan_names.replace(resolved));
        }
    }
}

/// Joins the router's two tables with the appliance's own names: a client
/// address is labelled by whatever device owns the MAC that answered for it.
///
/// Addresses the appliance already names are skipped — it wins, and the map
/// exists only to cover what it cannot see.
fn resolve(
    neighbors: &[Neighbor],
    leases: &[DhcpLease],
    clients: &[ClientEntry],
) -> HashMap<IpAddr, Arc<str>> {
    let named: HashMap<IpAddr, &str> = clients
        .iter()
        .filter_map(|entry| Some((entry.ip, entry.name.as_deref()?)))
        .collect();

    let mut by_mac: HashMap<String, Arc<str>> = HashMap::new();
    let mut resolved: HashMap<IpAddr, Arc<str>> = HashMap::new();
    for lease in leases {
        let Some(mac) = lease.mac_address.as_deref() else {
            continue;
        };
        let address = lease
            .address
            .as_deref()
            .and_then(|address| address.parse::<IpAddr>().ok());
        let label = address
            .and_then(|address| named.get(&address).copied())
            .or(lease.comment.as_deref())
            .or(lease.host_name.as_deref())
            .filter(|label| !label.trim().is_empty());
        let Some(label) = label else { continue };
        let label: Arc<str> = Arc::from(label);
        // The lease is itself an answer for its own IPv4 address, so a device
        // the appliance has not named yet is labelled without any ND entry.
        if let Some(address) = address.filter(|address| !named.contains_key(address)) {
            resolved.insert(address, label.clone());
        }
        by_mac.insert(mac.to_ascii_uppercase(), label);
    }

    // ND is what extends the same labels to IPv6, where no lease exists.
    for neighbor in neighbors {
        let Some(address) = neighbor
            .address
            .as_deref()
            .and_then(|address| address.parse::<IpAddr>().ok())
        else {
            continue;
        };
        if named.contains_key(&address) {
            continue;
        }
        let Some(mac) = neighbor.mac_address.as_deref() else {
            continue;
        };
        if let Some(label) = by_mac.get(&mac.to_ascii_uppercase()) {
            resolved.insert(address, label.clone());
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neighbor(address: &str, mac: &str) -> Neighbor {
        Neighbor {
            address: Some(address.to_string()),
            mac_address: Some(mac.to_string()),
        }
    }

    fn lease(address: &str, mac: &str, comment: Option<&str>, host: Option<&str>) -> DhcpLease {
        DhcpLease {
            address: Some(address.to_string()),
            mac_address: Some(mac.to_string()),
            comment: comment.map(str::to_string),
            host_name: host.map(str::to_string),
        }
    }

    fn client(ip: &str, name: Option<&str>) -> ClientEntry {
        ClientEntry {
            ip: ip.parse().unwrap(),
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn a_v6_address_takes_the_name_its_owner_carries_on_v4() {
        let resolved = resolve(
            &[neighbor("fd6c::5742", "78:ED:BC:44:AE:1D")],
            &[lease(
                "192.168.10.11",
                "78:ED:BC:44:AE:1D",
                None,
                Some("OnePlus-15"),
            )],
            &[client("192.168.10.11", Some("Liviu's OnePlus 15"))],
        );
        assert_eq!(
            resolved
                .get(&"fd6c::5742".parse().unwrap())
                .map(Arc::as_ref),
            Some("Liviu's OnePlus 15"),
            "the curated name wins over the DHCP host-name"
        );
    }

    /// The appliance is the authority; the router only fills its gaps.
    #[test]
    fn an_address_the_appliance_already_names_is_left_out() {
        let resolved = resolve(
            &[neighbor("fd6c::f286", "F0:86:20:8E:7C:06")],
            &[lease("192.168.10.16", "F0:86:20:8E:7C:06", None, None)],
            &[client("fd6c::f286", Some("LG WebOS TV"))],
        );
        assert!(resolved.is_empty());
    }

    #[test]
    fn an_unnamed_lease_falls_back_to_the_comment_then_the_host_name() {
        let resolved = resolve(
            &[
                neighbor("fd6c::a", "AA:AA:AA:AA:AA:AA"),
                neighbor("fd6c::b", "BB:BB:BB:BB:BB:BB"),
            ],
            &[
                lease(
                    "192.168.10.20",
                    "AA:AA:AA:AA:AA:AA",
                    Some("TV hol"),
                    Some("host-a"),
                ),
                lease("192.168.10.21", "BB:BB:BB:BB:BB:BB", None, Some("host-b")),
            ],
            &[],
        );
        assert_eq!(
            resolved.get(&"fd6c::a".parse().unwrap()).map(Arc::as_ref),
            Some("TV hol")
        );
        assert_eq!(
            resolved.get(&"fd6c::b".parse().unwrap()).map(Arc::as_ref),
            Some("host-b")
        );
    }

    /// RouterOS reports MACs uppercase in both tables today; matching on the
    /// raw strings would break silently the day one of them changes.
    #[test]
    fn the_mac_join_ignores_case() {
        let resolved = resolve(
            &[neighbor("fd6c::c", "c8:7f:54:65:3f:df")],
            &[lease(
                "192.168.10.10",
                "C8:7F:54:65:3F:DF",
                Some("Asus"),
                None,
            )],
            &[],
        );
        assert_eq!(
            resolved.get(&"fd6c::c".parse().unwrap()).map(Arc::as_ref),
            Some("Asus")
        );
    }

    /// The feature is not IPv6-only: a lease answers for its own address, so a
    /// device the appliance has not named yet is labelled without any ND entry.
    #[test]
    fn a_lease_labels_its_own_v4_address_with_no_neighbor_involved() {
        let resolved = resolve(
            &[],
            &[lease(
                "192.168.10.33",
                "AA:BB:CC:DD:EE:FF",
                None,
                Some("OnePlus-15"),
            )],
            &[],
        );
        assert_eq!(
            resolved
                .get(&"192.168.10.33".parse().unwrap())
                .map(Arc::as_ref),
            Some("OnePlus-15")
        );
    }

    #[test]
    fn a_neighbor_with_no_matching_lease_is_dropped_rather_than_labelled_blank() {
        let resolved = resolve(
            &[neighbor("fd6c::d", "DD:DD:DD:DD:DD:DD")],
            &[lease(
                "192.168.10.30",
                "EE:EE:EE:EE:EE:EE",
                Some("  "),
                None,
            )],
            &[],
        );
        assert!(resolved.is_empty());
    }
}
