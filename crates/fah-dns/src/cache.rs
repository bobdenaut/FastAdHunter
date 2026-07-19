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
//! **Eviction: FIFO with stale-first quick-demotion, not LRU.** True LRU
//! requires mutating access-order state on every *read*, which turns a cache
//! hit into a write under lock; a bounded network cache doesn't need that
//! precision because TTL already retires most entries before capacity
//! pressure ever triggers eviction. When a shard is full, an entry already
//! useless (past its stale-serving deadline — or merely expired when
//! `serve_stale` is off) is evicted first; only if none exists does the
//! true-oldest entry go. This is
//! S3-FIFO's "quick demotion of used-once, cold entries" idea in miniature,
//! without the ghost queue's bookkeeping cost.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::BuildHasher;
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
}

impl Entry {
    fn expires_at(&self) -> Instant {
        self.inserted_at + self.ttl
    }

    fn stale_deadline(&self) -> Instant {
        self.expires_at() + MAX_STALE
    }
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
    capacity: usize,
}

/// Evicts one entry from a full shard: quick-demote anything already useless
/// first, else the true-oldest. "Useless" depends on `serve_stale`: with it
/// on, an entry stays servable until the RFC 8767 window closes; with it off,
/// TTL expiry is the end of the line.
fn evict_one(map: &mut HashMap<CacheKey, Entry>, now: Instant, serve_stale: bool) {
    let dead_after = |e: &Entry| {
        if serve_stale {
            e.stale_deadline()
        } else {
            e.expires_at()
        }
    };
    if let Some(dead) = map
        .iter()
        .find(|(_, e)| now >= dead_after(e))
        .map(|(k, _)| k.clone())
    {
        map.remove(&dead);
        return;
    }
    if let Some(oldest) = map
        .iter()
        .min_by_key(|(_, e)| e.inserted_at)
        .map(|(k, _)| k.clone())
    {
        map.remove(&oldest);
    }
}

pub(crate) struct DnsCache {
    shards: Vec<Mutex<Shard>>,
    hash_builder: RandomState,
    min_ttl: u32,
    max_ttl: u32,
    negative_ttl_max: u32,
    serve_stale: bool,
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
            evict_one(&mut guard.map, now, self.serve_stale);
        }
        guard.map.insert(
            key.clone(),
            Entry {
                answer: Arc::new(answer),
                inserted_at: now,
                ttl: Duration::from_secs(u64::from(ttl_seconds)),
            },
        );
    }

    /// Total entries across every shard. Test-only for now (asserts the
    /// capacity bound); p1-08 can promote this to a real accessor when the
    /// metrics exporter needs a cache-size gauge.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().unwrap().map.len())
            .sum()
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

    #[tokio::test(start_paused = true)]
    async fn evict_one_prefers_expired_entries_when_serve_stale_is_off() {
        // Exercises `evict_one` directly: with serve_stale off, an entry
        // expired one second ago is dead weight and must go before an older
        // but still-fresh entry; with serve_stale on, that same entry is
        // still servable (RFC 8767 window) so the true-oldest goes instead.
        fn entry(answer_ttl: u64, inserted_at: Instant) -> Entry {
            Entry {
                answer: Arc::new(CachedAnswer {
                    records: Vec::new(),
                    authorities: Vec::new(),
                    response_code: ResponseCode::NoError,
                }),
                inserted_at,
                ttl: Duration::from_secs(answer_ttl),
            }
        }
        let cache = DnsCache::new(&config(100));
        let older_fresh = a_key(&cache, "older-fresh.example.");
        let newer_expired = a_key(&cache, "newer-expired.example.");

        let t0 = Instant::now();
        tokio::time::advance(Duration::from_secs(10)).await;
        let t10 = Instant::now();
        tokio::time::advance(Duration::from_secs(6)).await;
        let now = Instant::now(); // t16: ttl-5 entry from t10 expired at t15

        let mut map = HashMap::new();
        map.insert(older_fresh.clone(), entry(3600, t0));
        map.insert(newer_expired.clone(), entry(5, t10));
        evict_one(&mut map, now, false);
        assert!(
            !map.contains_key(&newer_expired) && map.contains_key(&older_fresh),
            "serve_stale off: the expired entry is dead weight and goes first"
        );

        let mut map = HashMap::new();
        map.insert(older_fresh.clone(), entry(3600, t0));
        map.insert(newer_expired.clone(), entry(5, t10));
        evict_one(&mut map, now, true);
        assert!(
            map.contains_key(&newer_expired) && !map.contains_key(&older_fresh),
            "serve_stale on: the expired entry is still stale-servable, oldest goes"
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
