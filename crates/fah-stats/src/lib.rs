//! Product data: query log, aggregates, and snapshots (ARCHITECTURE.md L3).
//!
//! [`Stats`] is the crate's one public handle — owns the aggregates
//! ([`aggregates`]), the client registry ([`client_registry`]) and the query
//! log (in-RAM ring + `/data` JSONL segments, [`query_log`]). The binary's
//! event fan-out — the single consumer of the DNS pipeline's bounded
//! [`fah_model::QueryEvent`] channel — calls [`Stats::record`] per event,
//! and periodic snapshots ([`snapshot`]) persist the aggregates so a
//! restart isn't zero'd (ADR-0002: no embedded DB).

mod aggregates;
mod bucket;
mod client_registry;
mod dto;
mod query_log;
mod snapshot;
mod stats;
mod top_n;

pub use bucket::BucketView;
pub use client_registry::ClientView;
pub use dto::{ClientCount, DomainCount, StatsSnapshot};
pub use query_log::{QueryLogEntry, QueryLogFilter, QueryPage, VerdictKind};
pub use stats::Stats;
