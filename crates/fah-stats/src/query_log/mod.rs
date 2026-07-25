//! Query log (CONTEXT.md: "bounded, persisted record of individual
//! queries"). Two tiers: [`ring::Ring`] — in-RAM, bounded, serves the query
//! API — and [`segment::SegmentWriter`] — batched append-only JSONL on
//! `/data`, pruned by age and size (ADR-0002: flat files, no embedded DB).

pub(crate) mod ring;
pub(crate) mod segment;

pub(crate) use ring::entry_string_bytes;

use std::net::IpAddr;
use std::time::SystemTime;

use fah_model::{QueryEvent, Verdict};
use serde::{Deserialize, Serialize};

/// One entry in the query log: the DNS pipeline's [`QueryEvent`] plus the
/// client name resolved from the registry at record time (so a later rename
/// doesn't retroactively change history) and a monotonic sequence number
/// (the query API's pagination cursor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryLogEntry {
    pub sequence: u64,
    pub event: QueryEvent,
    pub client_name: Option<String>,
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
        if let Some(ip) = self.client {
            if entry.event.query.client_ip != ip {
                return false;
            }
        }
        if let Some(domain) = &self.domain {
            if !entry.event.query.domain.contains(domain.as_str()) {
                return false;
            }
        }
        if let Some(kind) = self.verdict {
            if !kind.matches(&entry.event.verdict) {
                return false;
            }
        }
        if let Some(from) = self.from {
            if entry.event.query.timestamp < from {
                return false;
            }
        }
        if let Some(to) = self.to {
            if entry.event.query.timestamp > to {
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
