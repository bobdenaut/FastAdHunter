//! Client registry (CONTEXT.md/ARCHITECTURE.md: source IP -> first/last seen,
//! per-client counters, optional name). Capped at [`DEFAULT_CAPACITY`]
//! entries — a flood of spoofed/scanning source IPs must not grow memory
//! forever (hard rule 4); once full, the least-recently-seen *unnamed*
//! client is evicted first, so a spoof flood churns through its own
//! garbage entries instead of wiping the user's named devices.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::bucket::HourlyBuckets;

const DEFAULT_CAPACITY: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClientRecord {
    name: Option<String>,
    first_seen: SystemTime,
    last_seen: SystemTime,
    buckets: HourlyBuckets,
}

/// One client's view, for the API's client list/lookup (API.md `GET
/// /api/v1/clients`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientView {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub first_seen: SystemTime,
    pub last_seen: SystemTime,
    pub queries_24h: u64,
    pub blocked_24h: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClientRegistry {
    clients: HashMap<IpAddr, ClientRecord>,
    /// Not serialized: a snapshot written by a build with a different cap
    /// must not pin the old value forever — the current constant always
    /// wins, and `record`'s eviction loop converges an over-cap load.
    #[serde(skip, default = "default_capacity")]
    capacity: usize,
}

fn default_capacity() -> usize {
    DEFAULT_CAPACITY
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl ClientRegistry {
    pub fn new(capacity: usize) -> Self {
        Self {
            clients: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// Heap owned by the registry: the map's buckets plus each record's
    /// optional name. `ClientRecord::buckets` is a fixed-size array counted
    /// inside `size_of::<ClientRecord>()` by [`crate::heap::hashmap_bytes`].
    pub(crate) fn heap_bytes(&self) -> usize {
        crate::heap::hashmap_bytes::<IpAddr, ClientRecord>(self.clients.len())
            + self
                .clients
                .values()
                .filter_map(|record| record.name.as_deref())
                .map(crate::heap::string_bytes)
                .sum::<usize>()
    }

    pub fn record(&mut self, ip: IpAddr, at: SystemTime, blocked: bool, cache_hit: bool) {
        if !self.clients.contains_key(&ip) {
            // Unnamed clients go first (false < true), least-recently-seen
            // within each group; a named device only falls out when the
            // whole registry is named. `while` so an over-cap deserialized
            // load converges (see the `capacity` field).
            while self.clients.len() >= self.capacity {
                match self
                    .clients
                    .iter()
                    .min_by_key(|(_, record)| (record.name.is_some(), record.last_seen))
                    .map(|(ip, _)| *ip)
                {
                    Some(victim) => {
                        self.clients.remove(&victim);
                    }
                    None => break,
                }
            }
        }
        let record = self.clients.entry(ip).or_insert_with(|| ClientRecord {
            name: None,
            first_seen: at,
            last_seen: at,
            buckets: HourlyBuckets::new(),
        });
        record.last_seen = at;
        record.buckets.record(at, blocked, cache_hit);
    }

    pub fn name(&self, ip: IpAddr) -> Option<String> {
        self.clients.get(&ip).and_then(|record| record.name.clone())
    }

    pub fn set_name(&mut self, ip: IpAddr, name: Option<String>) -> Option<ClientView> {
        let record = self.clients.get_mut(&ip)?;
        record.name = name;
        Some(view(ip, record, SystemTime::now()))
    }

    pub fn list(&self, now: SystemTime) -> Vec<ClientView> {
        self.clients
            .iter()
            .map(|(ip, record)| view(*ip, record, now))
            .collect()
    }

    /// Clients ranked by their rolling 24h query count, for the stats
    /// snapshot's `top_clients`.
    pub fn top_by_activity(&self, now: SystemTime, n: usize) -> Vec<ClientView> {
        let mut views = self.list(now);
        views.sort_by(|a, b| {
            b.queries_24h
                .cmp(&a.queries_24h)
                .then_with(|| a.ip.cmp(&b.ip))
        });
        views.truncate(n);
        views
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.clients.len()
    }
}

fn view(ip: IpAddr, record: &ClientRecord, now: SystemTime) -> ClientView {
    let totals = record.buckets.totals(now);
    ClientView {
        ip,
        name: record.name.clone(),
        first_seen: record.first_seen,
        last_seen: record.last_seen,
        queries_24h: totals.queries,
        blocked_24h: totals.blocked,
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::*;

    fn ip(last_octet: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last_octet))
    }

    #[test]
    fn first_and_last_seen_track_across_multiple_records() {
        let mut registry = ClientRegistry::default();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(3600);
        registry.record(ip(1), t0, false, false);
        registry.record(ip(1), t1, true, false);

        let view = registry.list(t1).into_iter().next().unwrap();
        assert_eq!(view.first_seen, t0);
        assert_eq!(view.last_seen, t1);
        assert_eq!(view.queries_24h, 2);
        assert_eq!(view.blocked_24h, 1);
    }

    #[test]
    fn set_name_updates_and_persists_on_the_view() {
        let mut registry = ClientRegistry::default();
        registry.record(ip(1), SystemTime::now(), false, false);
        let updated = registry.set_name(ip(1), Some("liviu-phone".to_string()));
        assert_eq!(updated.unwrap().name.as_deref(), Some("liviu-phone"));
        assert_eq!(registry.name(ip(1)).as_deref(), Some("liviu-phone"));
    }

    #[test]
    fn set_name_on_unknown_client_is_none() {
        let mut registry = ClientRegistry::default();
        assert!(registry.set_name(ip(9), Some("x".to_string())).is_none());
    }

    #[test]
    fn capacity_evicts_the_least_recently_seen_client() {
        let mut registry = ClientRegistry::new(2);
        let t0 = SystemTime::UNIX_EPOCH;
        registry.record(ip(1), t0, false, false);
        registry.record(ip(2), t0 + Duration::from_secs(1), false, false);
        // ip(1) is the oldest by last_seen; a third distinct client evicts it.
        registry.record(ip(3), t0 + Duration::from_secs(2), false, false);

        assert_eq!(registry.len(), 2);
        assert!(registry.list(t0).iter().all(|v| v.ip != ip(1)));
    }

    #[test]
    fn eviction_spares_named_clients_over_fresher_unnamed_ones() {
        let mut registry = ClientRegistry::new(2);
        let t0 = SystemTime::UNIX_EPOCH;
        // The named device is the *oldest* by last_seen — plain LRU would
        // evict it; name-aware eviction takes the fresher unnamed one.
        registry.record(ip(1), t0, false, false);
        registry.set_name(ip(1), Some("liviu-phone".to_string()));
        registry.record(ip(2), t0 + Duration::from_secs(1), false, false);
        registry.record(ip(3), t0 + Duration::from_secs(2), false, false);

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.name(ip(1)).as_deref(), Some("liviu-phone"));
        assert!(registry.list(t0).iter().all(|v| v.ip != ip(2)));
    }

    #[test]
    fn memory_stays_bounded_under_many_distinct_clients() {
        let mut registry = ClientRegistry::new(100);
        for i in 0..10_000u32 {
            let addr = IpAddr::V4(Ipv4Addr::from(i.to_be_bytes()));
            registry.record(addr, SystemTime::now(), false, false);
        }
        assert!(registry.len() <= 100);
    }
}
