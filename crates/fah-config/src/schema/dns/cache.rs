use serde::{Deserialize, Serialize};

use crate::schema::default_true;

/// `[dns.cache]` (CONFIGURATION.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DnsCacheConfig {
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
    /// Byte ceiling on what the cached answers themselves hold, enforced by
    /// the same FIFO eviction as `max_entries` — whichever bound binds first
    /// evicts. Entry count alone cannot bound memory: the ~91h soak
    /// (docs/code-review/p1-11-soak.md) filled a bounded cache with
    /// large TXT/SOA/NXDOMAIN answers and reached ~230 MiB, 80% over the
    /// 128 MB budget, while real traffic sat at ~55 MiB.
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
    #[serde(default)]
    pub min_ttl_seconds: u32,
    #[serde(default = "default_max_ttl_seconds")]
    pub max_ttl_seconds: u32,
    #[serde(default = "default_negative_ttl_max_seconds")]
    pub negative_ttl_max_seconds: u32,
    #[serde(default = "default_true")]
    pub serve_stale: bool,
    /// Size of the detached pool that refreshes stale entries in the
    /// background (ADR-0005). A stale hit is answered from cache immediately
    /// and a refresh job is enqueued; these workers consume it. `0` disables
    /// stale-while-refresh entirely, restoring the pre-ADR-0005 behaviour
    /// where a stale entry answers only after a forward has failed.
    ///
    /// The pool size is the whole priority mechanism: at most this many
    /// refreshes are ever in flight, whatever the query rate. Refreshes are
    /// I/O-bound, so they cost upstream bandwidth rather than CPU. Meaningless
    /// while `serve_stale = false` — there are no stale entries to refresh.
    #[serde(default = "default_swr_workers")]
    pub swr_workers: u32,
    /// How often a background task sweeps entries past the serve-stale window
    /// out of the cache. `0` disables the sweep; the cache is still bounded by
    /// `max_entries` and `max_bytes` either way, so this returns memory rather
    /// than capping it.
    ///
    /// Expect it to reclaim very little at default settings, and do not read
    /// that as a fault: with `serve_stale = true` an entry is only sweepable
    /// 24 h after its TTL lapsed, and under any real query rate FIFO eviction
    /// has taken such entries long before. What it is actually for is the
    /// cache that goes idle *below* both caps — a household resolver overnight
    /// — which nothing else ever reclaims.
    #[serde(default = "default_cleanup_interval_seconds")]
    pub cleanup_interval_seconds: u32,
}

impl Default for DnsCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: default_max_entries(),
            max_bytes: default_max_bytes(),
            min_ttl_seconds: 0,
            max_ttl_seconds: default_max_ttl_seconds(),
            negative_ttl_max_seconds: default_negative_ttl_max_seconds(),
            serve_stale: default_true(),
            swr_workers: default_swr_workers(),
            cleanup_interval_seconds: default_cleanup_interval_seconds(),
        }
    }
}

fn default_max_entries() -> u32 {
    10_000
}

/// 64 MiB — half of PERFORMANCE.md's 128 MB steady-state budget, which leaves
/// room for the compiled ruleset (≤40 MB at 1M domains) and the process base
/// underneath the ceiling even when the cache is completely full of
/// adversarially large answers. Non-binding at the default `max_entries`
/// (10 000 real answers are a few MB); it starts mattering exactly where
/// CONFIGURATION.md suggests raising entries "to 100k+ if RAM allows".
fn default_max_bytes() -> u64 {
    64 * 1024 * 1024
}

fn default_max_ttl_seconds() -> u32 {
    86_400
}

fn default_negative_ttl_max_seconds() -> u32 {
    60
}

/// Three background refreshers. Enough that a household's stale entries are
/// refreshed promptly without a queue building, few enough that a burst of
/// simultaneously-expiring entries cannot put more than three extra queries on
/// the wire at once — which is what keeps the RB5009's upstream link and the
/// cache's shard locks out of contention with serving.
fn default_swr_workers() -> u32 {
    3
}

/// Six minutes. Long enough that the sweep is invisible — a walk of a
/// 10 000-entry cache costs microseconds, so at this cadence it is a rounding
/// error against the query load — and short enough that memory a cache stopped
/// needing comes back within one idle stretch rather than at the next restart.
///
/// Nothing depends on the exact figure: the sweep is idempotent and holds one
/// shard lock at a time, so halving or doubling it changes only how promptly
/// dead entries leave.
fn default_cleanup_interval_seconds() -> u32 {
    360
}
