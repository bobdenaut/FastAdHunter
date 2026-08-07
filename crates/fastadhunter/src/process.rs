//! Kernel readings about this process, from `getrusage`.
//!
//! Separate from [`crate::allocator`] because the two answer to different
//! contracts: these describe the process and survive replacing the global
//! allocator, which is what lets `GET /api/v1/telemetry` publish them as a
//! stable surface. `peak_rss` and the major-fault count used to arrive through
//! `mi_process_info`, which tied them to mimalloc being linked in.

use fah_model::ProcessStats;

/// Peak RSS and both fault counters, or `None` where `getrusage` is
/// unavailable. One syscall, so the three always share an instant.
#[cfg(unix)]
pub fn stats() -> Option<ProcessStats> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();

    // SAFETY: `getrusage` writes a fully initialised `rusage` through the
    // pointer whenever it returns 0, and the pointer is to a live, uniquely
    // borrowed, correctly sized and aligned allocation. The return value is
    // checked before `assume_init`, so a failed call never reads uninitialised
    // memory. `RUSAGE_SELF` needs no capabilities and the call cannot block.
    let usage = unsafe {
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return None;
        }
        usage.assume_init()
    };

    Some(ProcessStats {
        // `ru_maxrss` is KiB on Linux; the API publishes bytes.
        peak_rss: (usage.ru_maxrss as u64).saturating_mul(1024),
        major_page_faults: usage.ru_majflt as u64,
        minor_page_faults: usage.ru_minflt as u64,
    })
}

#[cfg(not(unix))]
pub fn stats() -> Option<ProcessStats> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Not a numeric assertion — the figures are whatever the test binary has
    /// done. What this pins is that the call succeeds and that `peak_rss` is
    /// scaled into bytes rather than left in `ru_maxrss`'s KiB.
    #[test]
    fn the_kernel_figures_are_readable_and_in_bytes() {
        let stats = stats().expect("getrusage(RUSAGE_SELF) cannot fail for our own process");
        assert!(
            stats.peak_rss > 1024 * 1024,
            "a running test binary peaks above 1 MiB; got {} bytes — is ru_maxrss still in KiB?",
            stats.peak_rss
        );
        assert!(stats.minor_page_faults > 0, "any running process faults");
    }
}
