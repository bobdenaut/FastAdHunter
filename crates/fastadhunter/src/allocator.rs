//! Process allocator and its statistics.
//!
//! **This module is the only place in the workspace that names `mimalloc` or
//! `libmimalloc-sys`** — the single exception being `fah-rules`'
//! `tests/dedup_alloc_bound.rs`, which wraps the production allocator to make a
//! transient allocation's peak observable. Replacing the allocator means editing
//! this file and nothing else: [`stats`] returns [`fah_model::AllocatorStats`],
//! a plain L1 data type, so `/api/v1/telemetry` and `/api/v1/debug/memory` publish
//! allocator figures without any crate but this one knowing whose they are.
//! Kernel readings deliberately do **not** come through here — see
//! [`crate::process`].
//!
//! **Why not the platform allocator.** The shipped artefact is statically linked
//! against musl (ARCHITECTURE.md §Docker), so "the platform allocator" here is
//! musl's `mallocng` — not glibc's. `mallocng` is written for size and
//! hardening and has no per-thread cache, which is the worst case for a Tokio
//! multi-thread runtime allocating per query. See this module for the measured
//! context, including what this change is *not* expected to fix.

use fah_model::AllocatorStats;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The allocator's own commit counters at one instant, or `None` where they
/// cannot be read.
///
/// Callers must treat this as one sample: [`fah_model::MemoryBreakdown`]
/// requires every field to come from the same instant, because skew between the
/// components and the memory figures lands in the residual as noise.
///
/// Cheap enough for the 10 s telemetry poll — two relaxed atomic loads — and
/// never called from the DNS hot path.
///
/// **`current_commit` is not a live figure.** mimalloc v3 does not decrement it
/// on purge, so it reads as a lifetime high-water mark and can sit several times
/// above RSS. That is a property of the counter, not of this call; see
/// [`fah_model::AllocatorStats::current_commit`] before deriving anything from
/// it.
///
/// The figures describe whichever allocator the `GLOBAL` above installs; under a
/// different one the commit counters would read near zero. Keeping the read
/// beside the `#[global_allocator]` is what makes that impossible to overlook.
pub fn stats() -> Option<AllocatorStats> {
    let mut current_commit: usize = 0;
    let mut peak_commit: usize = 0;

    // Only the commit counters are read here. `peak_rss` and the fault counts
    // are kernel readings and come from `crate::process`, so replacing this
    // module cannot take them off `/api/v1/telemetry` with it.
    //
    // SAFETY: every parameter of `mi_process_info` is an optional out-pointer
    // that the function only writes through when non-null, and every pointer
    // passed here is a valid, uniquely borrowed, initialised `usize`. The
    // remaining out-params are passed as null, which the API documents as "skip
    // this field". The call itself is `noexcept` and allocates nothing.
    unsafe {
        libmimalloc_sys::mi_process_info(
            std::ptr::null_mut(), // elapsed_msecs
            std::ptr::null_mut(), // user_msecs
            std::ptr::null_mut(), // system_msecs
            std::ptr::null_mut(), // current_rss — see above
            std::ptr::null_mut(), // peak_rss — crate::process, from getrusage
            &mut current_commit,
            &mut peak_commit,
            std::ptr::null_mut(), // page_faults — crate::process
        );
    }

    Some(AllocatorStats {
        current_commit: current_commit as u64,
        peak_commit: peak_commit as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a numeric assertion — the figures are whatever the test binary has
    /// allocated. What this pins down is that the FFI call is sound and returns
    /// self-consistent values, since a mis-ordered out-parameter list would
    /// still compile and silently transpose two fields.
    #[test]
    fn process_info_is_readable_and_self_consistent() {
        let stats = stats().expect("mimalloc always reports its own counters");
        assert!(
            stats.current_commit > 0,
            "the test binary has allocated, so the allocator must hold something"
        );
        assert!(
            stats.peak_commit >= stats.current_commit,
            "a high-water mark cannot sit below the current value: \
             peak {} < current {}",
            stats.peak_commit,
            stats.current_commit
        );
    }
}
