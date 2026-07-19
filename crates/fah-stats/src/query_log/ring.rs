//! In-RAM bounded ring: the query API's read path (API.md `GET
//! /api/v1/queries`). Capacity is `[query_log].ring_entries`
//! (CONFIGURATION.md) — a fixed-size `VecDeque`, oldest entry evicted on
//! overflow, so memory never grows with query volume.

use std::collections::VecDeque;

use fah_model::QueryEvent;

use super::{QueryLogEntry, QueryLogFilter, QueryPage};

pub(crate) struct Ring {
    entries: VecDeque<QueryLogEntry>,
    capacity: usize,
    next_sequence: u64,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_sequence: 0,
        }
    }

    /// Pushes a new entry, evicting the oldest on overflow. Returns the
    /// stored entry (cloned) so the caller can also hand it to the segment
    /// writer's pending batch.
    pub fn push(&mut self, event: QueryEvent, client_name: Option<String>) -> QueryLogEntry {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let entry = QueryLogEntry {
            sequence,
            event,
            client_name,
        };
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::SystemTime;

    use fah_model::{DecisiveRule, Query, QueryType, Verdict};

    use super::*;
    use crate::query_log::VerdictKind;

    fn event(domain: &str, verdict: Verdict) -> QueryEvent {
        QueryEvent::new(
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
        let domains: Vec<_> = page
            .items
            .iter()
            .map(|e| e.event.query.domain.clone())
            .collect();
        assert_eq!(domains, vec!["c.example.com", "b.example.com"]);
    }

    #[test]
    fn query_is_newest_first() {
        let mut ring = Ring::new(10);
        ring.push(event("a.example.com", Verdict::Pass), None);
        ring.push(event("b.example.com", Verdict::Pass), None);

        let page = ring.query(&QueryLogFilter::default(), 10, None);
        assert_eq!(page.items[0].event.query.domain, "b.example.com");
        assert_eq!(page.items[1].event.query.domain, "a.example.com");
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
        assert_eq!(first.items[0].event.query.domain, "d4.example.com");
        let cursor = first.next_cursor.clone().unwrap();

        let second = ring.query(&QueryLogFilter::default(), 2, Some(cursor.parse().unwrap()));
        assert_eq!(second.items[0].event.query.domain, "d2.example.com");
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
        assert_eq!(page.items[0].event.query.domain, "ads.example.com");
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
        assert_eq!(page.items[0].event.query.domain, "ads.example.com");
    }

    #[test]
    fn filter_by_time_range() {
        use std::time::{Duration, UNIX_EPOCH};

        let mut ring = Ring::new(10);
        for minute in [10u64, 20, 30] {
            let mut ev = event(&format!("m{minute}.example.com"), Verdict::Pass);
            ev.query.timestamp = UNIX_EPOCH + Duration::from_secs(minute * 60);
            ring.push(ev, None);
        }

        let filter = QueryLogFilter {
            from: Some(UNIX_EPOCH + Duration::from_secs(15 * 60)),
            to: Some(UNIX_EPOCH + Duration::from_secs(25 * 60)),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].event.query.domain, "m20.example.com");
    }

    #[test]
    fn filter_by_client_ip() {
        let mut ring = Ring::new(10);
        let mut ev = event("example.com", Verdict::Pass);
        ev.query.client_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        ring.push(ev, None);
        ring.push(event("other.example.com", Verdict::Pass), None);

        let filter = QueryLogFilter {
            client: Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))),
            ..Default::default()
        };
        let page = ring.query(&filter, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].event.query.domain, "example.com");
    }
}
