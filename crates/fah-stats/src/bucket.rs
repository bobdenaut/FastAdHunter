//! Fixed-size rolling 24h counters (CONTEXT.md: Statistics — "rolling time
//! buckets"). One hour per slot, indexed by hour-of-epoch modulo 24: memory
//! never grows with query volume or uptime (hard rule 4, bounded everything).

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fah_model::QueryType;
use serde::{Deserialize, Serialize};

pub(crate) const BUCKET_COUNT: u64 = 24;
const SECONDS_PER_HOUR: u64 = 3600;

/// Fixed, bounded set of DNS-type labels tracked per hour slot (hard rule 4:
/// a per-query counter can't hold an unbounded set of type strings). Order is
/// the `per_type` array index — `qtype_index` must stay in sync. `OTHER` (last)
/// lumps every record type outside the named set.
pub(crate) const QTYPE_LABELS: [&str; 11] = [
    "A", "AAAA", "HTTPS", "MX", "TXT", "PTR", "NS", "SOA", "SRV", "CNAME", "OTHER",
];
pub(crate) const QTYPE_COUNT: usize = QTYPE_LABELS.len();
const QTYPE_OTHER: usize = QTYPE_COUNT - 1;

pub(crate) fn epoch_hour(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / SECONDS_PER_HOUR
}

/// Maps a query type to its fixed [`QTYPE_LABELS`] index. A handful of `==`
/// comparisons for the `Other` case — no allocation, no regex (hard rule 3);
/// the common `A`/`AAAA` path is a trivial enum match.
pub(crate) fn qtype_index(qtype: &QueryType) -> usize {
    match qtype {
        QueryType::A => 0,
        QueryType::Aaaa => 1,
        QueryType::Https => 2,
        QueryType::Mx => 3,
        QueryType::Txt => 4,
        QueryType::Ptr => 5,
        QueryType::Ns => 6,
        QueryType::Soa => 7,
        QueryType::Srv => 8,
        QueryType::Cname => 9,
        QueryType::Svcb
        | QueryType::Caa
        | QueryType::Ds
        | QueryType::Dnskey
        | QueryType::Naptr
        | QueryType::Other(_) => QTYPE_OTHER,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
struct Slot {
    hour_epoch: Option<u64>,
    queries: u64,
    blocked: u64,
    cache_hits: u64,
}

/// Rolling-24h sums across the fresh slots, for the stats snapshot's
/// windowed totals and percentages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BucketTotals {
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
}

/// A 24-slot ring indexed by hour-of-epoch. Writing to an hour that's rolled
/// back around (24h later, same slot) resets that slot first, so stale data
/// never leaks into a new day's totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyBuckets {
    slots: [Slot; BUCKET_COUNT as usize],
}

impl Default for HourlyBuckets {
    fn default() -> Self {
        Self {
            slots: [Slot::default(); BUCKET_COUNT as usize],
        }
    }
}

/// One hour's totals, for the API's `buckets` array (API.md `GET /api/v1/stats`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BucketView {
    pub start: SystemTime,
    pub queries: u64,
    pub blocked: u64,
}

impl HourlyBuckets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, at: SystemTime, blocked: bool, cache_hit: bool) {
        let hour = epoch_hour(at);
        let slot = &mut self.slots[(hour % BUCKET_COUNT) as usize];
        if slot.hour_epoch != Some(hour) {
            *slot = Slot {
                hour_epoch: Some(hour),
                queries: 0,
                blocked: 0,
                cache_hits: 0,
            };
        }
        slot.queries += 1;
        if blocked {
            slot.blocked += 1;
        }
        if cache_hit {
            slot.cache_hits += 1;
        }
    }

    /// Totals over the last 24h ending at `now`; slots older than 24h (or
    /// never written) don't count.
    pub fn totals(&self, now: SystemTime) -> BucketTotals {
        let current_hour = epoch_hour(now);
        self.slots
            .iter()
            .filter(|slot| is_fresh(slot.hour_epoch, current_hour))
            .fold(BucketTotals::default(), |acc, slot| BucketTotals {
                queries: acc.queries + slot.queries,
                blocked: acc.blocked + slot.blocked,
                cache_hits: acc.cache_hits + slot.cache_hits,
            })
    }

    /// Chronological (oldest-first) view of the non-stale buckets.
    pub fn snapshot(&self, now: SystemTime) -> Vec<BucketView> {
        let current_hour = epoch_hour(now);
        let mut views: Vec<BucketView> = self
            .slots
            .iter()
            .filter(|slot| is_fresh(slot.hour_epoch, current_hour))
            .map(|slot| BucketView {
                start: UNIX_EPOCH
                    + Duration::from_secs(slot.hour_epoch.unwrap() * SECONDS_PER_HOUR),
                queries: slot.queries,
                blocked: slot.blocked,
            })
            .collect();
        views.sort_by_key(|view| view.start);
        views
    }

    /// Every *completed* fresh hour (strictly before the hour containing `now`),
    /// oldest-first, for the history writer. The still-filling current hour is
    /// excluded so a rollup row is final once written (idempotent re-appends
    /// are keyed on `hour_epoch`). Off the hot path — the allocation is fine.
    pub(crate) fn completed_hours(&self, now: SystemTime) -> Vec<CompletedHour> {
        let current_hour = epoch_hour(now);
        let mut hours: Vec<CompletedHour> = self
            .slots
            .iter()
            .filter(|slot| is_fresh(slot.hour_epoch, current_hour))
            .filter_map(|slot| {
                let hour_epoch = slot.hour_epoch?;
                (hour_epoch < current_hour).then_some(CompletedHour {
                    hour_epoch,
                    queries: slot.queries,
                    blocked: slot.blocked,
                    cache_hits: slot.cache_hits,
                })
            })
            .collect();
        hours.sort_by_key(|hour| hour.hour_epoch);
        hours
    }
}

/// One completed hour's totals (no per-type — that lives in the separate
/// [`HourlyTypeCounts`] ring, joined by `hour_epoch` off the hot path).
pub(crate) struct CompletedHour {
    pub hour_epoch: u64,
    pub queries: u64,
    pub blocked: u64,
    pub cache_hits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct TypeSlot {
    hour_epoch: Option<u64>,
    per_type: [u64; QTYPE_COUNT],
}

impl Default for TypeSlot {
    fn default() -> Self {
        Self {
            hour_epoch: None,
            per_type: [0; QTYPE_COUNT],
        }
    }
}

/// Rolling-24h per-DNS-type counters, one [`TypeSlot`] per hour (same ring
/// scheme as [`HourlyBuckets`]). Kept separate so the per-client buckets in
/// the client registry — which never need a per-type breakdown — don't each
/// carry an unused `[u64; QTYPE_COUNT]` array (hard rule 4: stay lean).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HourlyTypeCounts {
    slots: [TypeSlot; BUCKET_COUNT as usize],
}

impl Default for HourlyTypeCounts {
    fn default() -> Self {
        Self {
            slots: [TypeSlot::default(); BUCKET_COUNT as usize],
        }
    }
}

impl HourlyTypeCounts {
    /// The sole hot-path cost of per-type tracking: one fixed-array increment
    /// in the already-hashed slot (`type_index` from [`qtype_index`]).
    pub(crate) fn record(&mut self, at: SystemTime, type_index: usize) {
        let hour = epoch_hour(at);
        let slot = &mut self.slots[(hour % BUCKET_COUNT) as usize];
        if slot.hour_epoch != Some(hour) {
            *slot = TypeSlot {
                hour_epoch: Some(hour),
                per_type: [0; QTYPE_COUNT],
            };
        }
        slot.per_type[type_index] += 1;
    }

    /// The label→count map for one specific hour, if that hour is still in the
    /// ring (the slot hasn't been reused by a later hour). Zero buckets are
    /// omitted. Off the hot path.
    pub(crate) fn per_type_for(&self, hour_epoch: u64) -> BTreeMap<String, u64> {
        let slot = &self.slots[(hour_epoch % BUCKET_COUNT) as usize];
        if slot.hour_epoch != Some(hour_epoch) {
            return BTreeMap::new();
        }
        QTYPE_LABELS
            .iter()
            .zip(slot.per_type.iter())
            .filter(|(_, &count)| count > 0)
            .map(|(&label, &count)| (label.to_string(), count))
            .collect()
    }
}

pub(crate) fn is_fresh(hour_epoch: Option<u64>, current_hour: u64) -> bool {
    hour_epoch.is_some_and(|hour| current_hour.saturating_sub(hour) < BUCKET_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hours(n: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(n * SECONDS_PER_HOUR)
    }

    fn totals(queries: u64, blocked: u64, cache_hits: u64) -> BucketTotals {
        BucketTotals {
            queries,
            blocked,
            cache_hits,
        }
    }

    const A: usize = 0;
    const AAAA: usize = 1;

    #[test]
    fn records_within_the_same_hour_accumulate() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(10), false, true);
        buckets.record(hours(10), true, false);
        assert_eq!(buckets.totals(hours(10)), totals(2, 1, 1));
    }

    #[test]
    fn totals_ignore_slots_older_than_24h() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(10), false, false);
        assert_eq!(buckets.totals(hours(10 + 24)), totals(0, 0, 0));
        assert_eq!(buckets.totals(hours(10 + 23)), totals(1, 0, 0));
    }

    #[test]
    fn same_slot_reused_a_day_later_resets_instead_of_accumulating() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(10), false, true); // slot 10
        buckets.record(hours(10 + 24), true, false); // same slot index, next day
        assert_eq!(buckets.totals(hours(10 + 24)), totals(1, 1, 0));
    }

    #[test]
    fn snapshot_is_chronological_and_excludes_stale_slots() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(5), false, false);
        buckets.record(hours(3), true, false);
        let views = buckets.snapshot(hours(5));
        assert_eq!(views.len(), 2);
        assert!(views[0].start < views[1].start);
    }

    #[test]
    fn memory_is_fixed_regardless_of_write_count() {
        let mut buckets = HourlyBuckets::new();
        for i in 0..100_000u64 {
            buckets.record(hours(i), i % 7 == 0, i % 3 == 0);
        }
        assert_eq!(buckets.slots.len(), BUCKET_COUNT as usize);
    }

    #[test]
    fn qtype_index_maps_named_types_and_lumps_the_rest() {
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::A)], "A");
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::Aaaa)], "AAAA");
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::Https)], "HTTPS");
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::Cname)], "CNAME");
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::Naptr)], "OTHER");
        assert_eq!(QTYPE_LABELS[qtype_index(&QueryType::Other(65534))], "OTHER");
    }

    #[test]
    fn completed_hours_exclude_the_still_filling_current_hour() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(10), false, true);
        buckets.record(hours(10), true, false);
        buckets.record(hours(11), false, false); // current, incomplete hour

        let completed = buckets.completed_hours(hours(11));
        assert_eq!(completed.len(), 1, "hour 11 is not complete yet");
        assert_eq!(completed[0].hour_epoch, 10);
        assert_eq!(completed[0].queries, 2);
        assert_eq!(completed[0].blocked, 1);
        assert_eq!(completed[0].cache_hits, 1);
    }

    #[test]
    fn completed_hours_are_oldest_first() {
        let mut buckets = HourlyBuckets::new();
        buckets.record(hours(5), false, false);
        buckets.record(hours(3), false, false);
        buckets.record(hours(4), false, false);
        let order: Vec<u64> = buckets
            .completed_hours(hours(6))
            .iter()
            .map(|h| h.hour_epoch)
            .collect();
        assert_eq!(order, vec![3, 4, 5]);
    }

    #[test]
    fn type_counts_track_per_type_and_forget_reused_hours() {
        let mut counts = HourlyTypeCounts::default();
        counts.record(hours(10), A);
        counts.record(hours(10), A);
        counts.record(hours(10), AAAA);

        let per_type = counts.per_type_for(10);
        assert_eq!(per_type.get("A"), Some(&2));
        assert_eq!(per_type.get("AAAA"), Some(&1));
        assert_eq!(per_type.get("MX"), None, "zero buckets are omitted");

        // The slot for hour 10 is reused 24h later — the old hour is gone.
        counts.record(hours(10 + 24), A);
        assert!(counts.per_type_for(10).is_empty());
        assert_eq!(counts.per_type_for(10 + 24).get("A"), Some(&1));
    }
}
