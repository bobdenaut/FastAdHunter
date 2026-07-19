//! Fixed-size rolling 24h counters (CONTEXT.md: Statistics — "rolling time
//! buckets"). One hour per slot, indexed by hour-of-epoch modulo 24: memory
//! never grows with query volume or uptime (hard rule 4, bounded everything).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub(crate) const BUCKET_COUNT: u64 = 24;
const SECONDS_PER_HOUR: u64 = 3600;

pub(crate) fn epoch_hour(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / SECONDS_PER_HOUR
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
}
