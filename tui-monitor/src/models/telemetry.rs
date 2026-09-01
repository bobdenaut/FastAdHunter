//! `GET /api/v1/telemetry` (API.md §Telemetry) — the whole engine state in one
//! request: ruleset, lifetime counters, per-stage latency, upstreams, cache and
//! memory.
//!
//! The engine half is [`fah_model::EngineTelemetry`] itself, which is the
//! published shape and derives `Deserialize`. Only `cache` and `memory` are
//! restated, because their wire types live in an L3 crate that would pull the
//! whole server in behind them.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Telemetry {
    pub process: ProcessInfo,
    /// `ruleset`, `counters`, `latency`, `upstreams` — served flattened at the
    /// top level.
    #[serde(flatten)]
    pub engine: fah_model::EngineTelemetry,
    pub cache: CacheStats,
    pub memory: Memory,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessInfo {
    pub version: String,
    /// The counter-reset marker: every figure here is process-lifetime, so a
    /// drop distinguishes a restart from a counter running backwards.
    pub uptime_seconds: u64,
}

/// The `cache` block — field-for-field `GET /api/v1/cache`.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct CacheStats {
    pub entries: u64,
    pub capacity: u64,
    pub fresh: u64,
    pub stale: u64,
    pub expired: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub bytes: u64,
    pub max_bytes: u64,
    pub load_percent: f64,
    pub byte_load_percent: f64,
}

impl CacheStats {
    pub fn lookup_hit_percent(&self) -> f64 {
        crate::util::format::percent(self.hits, self.hits + self.misses)
    }
}

/// The `memory` block: `GET /api/v1/debug/memory` minus the two
/// `allocator_committed_*` fields, which are specific to the linked allocator
/// and stay on `/debug/*`.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Memory {
    /// Served flattened into `memory`, not nested under a `components` key —
    /// `MemoryResponse` carries `#[serde(flatten)]` on it.
    #[serde(flatten)]
    pub components: MemoryComponents,
    pub cache_entries: u64,
    /// `None` off Linux, or when `/proc/self/status` is unreadable.
    pub process_rss: Option<u64>,
    pub process_peak_rss: Option<u64>,
    pub major_page_faults: Option<u64>,
    pub minor_page_faults: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct MemoryComponents {
    pub ruleset_bytes: u64,
    pub cache_estimated_bytes: u64,
    pub stats_aggregates_bytes: u64,
    pub stats_clients_bytes: u64,
    pub accounted_bytes: u64,
    /// `rss − accounted`: allocator overhead, thread stacks, fragmentation.
    /// `None` when RSS is unavailable — the subtraction has no left side.
    pub residual_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fixtures;

    /// The drift guard: this is the documented body, and a renamed or renested
    /// field must stop the build here rather than show zeros on the router.
    #[test]
    fn the_documented_response_body_parses_whole() {
        let telemetry: Telemetry = serde_json::from_str(fixtures::TELEMETRY).unwrap();

        assert_eq!(telemetry.process.version, "0.2.10");
        assert_eq!(telemetry.engine.ruleset.rules, 1_043_886);
        assert_eq!(telemetry.engine.counters.dns.block, 96_318);
        assert_eq!(telemetry.engine.latency.dns.forward.count, 269_446);
        assert_eq!(telemetry.engine.upstreams.len(), 2);
        assert_eq!(telemetry.engine.upstreams[0].address, "1.1.1.1:853");
        assert_eq!(telemetry.cache.entries, 1294);
        assert_eq!(telemetry.memory.components.residual_bytes, Some(31_895_996));
    }

    /// `#[serde(flatten)]` routes the engine block through serde's buffering
    /// path. The two duration fields are the only ones whose wire type differs
    /// from their Rust type, so they are the only ones that can silently drift.
    #[test]
    fn the_flattened_durations_survive_the_buffered_path() {
        let telemetry: Telemetry = serde_json::from_str(fixtures::TELEMETRY).unwrap();

        assert_eq!(
            telemetry.engine.ruleset.compile_duration,
            std::time::Duration::from_millis(7412)
        );
        assert_eq!(
            telemetry.engine.counters.cache_cleanup.last_duration,
            std::time::Duration::from_micros(1842)
        );
    }

    /// `fixtures/telemetry-zero.json` is a **captured** response from a freshly
    /// booted 0.2.11 container, not a hand-written one — which is how the flat
    /// `memory` block gets asserted against what the server really sends rather
    /// than against what this crate assumed.
    #[test]
    fn the_memory_block_is_flat_not_nested_under_components() {
        let telemetry: Telemetry = serde_json::from_str(fixtures::TELEMETRY_ZERO).unwrap();

        assert_eq!(telemetry.memory.components.ruleset_bytes, 68);
        assert_eq!(telemetry.memory.components.accounted_bytes, 9238);
        assert_eq!(telemetry.memory.process_rss, Some(46_084_096));
        assert_eq!(telemetry.memory.minor_page_faults, Some(10_609));
    }

    /// The same capture end to end: a fresh boot is all-zero counters with a
    /// live upstream pool, and must not read as "no data".
    #[test]
    fn a_freshly_booted_appliance_parses_whole() {
        let telemetry: Telemetry = serde_json::from_str(fixtures::TELEMETRY_ZERO).unwrap();

        assert_eq!(telemetry.process.version, "0.2.11");
        assert_eq!(telemetry.engine.counters.dns.pass, 0);
        assert_eq!(telemetry.engine.upstreams.len(), 2);
        assert_eq!(telemetry.cache.capacity, 10_000);
    }

    /// No field carries `#[serde(default)]`: one the server stops sending must
    /// fail loudly, not read back as a zero that charts like a measurement.
    #[test]
    fn a_missing_field_is_an_error_rather_than_a_zero() {
        let body = fixtures::TELEMETRY.replace("\"rules\": 1043886,", "");
        assert!(serde_json::from_str::<Telemetry>(&body).is_err());
    }

    #[test]
    fn the_two_hit_ratios_have_different_denominators() {
        let telemetry: Telemetry = serde_json::from_str(fixtures::TELEMETRY).unwrap();
        // 9921 / 15053 lookups, against the push's 26.8% of all queries.
        assert!((telemetry.cache.lookup_hit_percent() - 65.9).abs() < 0.1);
    }
}
