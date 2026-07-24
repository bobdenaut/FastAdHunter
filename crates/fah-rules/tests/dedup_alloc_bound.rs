//! Hard memory bound for the transient dedup index (p1.5-05 review, M1).
//!
//! A process-global counting allocator makes the *actual bytes* allocated by
//! `MatcherBuilder::with_capacity` observable, so this is a real red/green: it
//! passes with the `MAX_PREALLOC_RULES` clamp in place and **fails** without it.
//! The unit tests in `matcher.rs` assert the clamp structurally (`dedup.len()`);
//! this asserts the thing the clamp exists to protect — that a hostile list's
//! inflated rule ceiling cannot translate into a multi-hundred-MB allocation.
//!
//! Kept in its own test binary with exactly one test so no other test allocates
//! on a second thread while the global peak is being measured.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use fah_rules::MatcherBuilder;

/// Bytes currently allocated through the global allocator, and the high-water
/// mark reached. Relaxed is fine: a single test thread drives the measurement.
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Delegates every allocation to the system allocator and records the size, so
/// the test can read the live total and the peak. Not for production — a test
/// instrument only.
struct Counting;

// SAFETY: every method forwards to `System`, which is a sound `GlobalAlloc`, and
// the byte counters are pure side effects that never touch the returned memory
// or change the layout, so the allocator contract is preserved unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is a valid, non-zero layout per the trait contract;
        // forwarded verbatim to the system allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr`/`layout` come straight from a prior `alloc` call with
        // the same layout, as the trait requires; forwarded verbatim.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn with_capacity_bounds_the_transient_allocation_under_an_adversarial_ceiling() {
    // The rule *ceiling* a 64 MiB (MAX_LIST_BYTES) hostile list of "x " tokens
    // yields via `parser::rule_upper_bound`: one token per two bytes.
    const ADVERSARIAL_CEILING: usize = 64 * 1024 * 1024 / 2; // ~33.5M "rules"

    // Un-clamped, `with_capacity` would size the index at
    // dedup_slots(33.5M) = 67M u32 = ~256 MiB. The clamp caps it at
    // dedup_slots(4M) = 8M u32 = ~30.5 MiB. The 40 MiB limit sits cleanly
    // between the two, so this test is green with the clamp and red without.
    const LIMIT: usize = 40 * 1024 * 1024;

    let before = CURRENT.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);

    let builder = MatcherBuilder::with_capacity(ADVERSARIAL_CEILING);

    let resident = CURRENT.load(Ordering::Relaxed).saturating_sub(before);
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);

    // Keep the builder alive across the measurement so its allocation is not
    // freed before we read the counters.
    assert!(builder.duplicates_removed() == 0);

    assert!(
        peak <= LIMIT,
        "with_capacity({ADVERSARIAL_CEILING}) peaked at {peak} bytes \
         (resident {resident}); the clamp must hold it under {LIMIT} — \
         without MAX_PREALLOC_RULES this is ~256 MiB"
    );
}
