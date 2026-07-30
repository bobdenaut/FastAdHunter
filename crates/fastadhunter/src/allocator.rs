//! Process allocator and its statistics.
//!
//! **This module is the only place in the workspace that names `mimalloc` or
//! `libmimalloc-sys`** — the single exception being `fah-rules`'
//! `tests/dedup_alloc_bound.rs`, which wraps the production allocator to make a
//! transient allocation's peak observable. Replacing the allocator means editing
//! this file and nothing else: [`stats`] returns [`fah_model::AllocatorStats`],
//! a plain L1 data type, so `/metrics` and `/api/v1/debug/memory` publish
//! allocator figures without any crate but this one knowing whose they are.
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

/// Allocator and process figures at one instant, or `None` where they cannot be
/// read.
///
/// Callers must treat this as one sample: [`fah_model::MemoryBreakdown`]
/// requires every field to come from the same instant, because skew between the
/// components and the memory figures lands in the residual as noise.
///
/// Cheap enough for the 10 s telemetry poll — two `getrusage` calls plus two
/// relaxed atomic loads — and never called from the DNS hot path. The cost of
/// the whole accounting pass is exported as
/// `fastadhunter_memory_collection_seconds`, so it is measured rather than
/// assumed.
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
    let mut peak_rss: usize = 0;
    let mut current_commit: usize = 0;
    let mut peak_commit: usize = 0;
    let mut page_faults: usize = 0;

    // `current_rss` is deliberately not requested. On Linux `mi_process_info`
    // never assigns it: it pre-fills the field as `current_commit` and the Linux
    // backend overwrites only `peak_rss`. Reading it would produce a second
    // "RSS" that permanently disagrees with the `/proc/self/status` figure
    // beside it — see `fah_model::AllocatorStats`.
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
            &mut peak_rss,
            &mut current_commit,
            &mut peak_commit,
            &mut page_faults,
        );
    }

    Some(AllocatorStats {
        current_commit: current_commit as u64,
        peak_commit: peak_commit as u64,
        peak_rss: peak_rss as u64,
        page_faults: page_faults as u64,
        minor_page_faults: minor_page_faults(),
    })
}

/// Minor page faults since start, or 0 where `getrusage` does not exist.
///
/// A second syscall rather than a field of [`stats`]'s call, because
/// `mi_process_info` reports only *major* faults — which are structurally
/// near-zero here, nothing FAH touches being demand-paged from disk. The fault
/// count that actually moves, and the only way to see an over-aggressive
/// `MIMALLOC_PURGE_DELAY` charging a fault for every page it hands back and
/// immediately reclaims, is the minor one, and it has to be read directly.
///
/// Both counts come from the same `rusage` in the kernel, so the two fields
/// agree in origin even though they arrive by different routes. Two syscalls at
/// a 10 s poll is not a cost worth designing around.
#[cfg(unix)]
fn minor_page_faults() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();

    // SAFETY: `getrusage` writes a fully initialised `rusage` through the
    // pointer whenever it returns 0, and the pointer is to a live, uniquely
    // borrowed, correctly sized and aligned allocation. The return value is
    // checked before `assume_init`, so a failed call never reads uninitialised
    // memory. `RUSAGE_SELF` needs no capabilities and the call cannot block.
    unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return 0;
        }
        usage.assume_init().ru_minflt as u64
    }
}

#[cfg(not(unix))]
fn minor_page_faults() -> u64 {
    0
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
