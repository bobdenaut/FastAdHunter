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
}

impl MemoryBreakdown {
    /// Everything attributed to a named, bounded component.
    pub fn accounted(&self) -> u64 {
        self.ruleset + self.cache + self.stats.total()
    }

    /// `RSS − Σ(components)`: binary text and data pages, thread stacks, the
    /// tokio runtime, and allocator memory musl has not returned to the OS.
    /// Legitimately non-zero — it is the *trend* that matters.
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
}
