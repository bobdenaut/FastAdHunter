//! Product data: aggregates, per-client activity, and snapshots
//! (ARCHITECTURE.md L3).
//!
//! [`Stats`] is the crate's one public handle — owns the aggregates
//! ([`aggregates`]) and the client registry ([`client_registry`]). The binary's
//! event fan-out — the single consumer of the DNS pipeline's bounded
//! [`fah_model::QueryEvent`] channel — calls [`Stats::record`] per event,
//! and periodic snapshots ([`snapshot`]) persist the aggregates so a
//! restart isn't zero'd (ADR-0002: no embedded DB).
//!
//! No per-query storage: this crate keeps aggregates and bounded per-client
//! counters. Individual events are published live on `WS /api/v1/events` and
//! are written nowhere.

mod aggregates;
mod bucket;
mod client_registry;
mod dto;
mod heap;
mod history;
mod snapshot;
mod stats;
mod top_n;

pub use aggregates::PolicyCount;
pub use bucket::BucketView;
pub use client_registry::{ClientView, InterceptedHandshakes, InterceptedOutcome};
pub use dto::{ClientCount, DomainCount, StatsSnapshot};
pub use stats::Stats;
