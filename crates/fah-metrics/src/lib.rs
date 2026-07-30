//! Ops telemetry: Prometheus counters and histograms (ARCHITECTURE.md L3;
//! CONTEXT.md distinguishes Metrics — operational, for operators — from
//! Statistics — product data, for users, `fah-stats`).
//!
//! [`Metrics`] is the one registry instance. The binary's event fan-out —
//! the single consumer of the pipeline's `QueryEvent` channel, shared with
//! `fah-stats` and the WS hub — calls [`Metrics::record`] per event (query
//! counters by verdict, cache hit/miss/stale, per-stage latency histograms —
//! all hot-path safe, atomics only) plus periodic snapshots it pushes in from
//! `fah-dns`'s and
//! `fah-rules`' own counters ([`Metrics::set_dropped_events`],
//! [`Metrics::set_upstreams`], [`Metrics::set_ruleset`]) — siblings never
//! import each other's types (ARCHITECTURE.md §Dependency Layering), so this
//! crate defines its own [`UpstreamSnapshot`]/[`RulesetSnapshot`] DTOs rather
//! than depending on `fah-dns`/`fah-rules`. [`encode`] renders the registry
//! as Prometheus text exposition format for `fah-api`'s `GET /metrics`
//! (p1-09) to serve verbatim.

mod encode;
mod histogram;
mod process;
mod registry;
mod ruleset;
mod snapshot;
mod upstream;

pub use encode::encode;
pub use process::resident_memory_bytes;
pub use registry::Metrics;
pub use ruleset::RulesetSnapshot;
pub use snapshot::{MetricsSnapshot, StageHistogram, SwrSnapshot};
pub use upstream::UpstreamSnapshot;
