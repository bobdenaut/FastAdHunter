//! Memory breakdown (p2-07): where the process's resident memory is.
//!
//! Lives here, beside [`crate::PerfSample`], because four crates need the same
//! shape and none of them may import another: `fah-stats` fills its own slice,
//! `fah-dns` and `fah-rules` report their own totals, the binary assembles
//! them, and `fah-metrics` and `fah-api` both publish the result
//! (ARCHITECTURE.md §Dependency Layering — L3 siblings never import each
//! other, so a shared shape belongs at L1).
//!
//! Data plus trivial accessors only, per hard rule 2. [`MemoryBreakdown::accounted`]
//! is a sum and [`MemoryBreakdown::residual`] a subtraction; defining them once
//! here is what stops a new component from being added to the metrics path and
//! silently forgotten on the API path.

/// `fah-stats`'s slice of the breakdown: its bounded in-RAM structures.
/// Excludes anything on `/data`, which `retention_max_mb` bounds separately.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsHeap {
    /// 24 h rolling buckets, per-type counts and the bounded top-N counters.
    pub aggregates: u64,
    /// Per-client records and names, capped with LRU eviction.
    pub clients: u64,
    /// In-RAM query ring serving `GET /api/v1/queries`.
    pub ring: u64,
    /// Entries awaiting the next flush to `/data`.
    pub pending_log: u64,
}

impl StatsHeap {
    pub fn total(&self) -> u64 {
        self.aggregates + self.clients + self.ring + self.pending_log
    }
}

/// What the process allocator reports about itself, plus the two kernel figures
/// that arrive from the same call.
///
/// Read through the binary's allocator module (see `crates/fastadhunter/src/allocator.rs`) and carried as plain
/// data from here on, so no crate but that one module knows which allocator is
/// in use. The figures are only meaningful while that allocator is the global
/// one — under a different one they read near zero.
///
/// **`current_rss` is deliberately absent.** mimalloc's `mi_process_info`
/// exposes such a field, but on Linux it is never assigned: the function
/// pre-fills it as `current_commit` and the Linux backend overwrites only
/// `peak_rss`. Serving it would put two fields both meaning "RSS" —
/// [`MemoryBreakdown::rss`], read from `/proc/self/status`, and one permanently
/// equal to `current_commit` — in the same response, permanently disagreeing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocatorStats {
    /// Bytes the allocator has committed, as *it* accounts for them — not a
    /// kernel reading.
    ///
    /// **Treat this as a high-water mark, not a live figure.** Measured on the
    /// RB5009 under mimalloc v3 (0.2.7, `docs/code-review/`
    /// `0.2.7-router-memory-and-throughput.md` §5.2): the counter is monotone
    /// non-decreasing, because v3 does not decrement it when a purge returns
    /// pages to the OS. `current_commit == peak_commit` at every reading taken
    /// so far, across a 32M-query run, is the evidence.
    ///
    /// The practical consequence is that **this can and does exceed
    /// [`MemoryBreakdown::rss`] by several times** — 318 MB against 70 MB RSS
    /// was the measured state. That gap is memory that was committed, touched,
    /// and since reclaimed by the kernel; it is *not* memory being held. RSS is
    /// the authority on footprint. Do not subtract anything from this field and
    /// present the result as retention — see the note on
    /// [`MemoryBreakdown::residual`].
    pub current_commit: u64,
    /// High-water mark of `current_commit`, and **process-lifetime monotonic**.
    ///
    /// Kept despite `current_commit` behaving the same way, because that
    /// equality is an observation about one allocator version rather than a
    /// guarantee: should a future allocator (or mimalloc v2, or a build where
    /// purge accounting is fixed) decrement `current_commit`, the two fields
    /// diverge and the pair immediately becomes informative again. Their being
    /// equal is itself the signal documented above.
    pub peak_commit: u64,
    /// Peak RSS from `getrusage` — a real kernel high-water mark, unlike
    /// `current_commit`. **Process-lifetime monotonic and never decreasing**,
    /// so it answers "did we ever exceed the budget" and must not be charted as
    /// a trend.
    ///
    /// Its value is that a spike between two samples cannot be missed, which
    /// polled RSS alone cannot promise — and that has already paid off: the
    /// 150.7 MiB startup-compile peak on 0.2.7 fell entirely between two
    /// 2-minute RSS samples and was visible only here.
    pub peak_rss: u64,
    /// Major (disk-backed) page faults since start. Process-lifetime cumulative.
    ///
    /// Structurally near-zero for this workload — nothing FAH touches is
    /// demand-paged from disk — so a non-zero value means the host is under
    /// genuine memory pressure. `minor_page_faults` is the one that moves.
    pub page_faults: u64,
    /// Minor (no disk I/O) page faults since start. Process-lifetime cumulative.
    ///
    /// **This is the purge-thrash detector.** Returning pages with
    /// `MADV_DONTNEED` and then reallocating costs one minor fault per page
    /// faulted back in, and `MIMALLOC_PURGE_DELAY` tunes exactly that
    /// trade-off. Without this field an over-aggressive purge setting is
    /// invisible: RSS looks healthy while the process pays a syscall and fault
    /// on memory it is about to reuse. Read it as a rate against query volume,
    /// not as an absolute.
    ///
    /// Zero off Unix, where `getrusage` does not exist.
    pub minor_page_faults: u64,
}

/// One consistent read of where memory is.
///
/// **Every field must be sampled at the same instant.** If `rss` is read at *t*
/// and the components at *t+ε*, the skew lands in [`Self::residual`] as noise —
/// and the residual is the whole point: a leak shows as *residual growing while
/// the named components stay flat*, because the growth you legitimately expect
/// has been subtracted out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryBreakdown {
    /// Compiled ruleset — `Matcher::heap_bytes()`.
    pub ruleset: u64,
    /// DNS cache — the total it already tracks for eviction decisions.
    pub cache: u64,
    pub stats: StatsHeap,
    /// Process resident set size, or `None` where it cannot be read (no
    /// `/proc/self/status` off Linux). The residual is then not computable.
    pub rss: Option<u64>,
    /// Allocator's own view, or `None` where unavailable. Optional for the same
    /// reason as `rss`: a figure that cannot be read is served as absent rather
    /// than as zero, which would chart as a real measurement.
    pub allocator: Option<AllocatorStats>,
}

impl MemoryBreakdown {
    /// Everything attributed to a named, bounded component.
    pub fn accounted(&self) -> u64 {
        self.ruleset + self.cache + self.stats.total()
    }

    /// `RSS − Σ(components)`: binary text and data pages, thread stacks, the
    /// tokio runtime, and memory the allocator holds but has not returned to
    /// the OS. Legitimately non-zero — it is the *trend* that matters.
    ///
    /// **This is the only leak signal, and the allocator counters cannot refine
    /// it.** A derived `current_commit − accounted()` was served as
    /// `allocator_retained_bytes` in 0.2.7 and has been removed: with
    /// `current_commit` monotone (see [`AllocatorStats::current_commit`]) the
    /// subtraction produces a number that grows without bound and exceeded RSS
    /// by 3.7× on the device — 260 MiB of claimed "retention" in a process with
    /// 70 MiB resident. Nothing resident can exceed RSS, so the figure was not
    /// merely imprecise but impossible, and a wrong metric is worse than a
    /// missing one. Judge retention from `residual` against its own history.
    ///
    /// **Saturating on purpose.** A negative residual cannot happen in reality;
    /// it means a component double-counts or counts something not resident.
    /// Wrapping would turn that bug into an absurd number, so this floors at
    /// zero and [`Self::over_accounted`] reports the bug separately rather than
    /// letting the floor hide it.
    pub fn residual(&self) -> Option<u64> {
        self.rss.map(|rss| rss.saturating_sub(self.accounted()))
    }

    /// True when components claim more than RSS — always an accounting bug,
    /// never a real state.
    pub fn over_accounted(&self) -> bool {
        self.rss.is_some_and(|rss| self.accounted() > rss)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breakdown(rss: Option<u64>) -> MemoryBreakdown {
        MemoryBreakdown {
            ruleset: 100,
            cache: 10,
            stats: StatsHeap {
                aggregates: 5,
                clients: 4,
                ring: 3,
                pending_log: 2,
            },
            rss,
            allocator: None,
        }
    }

    #[test]
    fn residual_is_rss_minus_every_component() {
        let memory = breakdown(Some(200));
        assert_eq!(memory.accounted(), 124);
        assert_eq!(memory.residual(), Some(76));
        assert!(!memory.over_accounted());
    }

    #[test]
    fn over_accounting_floors_at_zero_and_stays_visible() {
        let memory = breakdown(Some(10));
        assert_eq!(
            memory.residual(),
            Some(0),
            "a negative residual must never wrap to a huge unsigned value"
        );
        assert!(memory.over_accounted(), "the accounting bug must stay loud");
    }

    #[test]
    fn without_rss_there_is_no_residual() {
        let memory = breakdown(None);
        assert_eq!(memory.residual(), None);
        assert!(!memory.over_accounted());
    }

    #[test]
    fn committed_far_above_rss_is_a_normal_state_not_an_accounting_bug() {
        // The shape measured on the RB5009: 318 MB committed against 70 MB
        // resident, because mimalloc v3 does not decrement its commit counter
        // when a purge hands pages back. `over_accounted` must stay false — it
        // reports *components* exceeding RSS, which is a real bug, and must not
        // be tripped by the allocator counter, which is not.
        let memory = MemoryBreakdown {
            allocator: Some(AllocatorStats {
                current_commit: 318_046_208,
                peak_commit: 318_046_208,
                peak_rss: 157_990_912,
                page_faults: 0,
                minor_page_faults: 4_211_337,
            }),
            ..breakdown(Some(73_707_520))
        };

        assert!(!memory.over_accounted());
        assert_eq!(memory.residual(), Some(73_707_520 - 124));

        // Guards the deleted `allocator_retained`: this subtraction is what it
        // served, and the result is larger than RSS — impossible for anything
        // resident, which is why no such field exists any more.
        let would_have_been = memory.allocator.unwrap().current_commit - memory.accounted();
        assert!(
            would_have_been > memory.rss.unwrap(),
            "if this ever stops holding, re-read AllocatorStats::current_commit \
             before reintroducing a derived retention figure"
        );
    }
}
