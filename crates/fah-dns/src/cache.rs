//! Bounded, sharded cache of upstream answers (ARCHITECTURE.md §DNS Pipeline:
//! "cache stores upstream answers only" — the Rule Engine already ran and
//! never touches this module; ADR-0001). Keyed by `(domain, qtype, qclass)`
//! exactly as queried, so a cached reply can be replayed byte-for-byte
//! (minus the TTL, which is recomputed on every read).
//!
//! **Sharding, not a single lock.** `SHARD_COUNT` independently-locked
//! buckets split contention across concurrent lookups/stores instead of one
//! global lock (ARCHITECTURE.md §Runtime Model: "sharding for the cache").
//! 16 is fixed rather than derived from core count or `max_entries`: the
//! RB5009 has 4 cores and Phase 1 runs a handful of listener tasks, so 16
//! already keeps per-shard contention negligible, and each critical section
//! is a handful of `HashMap` operations — nanoseconds, not the microseconds
//! PERFORMANCE.md's <1ms cache-hit budget allows for.
//!
//! **Eviction: O(1) FIFO by insertion order, not LRU.** True LRU requires
//! mutating access-order state on every *read*, which turns a cache hit into
//! a write under lock; a bounded network cache doesn't need that precision
//! because TTL already retires most entries before capacity pressure ever
//! triggers eviction. Each shard keeps an insertion-order queue; evicting
//! pops its head. An earlier version scanned the shard for dead entries
//! first and fell back to a `min_by_key` over `inserted_at` — two
//! O(shard-len) walks plus a key clone on every insert into a full shard,
//! measured at ~25 of the 27.7 µs a steady-state forwarded query cost
//! (p1-10 review). The dead-first preference went with the scans: dead
//! weight now leaves when it ages to the queue head, when its slot's TTL
//! turnover replaces it, or via an admin clean — a marginal hit-rate trade
//! for removing the dominant cost on the forward path.

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fah_config::DnsCacheConfig;
use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::{DNSClass, RData, Record, RecordType};
use tokio::time::Instant;

/// Independently-locked shard count (power of two — shard selection masks
/// instead of computing a modulo).
const SHARD_COUNT: usize = 16;

/// RFC 8767 §4 minimal serve-stale: an expired entry may still answer
/// queries for up to this long past its original TTL when upstreams are
/// unreachable. Not config-exposed (CONFIGURATION.md's `[dns.cache]` has no
/// stale-window knob) — the RFC's own suggested ceiling.
const MAX_STALE: Duration = Duration::from_secs(24 * 60 * 60);

/// TTL handed back on a stale-served answer. Short on purpose: it tells the
/// asking resolver (and our own next lookup) to try again soon rather than
/// pin the stale data client-side for a normal TTL's worth of time.
pub(crate) const STALE_SERVE_TTL: u32 = 30;

/// Built once per query via [`DnsCache::key`] and reused across the
/// lookup → store → stale-lookup sequence, so the resolve path pays for the
/// key exactly once.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    /// ASCII-lowercased, trailing dot included — matches `qtype::domain_of`'s
    /// wire-format output, case-folded by the pipeline before it gets here.
    domain: Box<str>,
    qtype: RecordType,
    qclass: DNSClass,
}

/// A cached reply, independent of how much of its TTL remains — the answer
/// records and response code as the upstream sent them, plus the authority
/// records for negative answers (RFC 2308: a replayed NXDOMAIN/NODATA keeps
/// its SOA so downstream cachers keep their TTL signal).
pub(crate) struct CachedAnswer {
    pub records: Vec<Record>,
    pub authorities: Vec<Record>,
    pub response_code: ResponseCode,
}

struct Entry {
    /// `Arc` so a hit hands back a refcount bump instead of deep-cloning the
    /// record set under the shard lock.
    answer: Arc<CachedAnswer>,
    inserted_at: Instant,
    ttl: Duration,
    /// Ties the entry to its [`Shard::queue`] node. A refresh re-inserts the
    /// key under a new seq, turning the old node into a ghost. A per-shard
    /// counter rather than `inserted_at` because paused-clock tests (and, in
    /// principle, a coarse clock) can hand two inserts the same `Instant`.
    seq: u64,
}

impl Entry {
    fn expires_at(&self) -> Instant {
        self.inserted_at + self.ttl
    }

    fn stale_deadline(&self) -> Instant {
        self.expires_at() + MAX_STALE
    }

    /// Which lifetime stage the entry is in at `now` (CONTEXT.md §Cache):
    /// fresh answers queries directly; stale answers only after a failed
    /// forward (RFC 8767, when `serve_stale` is on); expired is dead weight
    /// awaiting eviction or an admin clean.
    fn state(&self, now: Instant, serve_stale: bool) -> EntryState {
        if now < self.expires_at() {
            EntryState::Fresh
        } else if serve_stale && now < self.stale_deadline() {
            EntryState::Stale
        } else {
            EntryState::Expired
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    Fresh,
    Stale,
    Expired,
}

/// Point-in-time cache usage for the admin plane (`GET /api/v1/cache`,
/// surfaced through the binary — this crate never sees the API). Counter
/// fields are process-lifetime totals; the entry-state fields are a walk of
/// the shards, taken one shard lock at a time so a scrape never stalls the
/// resolve path globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: u64,
    pub capacity: u64,
    pub fresh: u64,
    pub stale: u64,
    pub expired: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// Coarse heap estimate: the hash-table slabs (every bucket, occupied or
    /// not — [`table_bytes`]), each entry's own heap ([`entry_heap_bytes`]),
    /// and the eviction queues ([`queue_bytes`]), all rounded to allocator
    /// granularity. Built to make the gap to RSS explainable, not to audit
    /// the allocator.
    pub estimated_bytes: u64,
}

/// Outcome of one admin clean (`POST /api/v1/cache/clean`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheClean {
    pub removed_expired: u64,
    pub removed_stale: u64,
    pub entries_before: u64,
    pub entries_after: u64,
    /// Coarse estimate of the removed entries' own heap
    /// ([`entry_heap_bytes`]); the table slab is not freed by a clean and is
    /// deliberately not counted here.
    pub freed_bytes: u64,
    pub duration: Duration,
}

/// Flat per-record allowance for heap the record owns but does not expose:
/// the `Name`'s label storage and the common inline rdata (A/AAAA). hickory
/// has no heap-usage accessor, and matching every `RData` variant for exact
/// numbers would couple this module to hickory's internals for a dashboard
/// figure — the struct sizes around it are exact, this part is a documented
/// estimate.
const RECORD_HEAP_ALLOWANCE: usize = 48;

/// Allocator padding: musl's allocator (the container's) hands out small
/// blocks in 16-byte-granular size classes, so every heap block is rounded
/// up — honest about padding without modeling any particular allocator.
fn alloc_rounded(bytes: usize) -> u64 {
    ((bytes + 15) & !15) as u64
}

/// Heap owned by one entry *outside* the hash-table slab: the key's domain
/// string, the `Arc<CachedAnswer>` block (two refcount words + the struct),
/// the two record `Vec` buffers, and [`RECORD_HEAP_ALLOWANCE`] per record.
/// The inline `(CacheKey, Entry)` pair is deliberately *not* here — it lives
/// in the table slab, which [`DnsCache::stats`] counts per shard via
/// [`table_bytes`].
fn entry_heap_bytes(key: &CacheKey, entry: &Entry) -> u64 {
    let records = entry.answer.records.len();
    let authorities = entry.answer.authorities.len();
    alloc_rounded(key.domain.len())
        + alloc_rounded(2 * std::mem::size_of::<usize>() + std::mem::size_of::<CachedAnswer>())
        + alloc_rounded(records * std::mem::size_of::<Record>())
        + alloc_rounded(authorities * std::mem::size_of::<Record>())
        + ((records + authorities) * RECORD_HEAP_ALLOWANCE) as u64
}

/// One shard's hash-table slab. hashbrown (std's `HashMap`) allocates one
/// control byte plus one `(key, value)` slot per *bucket*, occupied or not,
/// and keeps buckets ≈ `capacity()` × 8/7 (its 7/8 maximum load factor).
/// Counting the slab from capacity rather than length is what lets the
/// estimate explain RSS: 10 000 entries sitting in a table grown to 16 384
/// buckets cost 16 384 slots, not 10 000.
fn table_bytes(map: &HashMap<CacheKey, Entry>) -> u64 {
    let buckets = map.capacity() * 8 / 7;
    alloc_rounded(buckets * (std::mem::size_of::<(CacheKey, Entry)>() + 1))
}

/// One shard's eviction-queue heap: the ring buffer's slab (`VecDeque`
/// allocates its capacity, not its length) plus each node's cloned domain
/// string. Bounded by [`Shard::compact`] at 2× the shard's entry bound.
fn queue_bytes(queue: &VecDeque<(CacheKey, u64)>) -> u64 {
    let slab = alloc_rounded(queue.capacity() * std::mem::size_of::<(CacheKey, u64)>());
    slab + queue
        .iter()
        .map(|(key, _)| alloc_rounded(key.domain.len()))
        .sum::<u64>()
}

pub(crate) enum Lookup {
    /// Not yet expired. Carries the TTL still remaining (original TTL minus
    /// elapsed time) so the reply doesn't overstate freshness.
    Fresh(Arc<CachedAnswer>, u32),
    /// Expired but within the RFC 8767 stale window — only meant to be used
    /// after a forward attempt has actually failed.
    Stale(Arc<CachedAnswer>),
    Miss,
}

struct Shard {
    map: HashMap<CacheKey, Entry>,
    /// Insertion-order queue driving O(1) eviction. Every insert pushes one
    /// node; a node whose seq no longer matches the map's entry (the key was
    /// refreshed, cleaned, or evicted) is a ghost — skipped at pop time and
    /// swept by [`Shard::compact`] once the queue outgrows 2× capacity.
    queue: VecDeque<(CacheKey, u64)>,
    /// Seq handed to the next insert. Per shard, monotonic, never reused.
    next_seq: u64,
    capacity: usize,
}

impl Shard {
    /// Removes the oldest-inserted live entry — O(1) amortized: each popped
    /// ghost was pushed by exactly one insert, so ghost-skipping work is
    /// prepaid. Replaces the previous dead-first scan (`find` + `min_by_key`
    /// over the whole shard, ~25 µs per insert into a full cache).
    fn evict_oldest(&mut self) {
        while let Some((key, seq)) = self.queue.pop_front() {
            if self.map.get(&key).is_some_and(|entry| entry.seq == seq) {
                self.map.remove(&key);
                return;
            }
        }
        // Every live entry has exactly one matching queue node, so an empty
        // queue means an empty map. If that invariant ever breaks, stay
        // bounded anyway: drop an arbitrary entry rather than grow.
        debug_assert!(self.map.is_empty(), "live entries must have queue nodes");
        if let Some(key) = self.map.keys().next().cloned() {
            self.map.remove(&key);
        }
    }

    /// Sweeps ghost nodes once the queue holds more than twice the shard's
    /// entry bound. Amortized O(1): refilling to the threshold takes at
    /// least `capacity` pushes, each O(1).
    fn compact(&mut self) {
        if self.queue.len() > self.capacity * 2 {
            let map = &self.map;
            self.queue
                .retain(|(key, seq)| map.get(key).is_some_and(|entry| entry.seq == *seq));
        }
    }
}

pub(crate) struct DnsCache {
    shards: Vec<Mutex<Shard>>,
    hash_builder: RandomState,
    min_ttl: u32,
    max_ttl: u32,
    negative_ttl_max: u32,
    serve_stale: bool,
    /// Resolve-path outcomes ([`DnsCache::note_lookup`]) and capacity
    /// evictions, for [`DnsCache::stats`]. Relaxed atomics — admin-plane
    /// reporting, not synchronization.
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}

impl DnsCache {
    pub(crate) fn new(config: &DnsCacheConfig) -> Self {
        // At least 1 per shard: a misconfigured `max_entries` smaller than
        // `SHARD_COUNT` still yields a working (just very small) cache
        // instead of a shard that can never hold anything.
        let capacity = ((config.max_entries as usize) / SHARD_COUNT).max(1);
        let shards = (0..SHARD_COUNT)
            .map(|_| {
                Mutex::new(Shard {
                    map: HashMap::new(),
                    queue: VecDeque::new(),
                    next_seq: 0,
                    capacity,
                })
            })
            .collect();
        Self {
            shards,
            hash_builder: RandomState::new(),
            min_ttl: config.min_ttl_seconds,
            // Never let a misconfigured max < min make the clamp invalid;
            // std's `clamp` panics on min > max, so this is resolved here
            // once rather than defensively at every clamp call site.
            max_ttl: config.max_ttl_seconds.max(config.min_ttl_seconds),
            negative_ttl_max: config.negative_ttl_max_seconds,
            serve_stale: config.serve_stale,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Builds the cache key for one query. `domain` must already be
    /// ASCII-lowercased (the pipeline folds it once, right after decode) —
    /// the cache does no case work of its own on the per-query path.
    pub(crate) fn key(&self, domain: &str, qtype: RecordType, qclass: DNSClass) -> CacheKey {
        debug_assert!(
            !domain.bytes().any(|b| b.is_ascii_uppercase()),
            "cache keys must be pre-lowercased by the caller"
        );
        CacheKey {
            domain: domain.into(),
            qtype,
            qclass,
        }
    }

    fn shard_index(&self, key: &CacheKey) -> usize {
        (self.hash_builder.hash_one(key) as usize) & (SHARD_COUNT - 1)
    }

    pub(crate) fn lookup(&self, key: &CacheKey) -> Lookup {
        let shard = &self.shards[self.shard_index(key)];
        let guard = shard.lock().unwrap();
        let Some(entry) = guard.map.get(key) else {
            return Lookup::Miss;
        };
        let now = Instant::now();
        if now < entry.expires_at() {
            let remaining = (entry.expires_at() - now).as_secs() as u32;
            Lookup::Fresh(Arc::clone(&entry.answer), remaining)
        } else if self.serve_stale && now < entry.stale_deadline() {
            Lookup::Stale(Arc::clone(&entry.answer))
        } else {
            Lookup::Miss
        }
    }

    /// Caches an upstream response. Only `NOERROR` (positive, or negative
    /// per RFC 2308 when there are no answers) and `NXDOMAIN` are stored;
    /// anything else (`SERVFAIL`, `REFUSED`, …) is a transient upstream
    /// state, not an answer worth remembering — serve-stale exists for
    /// exactly that situation instead. A truncated (`TC`) reply is never
    /// stored either: its answer section is incomplete by definition, and
    /// its usual shape (NOERROR, zero answers) would otherwise be cached as
    /// a negative entry — turning "answer too big for UDP, retry over TCP"
    /// into a confidently-served NODATA for the negative TTL's duration.
    pub(crate) fn store(&self, key: &CacheKey, response: &Message) {
        if response.metadata.truncation {
            return;
        }
        let code = response.metadata.response_code;
        if !response.answers.is_empty() {
            if code == ResponseCode::NoError {
                let ttl = positive_ttl(&response.answers, self.min_ttl, self.max_ttl);
                self.insert(
                    key,
                    CachedAnswer {
                        records: response.answers.clone(),
                        authorities: Vec::new(),
                        response_code: ResponseCode::NoError,
                    },
                    ttl,
                );
            }
            return;
        }
        if code == ResponseCode::NoError || code == ResponseCode::NXDomain {
            let ttl = negative_ttl(response, self.negative_ttl_max);
            self.insert(
                key,
                CachedAnswer {
                    records: Vec::new(),
                    authorities: response.authorities.clone(),
                    response_code: code,
                },
                ttl,
            );
        }
    }

    fn insert(&self, key: &CacheKey, answer: CachedAnswer, ttl_seconds: u32) {
        let idx = self.shard_index(key);
        let mut guard = self.shards[idx].lock().unwrap();
        let now = Instant::now();
        if !guard.map.contains_key(key) && guard.map.len() >= guard.capacity {
            // A full shard is never empty, so this always removes one.
            guard.evict_oldest();
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
        let seq = guard.next_seq;
        guard.next_seq += 1;
        guard.queue.push_back((key.clone(), seq));
        guard.map.insert(
            key.clone(),
            Entry {
                answer: Arc::new(answer),
                inserted_at: now,
                ttl: Duration::from_secs(u64::from(ttl_seconds)),
                seq,
            },
        );
        guard.compact();
    }

    /// Total entries across every shard. Test-only (asserts the capacity
    /// bound); the admin plane reads [`DnsCache::stats`] instead.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().unwrap().map.len())
            .sum()
    }

    /// Total eviction-queue nodes across every shard, ghosts included.
    /// Test-only: asserts the queue's own bound under refresh churn.
    #[cfg(test)]
    pub(crate) fn queue_len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().unwrap().queue.len())
            .sum()
    }

    /// Counts one resolve-path outcome. Called by the pipeline once per
    /// Allow/Pass query (blocked queries never touch the cache), with the
    /// same hit definition the `QueryEvent` carries — a stale serve is a hit.
    pub(crate) fn note_lookup(&self, hit: bool) {
        let counter = if hit { &self.hits } else { &self.misses };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Usage snapshot for `GET /api/v1/cache`. O(entries) under one shard
    /// lock at a time — admin plane, never on the resolve path.
    pub(crate) fn stats(&self) -> CacheStats {
        let now = Instant::now();
        let mut stats = CacheStats {
            entries: 0,
            capacity: 0,
            fresh: 0,
            stale: 0,
            expired: 0,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            estimated_bytes: 0,
        };
        for shard in &self.shards {
            let guard = shard.lock().unwrap();
            stats.capacity += guard.capacity as u64;
            stats.entries += guard.map.len() as u64;
            stats.estimated_bytes += table_bytes(&guard.map) + queue_bytes(&guard.queue);
            for (key, entry) in &guard.map {
                match entry.state(now, self.serve_stale) {
                    EntryState::Fresh => stats.fresh += 1,
                    EntryState::Stale => stats.stale += 1,
                    EntryState::Expired => stats.expired += 1,
                }
                stats.estimated_bytes += entry_heap_bytes(key, entry);
            }
        }
        stats
    }

    /// Removes expired (dead) entries; with `purge_stale`, stale-window
    /// entries go too. Stale entries are kept by default on purpose — they
    /// are the RFC 8767 insurance an upstream outage is survived on, so
    /// dropping them is an explicit admin choice, not the default clean.
    ///
    /// `freed_bytes` counts only the removed entries' own heap
    /// ([`entry_heap_bytes`]): `retain` never shrinks the table, so the slab
    /// [`table_bytes`] reports stays allocated and keeps showing up in
    /// [`DnsCache::stats`] afterwards.
    pub(crate) fn clean(&self, purge_stale: bool) -> CacheClean {
        let started = std::time::Instant::now();
        let now = Instant::now();
        let mut outcome = CacheClean {
            removed_expired: 0,
            removed_stale: 0,
            entries_before: 0,
            entries_after: 0,
            freed_bytes: 0,
            duration: Duration::ZERO,
        };
        for shard in &self.shards {
            let mut guard = shard.lock().unwrap();
            outcome.entries_before += guard.map.len() as u64;
            guard
                .map
                .retain(|key, entry| match entry.state(now, self.serve_stale) {
                    EntryState::Fresh => true,
                    EntryState::Stale if !purge_stale => true,
                    state => {
                        outcome.freed_bytes += entry_heap_bytes(key, entry);
                        if state == EntryState::Stale {
                            outcome.removed_stale += 1;
                        } else {
                            outcome.removed_expired += 1;
                        }
                        false
                    }
                });
            outcome.entries_after += guard.map.len() as u64;
        }
        outcome.duration = started.elapsed();
        outcome
    }
}

/// The TTL to store a positive answer under: the minimum TTL across its
/// answer records (the standard resolver rule — the whole record set can't
/// outlive its shortest-lived member), clamped to `[min_ttl, max_ttl]`.
fn positive_ttl(records: &[Record], min_ttl: u32, max_ttl: u32) -> u32 {
    let ttl = records.iter().map(|r| r.ttl).min().unwrap_or(max_ttl);
    ttl.max(min_ttl).min(max_ttl)
}

/// RFC 2308 §5: a negative answer's TTL is the SOA record's MINIMUM field
/// (found in the authority section), capped by local policy
/// (`negative_ttl_max`). No SOA present (a resolver that skips sending one)
/// falls back to the cap itself.
fn negative_ttl(response: &Message, negative_ttl_max: u32) -> u32 {
    let soa_minimum = response.authorities.iter().find_map(|r| match &r.data {
        RData::SOA(soa) => Some(soa.minimum),
        _ => None,
    });
    soa_minimum
        .unwrap_or(negative_ttl_max)
        .min(negative_ttl_max)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    use hickory_proto::rr::rdata::{A, SOA};
    use hickory_proto::rr::{Name, RData};

    use super::*;

    fn config(max_entries: u32) -> DnsCacheConfig {
        DnsCacheConfig {
            max_entries,
            min_ttl_seconds: 0,
            max_ttl_seconds: 86400,
            negative_ttl_max_seconds: 60,
            serve_stale: true,
        }
    }

    fn a_key(cache: &DnsCache, domain: &str) -> CacheKey {
        cache.key(domain, RecordType::A, DNSClass::IN)
    }

    fn a_record(name: &str, ttl: u32, addr: Ipv4Addr) -> Record {
        Record::from_rdata(Name::from_str(name).unwrap(), ttl, RData::A(A(addr)))
    }

    fn positive_response(ttl: u32) -> Message {
        let mut message = Message::query();
        message.metadata.response_code = ResponseCode::NoError;
        message.add_answer(a_record(
            "example.com.",
            ttl,
            Ipv4Addr::new(93, 184, 216, 34),
        ));
        message
    }

    fn soa_authority(minimum: u32) -> Record {
        Record::from_rdata(
            Name::from_str("example.com.").unwrap(),
            3600,
            RData::SOA(SOA::new(
                Name::from_str("ns1.example.com.").unwrap(),
                Name::from_str("admin.example.com.").unwrap(),
                1,
                7200,
                3600,
                1209600,
                minimum,
            )),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn fresh_entry_is_returned_with_remaining_ttl() {
        let cache = DnsCache::new(&config(100));
        cache.store(&a_key(&cache, "example.com."), &positive_response(100));

        tokio::time::advance(Duration::from_secs(40)).await;

        match cache.lookup(&a_key(&cache, "example.com.")) {
            Lookup::Fresh(answer, remaining) => {
                assert_eq!(answer.records.len(), 1);
                assert_eq!(remaining, 60);
            }
            _ => panic!("expected a fresh hit"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn entry_past_ttl_but_within_stale_window_is_reported_stale() {
        let cache = DnsCache::new(&config(100));
        cache.store(&a_key(&cache, "example.com."), &positive_response(10));

        tokio::time::advance(Duration::from_secs(11)).await;

        assert!(matches!(
            cache.lookup(&a_key(&cache, "example.com.")),
            Lookup::Stale(_)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn entry_past_the_stale_window_is_a_miss() {
        let cache = DnsCache::new(&config(100));
        cache.store(&a_key(&cache, "example.com."), &positive_response(10));

        tokio::time::advance(Duration::from_secs(10) + MAX_STALE + Duration::from_secs(1)).await;

        assert!(matches!(
            cache.lookup(&a_key(&cache, "example.com.")),
            Lookup::Miss
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn serve_stale_disabled_misses_immediately_on_expiry() {
        let mut cfg = config(100);
        cfg.serve_stale = false;
        let cache = DnsCache::new(&cfg);
        cache.store(&a_key(&cache, "example.com."), &positive_response(10));

        tokio::time::advance(Duration::from_secs(11)).await;

        assert!(matches!(
            cache.lookup(&a_key(&cache, "example.com.")),
            Lookup::Miss
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn ttl_is_clamped_to_the_configured_range() {
        let mut cfg = config(100);
        cfg.min_ttl_seconds = 30;
        cfg.max_ttl_seconds = 300;
        let cache = DnsCache::new(&cfg);

        cache.store(&a_key(&cache, "short.example."), &positive_response(5));
        cache.store(&a_key(&cache, "long.example."), &positive_response(10_000));

        let Lookup::Fresh(_, short_remaining) = cache.lookup(&a_key(&cache, "short.example."))
        else {
            panic!("expected a hit");
        };
        let Lookup::Fresh(_, long_remaining) = cache.lookup(&a_key(&cache, "long.example.")) else {
            panic!("expected a hit");
        };
        assert_eq!(short_remaining, 30, "5s TTL clamped up to the 30s floor");
        assert_eq!(
            long_remaining, 300,
            "10000s TTL clamped down to the 300s ceiling"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn nxdomain_is_cached_as_negative_with_soa_minimum_ttl() {
        let cache = DnsCache::new(&config(100));
        let mut response = Message::query();
        response.metadata.response_code = ResponseCode::NXDomain;
        response.add_authority(soa_authority(45));

        cache.store(&a_key(&cache, "nowhere.example.com."), &response);

        match cache.lookup(&a_key(&cache, "nowhere.example.com.")) {
            Lookup::Fresh(answer, remaining) => {
                assert_eq!(answer.response_code, ResponseCode::NXDomain);
                assert!(answer.records.is_empty());
                assert_eq!(remaining, 45, "SOA minimum used as the negative TTL");
                assert_eq!(
                    answer.authorities.len(),
                    1,
                    "the SOA is kept for RFC 2308-faithful replay"
                );
            }
            _ => panic!("expected a fresh negative hit"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn negative_ttl_is_capped_even_when_soa_minimum_is_higher() {
        let cache = DnsCache::new(&config(100));
        let mut response = Message::query();
        response.metadata.response_code = ResponseCode::NoError; // NODATA case
        response.add_authority(soa_authority(999_999));

        let key = cache.key("example.com.", RecordType::TXT, DNSClass::IN);
        cache.store(&key, &response);

        let Lookup::Fresh(_, remaining) = cache.lookup(&key) else {
            panic!("expected a fresh negative hit");
        };
        assert_eq!(remaining, 60, "capped by negative_ttl_max_seconds");
    }

    #[tokio::test]
    async fn servfail_is_never_cached() {
        let cache = DnsCache::new(&config(100));
        let mut response = Message::query();
        response.metadata.response_code = ResponseCode::ServFail;

        cache.store(&a_key(&cache, "example.com."), &response);

        assert!(matches!(
            cache.lookup(&a_key(&cache, "example.com.")),
            Lookup::Miss
        ));
    }

    #[tokio::test]
    async fn truncated_reply_is_never_cached() {
        // A TC reply's usual shape — NOERROR, empty answer section — would
        // otherwise be cached as a negative entry, turning "retry over TCP"
        // into a served NODATA for the negative TTL's duration.
        let cache = DnsCache::new(&config(100));
        let mut response = positive_response(3600);
        response.answers.clear();
        response.metadata.truncation = true;

        cache.store(&a_key(&cache, "example.com."), &response);
        assert!(matches!(
            cache.lookup(&a_key(&cache, "example.com.")),
            Lookup::Miss
        ));

        // Same for a TC reply that kept some answers: partial set, not cacheable.
        let mut partial = positive_response(3600);
        partial.metadata.truncation = true;
        cache.store(&a_key(&cache, "partial.example.com."), &partial);
        assert!(matches!(
            cache.lookup(&a_key(&cache, "partial.example.com.")),
            Lookup::Miss
        ));
    }

    #[tokio::test]
    async fn filling_past_capacity_evicts_and_never_grows_unbounded() {
        // 32 total capacity (2 per shard) — insert many more than that and
        // assert the total never exceeds it.
        let cache = DnsCache::new(&config(32));
        for i in 0..500 {
            cache.store(
                &a_key(&cache, &format!("host{i}.example.com.")),
                &positive_response(3600),
            );
            assert!(cache.len() <= 32, "cache grew past its bound at i={i}");
        }
        assert!(cache.len() <= 32);
    }

    /// A shard built by hand — the public API hashes keys across 16 shards,
    /// so per-shard eviction order is only assertable directly.
    fn shard_with(entries: &[(&CacheKey, u64)], capacity: usize) -> Shard {
        let mut shard = Shard {
            map: HashMap::new(),
            queue: VecDeque::new(),
            next_seq: entries.iter().map(|(_, seq)| seq + 1).max().unwrap_or(0),
            capacity,
        };
        for (key, seq) in entries {
            shard.queue.push_back(((*key).clone(), *seq));
            shard.map.insert(
                (*key).clone(),
                Entry {
                    answer: Arc::new(CachedAnswer {
                        records: Vec::new(),
                        authorities: Vec::new(),
                        response_code: ResponseCode::NoError,
                    }),
                    inserted_at: Instant::now(),
                    ttl: Duration::from_secs(3600),
                    seq: *seq,
                },
            );
        }
        shard
    }

    #[tokio::test]
    async fn eviction_removes_the_oldest_inserted_entry() {
        let cache = DnsCache::new(&config(100));
        let first = a_key(&cache, "first.example.");
        let second = a_key(&cache, "second.example.");

        let mut shard = shard_with(&[(&first, 0), (&second, 1)], 2);
        shard.evict_oldest();
        assert!(
            !shard.map.contains_key(&first) && shard.map.contains_key(&second),
            "FIFO: the oldest-inserted entry goes first"
        );
    }

    #[tokio::test]
    async fn a_refreshed_entry_is_not_evicted_through_its_ghost_node() {
        let cache = DnsCache::new(&config(100));
        let hot = a_key(&cache, "hot.example.");
        let cold = a_key(&cache, "cold.example.");

        // `hot` was inserted first (seq 0), then refreshed (seq 2): its map
        // entry carries seq 2 and the seq-0 queue node is a ghost. Eviction
        // must skip the ghost and remove `cold`, the true oldest.
        let mut shard = shard_with(&[(&cold, 1), (&hot, 2)], 2);
        shard.queue.push_front((hot.clone(), 0));
        shard.evict_oldest();
        assert!(
            shard.map.contains_key(&hot) && !shard.map.contains_key(&cold),
            "a refresh must move the entry to the back of the eviction order"
        );
        assert!(
            !shard.queue.iter().any(|(_, seq)| *seq == 0),
            "the ghost node was consumed by the pop"
        );
    }

    #[tokio::test]
    async fn refresh_churn_keeps_the_eviction_queue_bounded() {
        // Re-inserting the same key leaves a ghost node per refresh; compact
        // must sweep them so the queue never outgrows 2× the shard bound.
        let cache = DnsCache::new(&config(32)); // 2 per shard
        let key = a_key(&cache, "refreshed.example.com.");
        for _ in 0..100 {
            cache.store(&key, &positive_response(3600));
        }
        assert_eq!(cache.len(), 1);
        assert!(
            cache.queue_len() <= 2 * 2 + 1,
            "ghost nodes accumulated past the compaction bound: {}",
            cache.queue_len()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stats_classifies_entries_by_lifetime_stage() {
        let cache = DnsCache::new(&config(100));
        cache.store(&a_key(&cache, "fresh.example."), &positive_response(3600));
        cache.store(&a_key(&cache, "stale.example."), &positive_response(10));
        cache.store(&a_key(&cache, "dead.example."), &positive_response(1));

        // t+11: the 10s entry is expired-but-stale-servable; nothing dead yet.
        tokio::time::advance(Duration::from_secs(11)).await;
        let stats = cache.stats();
        assert_eq!(stats.entries, 3);
        assert_eq!(stats.fresh, 1);
        assert_eq!(stats.stale, 2, "both short-TTL entries are in the window");
        assert_eq!(stats.expired, 0);
        assert_eq!(stats.capacity, 96, "16 shards × ⌊100/16⌋ — the real bound");
        assert!(stats.estimated_bytes > 0);

        // t+11+24h: the short-TTL entries are past their stale windows
        // (dead), the 3600s entry expired at t+3600 and stays stale-servable
        // until t+3600+24h.
        tokio::time::advance(MAX_STALE).await;
        let stats = cache.stats();
        assert_eq!(stats.fresh, 0);
        assert_eq!(stats.stale, 1);
        assert_eq!(stats.expired, 2);
        assert_eq!(stats.entries, 3, "stats never removes anything");
    }

    #[tokio::test(start_paused = true)]
    async fn clean_removes_dead_entries_but_keeps_stale_insurance() {
        // A raised TTL ceiling: with the default 86400s clamp no entry could
        // still be fresh once another is past its 24h stale window.
        let mut cfg = config(100);
        cfg.max_ttl_seconds = 200_000;
        let cache = DnsCache::new(&cfg);
        cache.store(
            &a_key(&cache, "fresh.example."),
            &positive_response(172_800),
        );
        cache.store(&a_key(&cache, "stale.example."), &positive_response(10));
        cache.store(&a_key(&cache, "dead.example."), &positive_response(1));

        // t+2+24h: the 1s entry is past its stale window (dead), the 10s one
        // expired but is still inside its window, the 2-day one is fresh.
        tokio::time::advance(Duration::from_secs(2) + MAX_STALE).await;

        let outcome = cache.clean(false);
        assert_eq!(outcome.entries_before, 3);
        assert_eq!(outcome.removed_expired, 1);
        assert_eq!(
            outcome.removed_stale, 0,
            "stale entries are RFC 8767 insurance"
        );
        assert_eq!(outcome.entries_after, 2);
        assert!(outcome.freed_bytes > 0);

        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.stale, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn clean_with_purge_stale_drops_the_stale_window_too() {
        let cache = DnsCache::new(&config(100));
        cache.store(&a_key(&cache, "fresh.example."), &positive_response(3600));
        cache.store(&a_key(&cache, "stale.example."), &positive_response(10));

        tokio::time::advance(Duration::from_secs(11)).await;

        let outcome = cache.clean(true);
        assert_eq!(outcome.removed_stale, 1);
        assert_eq!(outcome.removed_expired, 0);
        assert_eq!(outcome.entries_after, 1);
        assert!(matches!(
            cache.lookup(&a_key(&cache, "stale.example.")),
            Lookup::Miss
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn clean_without_serve_stale_treats_expiry_as_dead() {
        let mut cfg = config(100);
        cfg.serve_stale = false;
        let cache = DnsCache::new(&cfg);
        cache.store(&a_key(&cache, "gone.example."), &positive_response(10));

        tokio::time::advance(Duration::from_secs(11)).await;

        let outcome = cache.clean(false);
        assert_eq!(outcome.removed_expired, 1);
        assert_eq!(outcome.removed_stale, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn byte_estimate_counts_the_table_slab_not_just_occupied_entries() {
        let cache = DnsCache::new(&config(1000));
        assert_eq!(
            cache.stats().estimated_bytes,
            0,
            "no insert → no table, no heap"
        );

        cache.store(&a_key(&cache, "one.example.com."), &positive_response(10));
        let one = cache.stats().estimated_bytes;
        let pair = std::mem::size_of::<(CacheKey, Entry)>() as u64;
        assert!(
            one > pair,
            "one entry must cost more than its inline pair (slab slots + \
             domain + Arc block + record buffer), got {one} vs pair {pair}"
        );

        // The entry dies and is cleaned; the slab stays allocated, so the
        // estimate must not fall back to zero — that residue is exactly what
        // explains RSS not dropping after a clean.
        tokio::time::advance(Duration::from_secs(11) + MAX_STALE).await;
        let outcome = cache.clean(false);
        assert_eq!(outcome.removed_expired, 1);
        assert!(outcome.freed_bytes > 0);
        assert!(outcome.freed_bytes < one, "the slab is not freed by retain");

        let after = cache.stats();
        assert_eq!(after.entries, 0);
        assert!(
            after.estimated_bytes > 0 && after.estimated_bytes == one - outcome.freed_bytes,
            "empty-but-grown table keeps its slab: {} = {one} - {}",
            after.estimated_bytes,
            outcome.freed_bytes
        );
    }

    #[tokio::test]
    async fn lookup_outcomes_and_evictions_are_counted() {
        let cache = DnsCache::new(&config(32));
        cache.note_lookup(true);
        cache.note_lookup(true);
        cache.note_lookup(false);

        for i in 0..100 {
            cache.store(
                &a_key(&cache, &format!("host{i}.example.com.")),
                &positive_response(3600),
            );
        }

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!(
            stats.evictions >= 100 - 32,
            "100 inserts into 32 slots evicted at least the overflow, got {}",
            stats.evictions
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_hammering_never_exceeds_capacity() {
        let cache = Arc::new(DnsCache::new(&config(64)));
        let mut handles = Vec::new();
        for worker in 0..8 {
            let cache = Arc::clone(&cache);
            handles.push(tokio::spawn(async move {
                for i in 0..200 {
                    let key = a_key(&cache, &format!("w{worker}-h{i}.example.com."));
                    cache.store(&key, &positive_response(3600));
                    let _ = cache.lookup(&key);
                }
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        assert!(cache.len() <= 64);
    }
}
