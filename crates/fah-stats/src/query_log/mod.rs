//! Query log (CONTEXT.md: "bounded, persisted record of individual
//! queries"). Two tiers: [`ring::Ring`] — in-RAM, bounded, serves the query
//! API — and [`segment::SegmentWriter`] — batched append-only JSONL on
//! `/data`, pruned by age and size (ADR-0002: flat files, no embedded DB).

pub(crate) mod ring;
pub(crate) mod segment;

pub(crate) use ring::entry_string_bytes;

use std::net::IpAddr;
use std::time::SystemTime;

use fah_model::{Event, EventKind, Verdict};
use serde::{Deserialize, Serialize};

/// One entry in the query log: a completed [`Event`] from either pipeline,
/// plus the client name resolved from the registry at record time (so a later
/// rename doesn't retroactively change history) and a monotonic sequence
/// number (the query API's pagination cursor).
///
/// **Format change at p2-04.** `event` used to be a bare `QueryEvent`; it is
/// now a tagged [`Event`], so a persisted record carries `"kind":"dns"` or
/// `"kind":"http"`. Records written before the upgrade have no tag and will
/// not parse. That is acceptable *here specifically* and nowhere else: the
/// query log is bounded and pruned by age, nothing reads the segments yet
/// (`p2-09` builds that reader), and the log self-heals within one retention
/// window. API.md says so rather than leaving it to be discovered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryLogEntry {
    pub sequence: u64,
    pub event: Event,
    pub client_name: Option<String>,
}

impl QueryLogEntry {
    /// The name this entry is *about* — a DNS question's domain or an HTTP
    /// request's host. One accessor so the `domain` filter, the aggregates and
    /// the API all read the same field rather than each picking one.
    pub fn name(&self) -> &str {
        match &self.event {
            Event::Dns(event) => &event.query.domain,
            Event::Http(event) => &event.request.host,
        }
    }
}

/// `verdict` filter values (API.md `GET /api/v1/queries`: `allow|block|pass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    Allow,
    Block,
    Pass,
}

impl VerdictKind {
    fn matches(self, verdict: &Verdict) -> bool {
        matches!(
            (self, verdict),
            (VerdictKind::Allow, Verdict::Allow(_))
                | (VerdictKind::Block, Verdict::Block(_))
                | (VerdictKind::Pass, Verdict::Pass)
        )
    }
}

/// Filters for [`ring::Ring::query`] (API.md `GET /api/v1/queries` query
/// string: `client`, `domain` (substring), `verdict`, `from`/`to`).
#[derive(Debug, Clone, Default)]
pub struct QueryLogFilter {
    pub client: Option<IpAddr>,
    pub domain: Option<String>,
    pub verdict: Option<VerdictKind>,
    pub from: Option<SystemTime>,
    pub to: Option<SystemTime>,
    /// `dns` / `http` (p2-04). `None` returns both, which is what an operator
    /// looking at "what did this client just do" wants by default.
    pub kind: Option<EventKind>,
}

impl QueryLogFilter {
    /// The pipeline lowercases domains before they reach the event channel;
    /// lowercasing the needle once here keeps the substring filter
    /// case-insensitive without a per-entry allocation in `matches`.
    pub(crate) fn normalized(&self) -> Self {
        let mut filter = self.clone();
        if let Some(domain) = &mut filter.domain {
            domain.make_ascii_lowercase();
        }
        filter
    }

    fn matches(&self, entry: &QueryLogEntry) -> bool {
        if let Some(kind) = self.kind {
            if entry.event.kind() != kind {
                return false;
            }
        }
        if let Some(ip) = self.client {
            if entry.event.client_ip() != ip {
                return false;
            }
        }
        if let Some(needle) = &self.domain {
            // The same filter reads a DNS question's name and an HTTP
            // request's host: an operator searching "doubleclick" means the
            // same thing in both pipelines and should not have to know which
            // one answered.
            if !entry.name().contains(needle.as_str()) {
                return false;
            }
        }
        if let Some(kind) = self.verdict {
            if !kind.matches(entry.event.verdict()) {
                return false;
            }
        }
        if let Some(from) = self.from {
            if entry.event.timestamp() < from {
                return false;
            }
        }
        if let Some(to) = self.to {
            if entry.event.timestamp() > to {
                return false;
            }
        }
        true
    }
}

/// A page of query log results, newest first (API.md `GET /api/v1/queries`).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPage {
    pub items: Vec<QueryLogEntry>,
    pub next_cursor: Option<String>,
}
