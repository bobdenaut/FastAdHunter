//! The API's responses as Rust types — the far side of the HTTP boundary.
//!
//! Bytes become these; nothing downstream sees a `serde_json::Value`. A shape
//! the API already publishes as a Rust type is imported rather than restated
//! (see [`telemetry::Telemetry`]).

pub mod config;
pub mod events;
pub mod history;
pub mod lan;
pub mod routeros;
pub mod telemetry;

/// JSON fixtures under `fixtures/`, shared by the model tests and the panel
/// tests.
///
/// **Captured from a running appliance, never hand-written.** A fixture written
/// from a field list encodes the same assumptions as the model it checks, so
/// the pair agree with each other and not with the server — which is how a
/// flattened `memory` block passed a full test suite. Refresh with a `curl`
/// against `/api/v1/…` and truncate the item arrays; change nothing else.
#[cfg(test)]
pub mod fixtures {
    pub const TELEMETRY: &str = include_str!("../../fixtures/telemetry.json");
    pub const TELEMETRY_ZERO: &str = include_str!("../../fixtures/telemetry-zero.json");
    pub const HISTORY_SUMMARY: &str = include_str!("../../fixtures/history-summary.json");
    pub const HISTORY_PERF: &str = include_str!("../../fixtures/history-perf.json");
    pub const EVENTS_QUERY: &str = include_str!("../../fixtures/events-query.json");
    pub const EVENTS_STATS: &str = include_str!("../../fixtures/events-stats.json");
    pub const ROUTEROS_RESOURCE: &str = include_str!("../../fixtures/routeros-resource.json");
    pub const ROUTEROS_CONTAINERS: &str = include_str!("../../fixtures/routeros-containers.json");

    /// The telemetry fixture, parsed. Panel tests render against this.
    pub fn telemetry() -> super::telemetry::Telemetry {
        serde_json::from_str(TELEMETRY).expect("fixtures/telemetry.json")
    }
}
