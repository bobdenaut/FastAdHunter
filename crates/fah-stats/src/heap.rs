//! Heap accounting for the bounded in-RAM structures (p2-07).
//!
//! # Why this exists
//!
//! RSS answers "is memory growing?" but never "growing *where*?". With every
//! bounded structure reporting its own size, `RSS − Σ(components)` is a
//! **residual** — and a leak shows as a growing residual while the named
//! components stay flat. Measured on the RB5009 before this existed, 45 % of
//! RSS was unaccounted, which is precisely where a slow leak would have hidden.
//!
//! # What these numbers are, and are not
//!
//! Every `heap_bytes` in this crate counts **heap allocations owned by the
//! structure**: container capacity plus the bytes behind any pointer it owns.
//! They deliberately exclude:
//!
//! - the struct's own inline size when it lives inside another counted
//!   structure (counting it twice would understate the residual);
//! - allocator overhead and fragmentation — musl's arenas are a real part of
//!   RSS and belong in the residual, not attributed to a component;
//! - anything reachable through an `Arc` shared with another component, which
//!   is counted once at its owner.
//!
//! An unstated exclusion silently becomes residual, which is why each method
//! names its own. `p1-02-review.md` §4 already caught a `heap_bytes` counting
//! `Arc` control blocks but not their payloads — in the number feeding a
//! budget claim.
//!
//! # Cost, measured
//!
//! `Stats::heap()` over a saturated system: **~43 µs** on x86, an estimated
//! 200–300 µs on the RB5009's 1.4 GHz core. On the 10 s telemetry poll that is
//! ~0.003 % duty cycle, and it never touches the query path.
//!
//! The split is deliberate, not uniform:
//!
//! - **The ring keeps a running total.** It is the largest structure (16,384
//!   entries, each with a heap-allocated domain) and walking it was ~half the
//!   total cost — 80 µs before, 43 after. It also has exactly *one* mutation
//!   point, so the total is cheap to keep correct, and it is guarded by a test
//!   asserting the tracked figure equals a full walk after eviction. Same
//!   reasoning as the DNS cache, which tracks its bytes for eviction decisions.
//! - **The bounded counters are walked.** `top_n` (≤256 keys per hour slot) and
//!   `ClientRegistry` (≤4,096) have eviction logic with several mutation
//!   points — min-count eviction, LRU with a named/unnamed preference — where a
//!   running total is where drift bugs live. At their size the walk is a few
//!   tens of µs, so buying complexity there would trade a real risk for an
//!   immaterial gain.

use std::collections::HashMap;
use std::sync::Arc;

/// Per-slot cost of a `HashMap<K, V>`: hashbrown stores keys, values and one
/// control byte per bucket, and keeps capacity at 8/7 of the requested size.
/// Approximate by construction — the map does not expose its true allocation.
pub(crate) fn hashmap_bytes<K, V>(len: usize) -> usize {
    let slots = len.saturating_mul(8).div_ceil(7).next_power_of_two();
    slots * (std::mem::size_of::<K>() + std::mem::size_of::<V>() + 1)
}

/// A `VecDeque<T>`'s buffer. Capacity, not length — the allocation is what
/// occupies RAM.
pub(crate) fn vecdeque_bytes<T>(capacity: usize) -> usize {
    capacity * std::mem::size_of::<T>()
}

/// The payload behind an `Arc<str>`: string bytes plus the two reference
/// counts in the control block.
pub(crate) fn arc_str_bytes(value: &Arc<str>) -> usize {
    value.len() + 2 * std::mem::size_of::<usize>()
}

/// A `String`'s buffer. Capacity, not length.
pub(crate) fn string_bytes(value: &str) -> usize {
    value.len()
}

/// Sum of a map's `Arc<str>` keys, for the bounded domain counters.
pub(crate) fn arc_key_bytes<V>(map: &HashMap<Arc<str>, V>) -> usize {
    map.keys().map(arc_str_bytes).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashmap_cost_grows_with_length_and_never_underflows() {
        assert_eq!(hashmap_bytes::<u64, u64>(0), 8 + 8 + 1);
        assert!(hashmap_bytes::<u64, u64>(100) > hashmap_bytes::<u64, u64>(10));
    }

    #[test]
    fn arc_str_counts_payload_and_control_block() {
        let value: Arc<str> = Arc::from("example.com");
        assert_eq!(
            arc_str_bytes(&value),
            "example.com".len() + 2 * std::mem::size_of::<usize>()
        );
    }
}
