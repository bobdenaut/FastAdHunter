//! `GET /api/v1/telemetry` — the whole engine state in one JSON request.
//!
//! Exists because a dashboard rendering one screen otherwise has to poll
//! `/api/v1/cache` and `/api/v1/debug/memory` separately, and the compiled
//! rule count, the per-stage latency figures and the per-upstream counters
//! have no other JSON home.
//!
//! ## What belongs here
//!
//! Metrics are placed by **who produces them**. FastAdHunter's own counters and
//! the kernel's readings are served here under a stable contract; figures
//! specific to whichever allocator is linked in stay on `/api/v1/debug/*`,
//! which promises nothing (see [`crate::wire::DebugMemoryResponse`]).
//!
//! > `/api/v1/telemetry` is intended to remain backward-compatible across
//! > releases. New fields may be added; existing fields must not change meaning
//! > or units.
//!
//! ## One gathering site
//!
//! [`TelemetrySnapshot::collect`] reads every source in one function rather
//! than letting handlers each take their own reading. This is **bounded skew,
//! not atomicity** — the counters are independent atomics read one at a time
//! and RSS is a separate syscall, so a simultaneous read is not achievable.
//! What it removes is skew spread across three handlers plus a poll interval,
//! which matters because `residual = rss − Σcomponents`
//! ([`fah_model::MemoryBreakdown`]) and any skew between those inputs lands in
//! it as noise.
//!
//! ## Why there is barely a wire type here
//!
//! [`fah_model::EngineTelemetry`] already *is* the published shape and carries
//! `Serialize`. Restating it as a parallel set of response structs would have
//! been eight field-identical copies and a second allocation per request, for
//! an identity mapping — see that type's own note. This module adds only what
//! `fah-api` owns and `fah-metrics` cannot know: process identity, and the
//! cache and memory blocks it assembles from the other ports.

use std::sync::Arc;

use serde::Serialize;

use crate::state::AppState;
use crate::wire::{CacheStatsResponse, MemoryResponse};

/// Where the memory goes, plus the cache read it is derived from — the whole
/// of `GET /api/v1/debug/memory` and the `cache`/`memory` blocks of
/// `/api/v1/telemetry`.
///
/// Holds domain values rather than wire types, so gathering stays this
/// module's concern and JSON stays the wire layer's.
pub struct MemorySnapshot {
    memory: fah_model::MemoryBreakdown,
    cache: crate::ports::CacheStats,
}

impl MemorySnapshot {
    /// Reads the cache, the stats heap, the ruleset heap, RSS and both stat
    /// blocks — once each, in one pass.
    ///
    /// `cache.stats()` is a bounded walk and RSS is a `/proc/self/status` read,
    /// so this is a read-path endpoint rather than a free one — the same cost
    /// `/debug/memory` already carried, now paid once for both blocks instead
    /// of once per endpoint.
    pub fn collect(state: &Arc<AppState>) -> Self {
        let cache = state.cache.stats();
        Self {
            memory: fah_model::MemoryBreakdown {
                components: fah_model::MemoryComponents {
                    ruleset: state.rules.matcher().heap_bytes() as u64,
                    cache: cache.estimated_bytes,
                    stats: state.stats.heap(),
                },
                rss: fah_common::process::resident_bytes(),
                // Both through the port, so this crate never learns which
                // allocator is installed (`crates/fastadhunter/src/allocator.rs`).
                process: state.telemetry.process(),
                allocator: state.telemetry.allocator(),
            },
            cache,
        }
    }

    pub fn breakdown(&self) -> &fah_model::MemoryBreakdown {
        &self.memory
    }

    pub fn cache_entries(&self) -> u64 {
        self.cache.entries
    }
}

/// [`MemorySnapshot`] plus what only `/api/v1/telemetry` serves.
///
/// The engine read is **not** folded into `MemorySnapshot`: it clones the
/// upstream vector and walks ~20 atomics, none of which `/debug/memory`
/// publishes — the same objection `Metrics::engine_telemetry` raises against
/// building on `Metrics::snapshot`.
pub struct TelemetrySnapshot {
    memory: MemorySnapshot,
    engine: fah_model::EngineTelemetry,
    uptime_seconds: u64,
}

impl TelemetrySnapshot {
    pub fn collect(state: &Arc<AppState>) -> Self {
        Self {
            memory: MemorySnapshot::collect(state),
            engine: state.telemetry.engine(),
            uptime_seconds: state.uptime_seconds(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub process: ProcessResponse,
    /// `ruleset`, `counters`, `latency` and `upstreams`, straight off the
    /// engine value — flattened so they sit at the top level beside the blocks
    /// this crate adds.
    #[serde(flatten)]
    pub engine: fah_model::EngineTelemetry,
    pub cache: CacheStatsResponse,
    pub memory: MemoryResponse,
}

/// Process identity — and the marker that makes every counter above readable.
#[derive(Debug, Serialize)]
pub struct ProcessResponse {
    pub version: &'static str,
    /// **Read this before deltaing any counter.** All of them are
    /// process-lifetime, so they return to zero on restart; a drop here is what
    /// distinguishes that from a counter going backwards for any other reason.
    pub uptime_seconds: u64,
}

impl From<TelemetrySnapshot> for TelemetryResponse {
    fn from(snapshot: TelemetrySnapshot) -> Self {
        let memory = snapshot.memory;
        Self {
            process: ProcessResponse {
                version: env!("CARGO_PKG_VERSION"),
                uptime_seconds: snapshot.uptime_seconds,
            },
            engine: snapshot.engine,
            // Derived from the port snapshot exactly as `GET /api/v1/cache`
            // derives it — never from that endpoint's output, which would
            // couple the two.
            cache: memory.cache.into(),
            memory: MemoryResponse::of(memory.breakdown(), memory.cache_entries()),
        }
    }
}
