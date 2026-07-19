//! Bounded top-N counter (CONTEXT.md: Statistics — "top domains"). A cap on
//! tracked keys keeps memory fixed regardless of how many distinct domains a
//! ruleset or upstream sees (hard rule 4); once full, a brand-new key evicts
//! whichever tracked key currently has the lowest count — an approximation
//! (space-saving style), not an exact top-N, but the bound is what matters.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::bucket::{epoch_hour, is_fresh, BUCKET_COUNT};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BoundedCounter {
    counts: HashMap<Arc<str>, u64>,
    capacity: usize,
}

impl BoundedCounter {
    pub fn new(capacity: usize) -> Self {
        Self {
            counts: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// Existing keys are looked up by `&str` (no allocation); only a
    /// genuinely new key allocates, once, via `Arc::from`.
    pub fn record(&mut self, key: &str) {
        if let Some(count) = self.counts.get_mut(key) {
            *count += 1;
            return;
        }
        // `while`, not `if`: a snapshot written by a build with a larger
        // capacity may deserialize over-cap — converge back down instead of
        // hovering above the bound forever.
        while self.counts.len() >= self.capacity {
            match self
                .counts
                .iter()
                .min_by_key(|(_, count)| **count)
                .map(|(key, _)| key.clone())
            {
                Some(min_key) => {
                    self.counts.remove(&min_key);
                }
                None => break,
            }
        }
        self.counts.insert(Arc::from(key), 1);
    }

    /// Only [`HourlyTopN::top`]'s merged view is read in production; the
    /// per-counter sort survives for its direct tests.
    #[cfg(test)]
    pub fn top(&self, n: usize) -> Vec<(Arc<str>, u64)> {
        let mut items: Vec<_> = self.counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(n);
        items
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, u64)> {
        self.counts.iter().map(|(key, count)| (key, *count))
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.counts.len()
    }
}

/// Cap on distinct domains tracked *per hour slot* — worst case
/// `24 × HOURLY_TRACKED_DOMAINS` keys per [`HourlyTopN`], fixed regardless
/// of how many unique domains are queried (hard rule 4).
const HOURLY_TRACKED_DOMAINS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TopSlot {
    hour_epoch: Option<u64>,
    counter: BoundedCounter,
}

impl Default for TopSlot {
    fn default() -> Self {
        Self {
            hour_epoch: None,
            counter: BoundedCounter::new(HOURLY_TRACKED_DOMAINS),
        }
    }
}

/// Rolling-24h top-N: one [`BoundedCounter`] per hour slot, reset when its
/// hour rolls around (same scheme as [`crate::bucket::HourlyBuckets`]), so
/// `top` reflects the last 24h — matching the `window: "24h"` contract of
/// API.md `GET /api/v1/stats` — instead of accumulating since boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HourlyTopN {
    slots: [TopSlot; BUCKET_COUNT as usize],
}

impl Default for HourlyTopN {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| TopSlot::default()),
        }
    }
}

impl HourlyTopN {
    pub fn record(&mut self, key: &str, at: SystemTime) {
        let hour = epoch_hour(at);
        let slot = &mut self.slots[(hour % BUCKET_COUNT) as usize];
        if slot.hour_epoch != Some(hour) {
            slot.hour_epoch = Some(hour);
            slot.counter = BoundedCounter::new(HOURLY_TRACKED_DOMAINS);
        }
        slot.counter.record(key);
    }

    /// Merges the fresh (≤24h-old) slots and returns the `n` highest totals.
    /// Read path only (the stats API), so the merge allocation is fine.
    pub fn top(&self, n: usize, now: SystemTime) -> Vec<(Arc<str>, u64)> {
        let current_hour = epoch_hour(now);
        let mut merged: HashMap<Arc<str>, u64> = HashMap::new();
        for slot in &self.slots {
            if !is_fresh(slot.hour_epoch, current_hour) {
                continue;
            }
            for (key, count) in slot.counter.iter() {
                *merged.entry(Arc::clone(key)).or_insert(0) += count;
            }
        }
        let mut items: Vec<_> = merged.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(n);
        items
    }

    #[cfg(test)]
    pub(crate) fn tracked_keys(&self) -> usize {
        self.slots.iter().map(|slot| slot.counter.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_keys_accumulate() {
        let mut counter = BoundedCounter::new(10);
        counter.record("ads.example.com");
        counter.record("ads.example.com");
        counter.record("tracker.example.net");
        assert_eq!(counter.top(10)[0], (Arc::from("ads.example.com"), 2));
    }

    #[test]
    fn top_is_sorted_descending_by_count() {
        let mut counter = BoundedCounter::new(10);
        for _ in 0..3 {
            counter.record("a.example.com");
        }
        counter.record("b.example.com");
        counter.record("b.example.com");
        counter.record("c.example.com");
        let top = counter.top(2);
        assert_eq!(top[0].0.as_ref(), "a.example.com");
        assert_eq!(top[1].0.as_ref(), "b.example.com");
    }

    #[test]
    fn capacity_bounds_memory_regardless_of_unique_key_count() {
        let mut counter = BoundedCounter::new(50);
        for i in 0..10_000 {
            counter.record(&format!("domain{i}.example.com"));
        }
        assert!(counter.len() <= 50);
    }

    #[test]
    fn hourly_top_excludes_slots_older_than_24h() {
        use std::time::{Duration, UNIX_EPOCH};
        let hours = |n: u64| UNIX_EPOCH + Duration::from_secs(n * 3600);

        let mut top_n = HourlyTopN::default();
        top_n.record("old.example.com", hours(10));
        top_n.record("fresh.example.com", hours(20));

        let top = top_n.top(10, hours(10 + 24));
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0.as_ref(), "fresh.example.com");
    }

    #[test]
    fn hourly_top_merges_counts_across_hours() {
        use std::time::{Duration, UNIX_EPOCH};
        let hours = |n: u64| UNIX_EPOCH + Duration::from_secs(n * 3600);

        let mut top_n = HourlyTopN::default();
        top_n.record("ads.example.com", hours(10));
        top_n.record("ads.example.com", hours(11));
        top_n.record("other.example.com", hours(11));

        let top = top_n.top(10, hours(11));
        assert_eq!(top[0], (Arc::from("ads.example.com"), 2));
    }

    #[test]
    fn hourly_slot_reused_a_day_later_resets() {
        use std::time::{Duration, UNIX_EPOCH};
        let hours = |n: u64| UNIX_EPOCH + Duration::from_secs(n * 3600);

        let mut top_n = HourlyTopN::default();
        top_n.record("yesterday.example.com", hours(10));
        top_n.record("today.example.com", hours(10 + 24)); // same slot index

        let top = top_n.top(10, hours(10 + 24));
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0.as_ref(), "today.example.com");
    }

    #[test]
    fn hourly_top_stays_bounded_under_many_unique_keys() {
        let now = SystemTime::now();
        let mut top_n = HourlyTopN::default();
        for i in 0..50_000 {
            top_n.record(&format!("domain{i}.example.com"), now);
        }
        assert!(top_n.tracked_keys() <= 24 * HOURLY_TRACKED_DOMAINS);
    }

    #[test]
    fn eviction_prefers_the_lowest_count() {
        let mut counter = BoundedCounter::new(2);
        counter.record("popular.example.com");
        counter.record("popular.example.com");
        counter.record("rare.example.com"); // count 1, the eviction target
        counter.record("newcomer.example.com"); // forces an eviction

        let top: HashMap<_, _> = counter.top(10).into_iter().collect();
        assert!(top.contains_key(&Arc::<str>::from("popular.example.com")));
        assert!(top.contains_key(&Arc::<str>::from("newcomer.example.com")));
        assert!(!top.contains_key(&Arc::<str>::from("rare.example.com")));
    }
}
