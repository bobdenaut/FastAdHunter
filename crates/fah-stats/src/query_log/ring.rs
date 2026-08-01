//! In-RAM bounded ring: the query API's read path (API.md `GET
//! /api/v1/queries`). Capacity is `[query_log].ring_entries`
//! (CONFIGURATION.md) — a fixed-size `VecDeque`, oldest entry evicted on
//! overflow, so memory never grows with query volume.

use std::collections::VecDeque;

use fah_model::Event;

use super::{QueryLogEntry, QueryLogFilter, QueryPage};

pub(crate) struct Ring {
    entries: VecDeque<QueryLogEntry>,
    capacity: usize,
    next_sequence: u64,
    /// Owned string bytes across all resident entries, maintained on push and
    /// eviction rather than walked on read (p2-07).
    ///
    /// The ring is the largest counted structure — 16,384 entries by default,
    /// each holding a heap-allocated domain — so walking it cost ~80 µs per
    /// accounting call on x86 and an estimated 0.4–0.6 ms on the RB5009, with
    /// the ring mutex held. That is only 0.005 % duty cycle at a 10 s poll, but
    /// it is paid forever and it is latency a query can land on. Unlike the
    /// bounded counters, this structure has exactly one mutation point, so a
    /// running total is cheap to keep correct — the same reasoning that made
    /// the DNS cache track its own bytes.
    bytes: usize,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_sequence: 0,
            bytes: 0,
        }
    }

    /// Heap owned by the ring: the `VecDeque` buffer at its configured
    /// capacity — the allocation is what occupies RAM, not the fill — plus the
    /// running total of every resident entry's owned strings. O(1); see
    /// [`Self::bytes`]. Excludes the segment files on `/data`, which
    /// `retention_max_mb` bounds separately.
    pub(crate) fn heap_bytes(&self) -> usize {
        crate::heap::vecdeque_bytes::<QueryLogEntry>(self.capacity) + self.bytes
    }

    /// Pushes a new entry, evicting the oldest on overflow. Returns the
    /// stored entry (cloned) so the caller can also hand it to the segment
    /// writer's pending batch.
    pub fn push(&mut self, event: Event, client_name: Option<String>) -> QueryLogEntry {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let entry = QueryLogEntry {
            sequence,
            event,
            client_name,
        };
        if self.entries.len() >= self.capacity {
            if let Some(evicted) = self.entries.pop_front() {
                self.bytes -= entry_string_bytes(&evicted);
            }
        }
        self.bytes += entry_string_bytes(&entry);
        self.entries.push_back(entry.clone());
        entry
    }

    /// Newest-first page: `cursor` (exclusive) is the last-returned entry's
    /// sequence number, so the next page picks up strictly older entries.
    pub fn query(&self, filter: &QueryLogFilter, limit: usize, cursor: Option<u64>) -> QueryPage {
        let limit = limit.max(1);
        let mut items: Vec<QueryLogEntry> = self
            .entries
            .iter()
            .rev()
            .filter(|entry| cursor.is_none_or(|c| entry.sequence < c))
            .filter(|entry| filter.matches(entry))
            .take(limit + 1)
            .cloned()
            .collect();

        let next_cursor = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|entry| entry.sequence.to_string())
        } else {
            None
        };

        QueryPage { items, next_cursor }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Owned string bytes behind one log entry: the queried domain, the optional
/// client name, and the decisive rule's list/rule text when a verdict carries
/// one. Shared here and by the pending batch, which holds the same type.
pub(crate) fn entry_string_bytes(entry: &QueryLogEntry) -> usize {
    use fah_model::Verdict;

    let verdict = match entry.event.verdict() {
        Verdict::Block(rule) | Verdict::Allow(rule) => {
            crate::heap::arc_str_bytes(&rule.list) + crate::heap::arc_str_bytes(&rule.rule)
        }
        Verdict::Pass => 0,
    };
    // An HTTP entry owns more strings than a DNS one — host, path and method
    // — and undercounting them would understate the ring's own memory, which
    // is the number the byte cap is enforced against.
    let request = match &entry.event {
        Event::Dns(_) => 0,
        Event::Http(event) => {
            crate::heap::string_bytes(&event.request.path)
                + crate::heap::string_bytes(&event.request.method)
        }
    };
    request
        + crate::heap::string_bytes(entry.name())
        + entry
            .client_name
            .as_deref()
            .map_or(0, crate::heap::string_bytes)
        + verdict
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::SystemTime;

    use fah_model::{DecisiveRule, Query, QueryType, Verdict};

    use super::*;
    use crate::query_log::VerdictKind;

    fn event(domain: &str, verdict: Verdict) -> Event {
        Event::dns(dns_event(domain, verdict))
    }

    fn dns_event(domain: &str, verdict: Verdict) -> fah_model::QueryEvent {
        fah_model::QueryEvent::new(
            Query::new(
                domain,
                QueryType::A,
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
                SystemTime::now(),
            ),
            verdict,
            std::time::Duration::from_micros(100),
            false,
            true,
            false,
        )
    }

    #[test]
    fn overflow_evicts_the_oldest_entry() {
        let mut ring = Ring::new(2);
        ring.push(event("a.example.com", Verdict::Pass), None);
        ring.push(event("b.example.com", Verdict::Pass), None);
        ring.push(event("c.example.com", Verdict::Pass), None);

        assert_eq!(ring.len(), 2);
        let page = ring.query(&QueryLogFilter::default(), 10, None);
        let domains: Vec<_> = page.items.iter().map(|e| e.name().to_string()).collect();
        assert_eq!(domains, vec!["c.example.com", "b.example.com"]);
    }

    /// The failure mode a running total introduces: drifting from reality
    /// after evictions. Asserts the tracked figure equals a full walk (p2-07).
    #[test]
    fn tracked_bytes_match_a_full_walk_across_eviction() {
        let mut ring = Ring::new(64);
        for i in 0..500 {
            // Varying lengths, plus verdicts that own `Arc<str>` payloads, so
            // an eviction that subtracted the wrong amount would show up.
            let verdict = if i % 3 == 0 {
                Verdict::Block(DecisiveRule::new("oisd", format!("||ads{i}.example.com^")))
            } else {
                Verdict::Pass
            };
            let name = (i % 4 == 0).then(|| format!("client-name-{i}"));
            ring.push(
                event(&format!("d{i}.some-domain-{i}.example.com"), verdict),
                name,
            );
        }

        let walked: usize = ring.entries.iter().map(entry_string_bytes).sum();
        assert_eq!(
            ring.bytes, walked,
            "the running total drifted from the real contents after eviction"
        );
        assert_eq!(
            ring.heap_bytes(),
            crate::heap::vecdeque_bytes::<QueryLogEntry>(64) + walked
        );
    }

    #[test]
    fn query_is_newest_first() {
        let mut ring = Ring::new(10);
        ring.push(event("a.example.com", Verdict::Pass), None);
        ring.push(event("b.example.com", Verdict::Pass), None);

        let page = ring.query(&QueryLogFilter::default(), 10, None);
        assert_eq!(page.items[0].name(), "b.example.com");
        assert_eq!(page.items[1].name(), "a.example.com");
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn pagination_cursor_walks_older_pages() {
        let mut ring = Ring::new(10);
        for i in 0..5 {
            ring.push(event(&format!("d{i}.example.com"), Verdict::Pass), None);
        }

        let first = ring.query(&QueryLogFilter::default(), 2, None);
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.items[0].name(), "d4.example.com");
        let cursor = first.next_cursor.clone().unwrap();

        let second = ring.query(&QueryLogFilter::default(), 2, Some(cursor.parse().unwrap()));
        assert_eq!(second.items[0].name(), "d2.example.com");
        assert!(second.next_cursor.is_some());

        let third = ring.query(
            &QueryLogFilter::default(),
            2,
            Some(second.next_cursor.unwrap().parse().unwrap()),
        );
        assert_eq!(third.items.len(), 1);
        assert!(third.next_cursor.is_none());
    }

    #[test]
    fn filter_by_domain_substring() {
        let mut ring = Ring::new(10);
        ring.push(event("ads.example.com", Verdict::Pass), None);
        ring.push(event("api.example.com", Verdict::Pass), None);

        let filter = QueryLogFilter {
            domain: Some("ads".to_string()),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name(), "ads.example.com");
    }

    #[test]
    fn filter_by_verdict() {
        let mut ring = Ring::new(10);
        ring.push(
            event(
                "ads.example.com",
                Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
            ),
            None,
        );
        ring.push(event("example.com", Verdict::Pass), None);

        let filter = QueryLogFilter {
            verdict: Some(VerdictKind::Block),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name(), "ads.example.com");
    }

    #[test]
    fn filter_by_time_range() {
        use std::time::{Duration, UNIX_EPOCH};

        let mut ring = Ring::new(10);
        for minute in [10u64, 20, 30] {
            let mut ev = dns_event(&format!("m{minute}.example.com"), Verdict::Pass);
            ev.query.timestamp = UNIX_EPOCH + Duration::from_secs(minute * 60);
            let ev = Event::dns(ev);
            ring.push(ev, None);
        }

        let filter = QueryLogFilter {
            from: Some(UNIX_EPOCH + Duration::from_secs(15 * 60)),
            to: Some(UNIX_EPOCH + Duration::from_secs(25 * 60)),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name(), "m20.example.com");
    }

    #[test]
    fn filter_by_client_ip() {
        let mut ring = Ring::new(10);
        let mut ev = dns_event("example.com", Verdict::Pass);
        ev.query.client_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let ev = Event::dns(ev);
        ring.push(ev, None);
        ring.push(event("other.example.com", Verdict::Pass), None);

        let filter = QueryLogFilter {
            client: Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name(), "example.com");
    }
}
