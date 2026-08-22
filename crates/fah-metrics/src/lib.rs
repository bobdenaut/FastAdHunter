//! [`Metrics`] is the one registry instance. The binary's event fan-out —
//! the single consumer of the pipeline's `QueryEvent` channel, shared with
//! `fah-stats` and the WS hub — calls [`Metrics::record`] per event (query
//! counters by verdict, cache hit/miss/stale, per-stage latency histograms —
//! all hot-path safe, atomics only) plus periodic snapshots it pushes in from
//! `fah-dns`'s and
//! `fah-rules`' own counters ([`Metrics::set_dropped_events`],
//! [`Metrics::set_upstreams`], [`Metrics::set_ruleset`]) — siblings never
//! import each other's types (ARCHITECTURE.md §Dependency Layering), so where
//! no shared shape exists this crate defines its own DTO rather than depending
//! on `fah-dns`/`fah-rules` ([`RulesetSnapshot`]). Where one *does* exist at
//! L1, it is used directly: upstream rows are [`fah_model::UpstreamSample`],
//! the same type the pool status carries and every persisted `PerfSample`
//! stores, so nothing is remapped between them.
//!
//! [`Metrics::engine_telemetry`] renders the registry as the L1 value
//! `GET /api/v1/telemetry` publishes.

mod histogram;
mod registry;
mod ruleset;
mod snapshot;

pub use registry::Metrics;
pub use ruleset::RulesetSnapshot;
pub use snapshot::{CleanupSnapshot, MetricsSnapshot, StageHistogram, SwrSnapshot};
