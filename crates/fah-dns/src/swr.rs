//! Stale-while-refresh: the detached pool that refreshes stale cache entries
//! (ADR-0005).
//!
//! A strict producer/consumer split. The DNS path *produces* — on a stale hit
//! it answers the client from cache and offers a job here — and never consumes,
//! never awaits, and never learns whether the refresh happened. This pool
//! *consumes*, on its own tasks, with no path back into request handling.
//!
//! Three properties keep it off the query path, and they are the whole
//! guarantee — Tokio has no priority scheduler, so nothing here can promise
//! "lower priority" beyond this:
//!
//! 1. **The pool is fixed at `[dns.cache] swr_workers`.** At most that many
//!    refreshes are in flight, whatever the query rate.
//! 2. **Refreshes are I/O-bound.** A worker holds a runtime thread for the
//!    microseconds it takes to serialize a query, then parks on the socket. The
//!    contended resources are upstream bandwidth and the cache's shard locks,
//!    not CPU.
//! 3. **Enqueue is [`mpsc::Sender::try_send`], never `send().await`.** A full
//!    queue drops the job and keeps serving stale; it can never become
//!    backpressure on a client.
//!
//! Deduplication does *not* live here. It lives in the cache entry itself, as
//! a claim taken under the shard lock the stale lookup already holds — see
//! [`crate::cache::DnsCache::lookup_and_claim_refresh`]. This module only ever
//! sees jobs whose claim was already won, so a thousand simultaneous queries
//! for one stale key put exactly one job in the queue.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hickory_proto::op::{Message, Query as WireQuery};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::cache::{CacheKey, DnsCache, REFRESH_FAILURE_COOLDOWN};
use crate::upstream::Forwarder;

/// Queue depth per worker. Deep enough to absorb a burst of distinct stale keys
/// expiring together, shallow enough that a job cannot sit so long that its
/// entry has changed underneath it. The claim in the cache entry already caps
/// duplicates, so this bounds *distinct* pending keys — hard rule 4: the queue
/// is bounded by configuration, never by traffic or uptime.
const QUEUE_DEPTH_PER_WORKER: usize = 64;

/// SWR counters for `/api/v1/telemetry`, read by the binary off
/// [`crate::Pipeline::swr_stats`]. Process-lifetime totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwrStats {
    /// Refresh jobs handed to the queue.
    pub enqueued: u64,
    /// Stale hits that found a refresh already claimed — the deduplication
    /// working. Expected to dwarf `enqueued` under load on a popular name.
    pub deduplicated: u64,
    /// Jobs dropped because the queue was full. The client still got its stale
    /// answer; only the refresh was skipped. Sustained non-zero means
    /// `swr_workers` is undersized for the traffic.
    pub dropped: u64,
    /// Refreshes that came back with a cacheable answer and replaced the entry.
    pub completed: u64,
    /// Refreshes that failed or returned something uncacheable, and therefore
    /// put the entry into its failure cooldown.
    pub failed: u64,
}

/// The queue and its counters. Held by [`crate::Pipeline`] as an `Option` —
/// `None` when `swr_workers = 0`, which restores the pre-ADR-0005 behaviour
/// where a stale entry answers only after a forward has failed.
pub(crate) struct SwrPool {
    tx: mpsc::Sender<CacheKey>,
    /// Taken exactly once, by [`SwrPool::spawn_workers`]. The receiver cannot
    /// live in the pool proper because the workers need to own it, and the pool
    /// is built in `Pipeline::new` — which has no business spawning tasks — so
    /// it is parked here until the binary starts the pool alongside its other
    /// long-lived tasks.
    rx: Mutex<Option<mpsc::Receiver<CacheKey>>>,
    workers: usize,
    enqueued: AtomicU64,
    deduplicated: AtomicU64,
    dropped: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
}

impl SwrPool {
    /// `None` when `workers == 0` — SWR disabled, and the caller keeps the
    /// forward-first stale behaviour.
    pub(crate) fn new(workers: u32) -> Option<Self> {
        let workers = workers as usize;
        if workers == 0 {
            return None;
        }
        let (tx, rx) = mpsc::channel(workers * QUEUE_DEPTH_PER_WORKER);
        Some(Self {
            tx,
            rx: Mutex::new(Some(rx)),
            workers,
            enqueued: AtomicU64::new(0),
            deduplicated: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        })
    }

    /// Offers a claimed refresh to the pool. Never blocks and never fails
    /// upward: a full queue drops the job, releases the claim so the next stale
    /// hit can retry immediately, and leaves the client's stale answer exactly
    /// as it was.
    ///
    /// Only ever called with a key whose claim this caller just won, so it
    /// cannot enqueue a duplicate.
    pub(crate) fn offer(&self, cache: &DnsCache, key: CacheKey) {
        match self.tx.try_send(key) {
            Ok(()) => {
                self.enqueued.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Full(key)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                // Hand the claim back rather than let it sit out its lease: no
                // worker ever saw this job, so nothing is refreshing that key.
                cache.release_refresh_claim(&key);
            }
            Err(mpsc::error::TrySendError::Closed(key)) => {
                // Workers never shut down before the listeners in production;
                // this is reachable in tests and during shutdown. Same repair.
                cache.release_refresh_claim(&key);
            }
        }
    }

    /// Counts a stale hit whose refresh was already claimed by someone else.
    pub(crate) fn note_deduplicated(&self) {
        self.deduplicated.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn stats(&self) -> SwrStats {
        SwrStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            deduplicated: self.deduplicated.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }

    /// Starts the pool. Returns one handle per worker for the binary to abort
    /// on shutdown, mirroring `ListManager::spawn_scheduler`; a second call
    /// returns nothing, since the receiver is already spoken for.
    pub(crate) fn spawn_workers<F: Forwarder>(
        self: &Arc<Self>,
        cache: Arc<DnsCache>,
        forwarder: F,
    ) -> Vec<JoinHandle<()>> {
        let Some(rx) = self.rx.lock().unwrap().take() else {
            return Vec::new();
        };
        // One receiver, N consumers: the lock is held only across `recv`, never
        // across the forward, so all N refreshes still run concurrently.
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        (0..self.workers)
            .map(|_| {
                let pool = Arc::clone(self);
                let cache = Arc::clone(&cache);
                let forwarder = forwarder.clone();
                let rx = Arc::clone(&rx);
                tokio::spawn(async move {
                    loop {
                        // Scoped so the receiver lock is released before the
                        // forward — otherwise the pool would be serial.
                        let key = {
                            let mut guard = rx.lock().await;
                            match guard.recv().await {
                                Some(key) => key,
                                None => return,
                            }
                        };
                        // Cooperative, not a mechanism: it hands the scheduler
                        // a chance to run queued query tasks before this one
                        // starts work. The real bound on this pool's impact is
                        // its fixed size (see the module docs).
                        tokio::task::yield_now().await;
                        pool.refresh(&cache, &forwarder, &key).await;
                    }
                })
            })
            .collect()
    }

    /// One refresh. Anything other than "the upstream answered and the cache
    /// kept it" is a failure that takes the cooldown — including a successful
    /// forward whose answer `store` declines (SERVFAIL, REFUSED, truncated).
    /// Treating those as success would leave the entry stale with its claim
    /// already cleared, and the next stale hit would immediately re-enqueue a
    /// job that fails the same way.
    async fn refresh<F: Forwarder>(&self, cache: &DnsCache, forwarder: &F, key: &CacheKey) {
        let request = refresh_query(key);
        let stored = match forwarder.forward(&request).await {
            Ok(outcome) => cache.store(key, &outcome.message),
            Err(err) => {
                tracing::trace!(error = %err, "stale-while-refresh forward failed");
                false
            }
        };

        if stored {
            self.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed.fetch_add(1, Ordering::Relaxed);
            cache.suppress_refresh(key, REFRESH_FAILURE_COOLDOWN);
        }
    }
}

/// Rebuilds the question from the cache key. The key holds everything a DNS
/// question needs — wire-format name, type and class — because that is exactly
/// what it is keyed on.
///
/// The refresh is a *new* query rather than a replay of the client's: the
/// original request is long gone by the time a worker runs, and holding one
/// would mean keeping a client's message alive in the queue. Consequently the
/// refresh carries no EDNS options from whoever triggered it. That matches how
/// the cache already behaves — `CacheKey` has never included EDNS, so entries
/// are already shared across clients with different options.
fn refresh_query(key: &CacheKey) -> Message {
    let mut request = Message::query();
    let mut question = WireQuery::query(key.name(), key.qtype());
    question.set_query_class(key.qclass());
    request.add_query(question);
    request.metadata.recursion_desired = true;
    request
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use hickory_proto::op::{OpCode, ResponseCode};
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{DNSClass, Name, RData, Record, RecordType};

    use super::*;
    use crate::cache::Lookup;
    use crate::upstream::ForwardOutcome;

    /// A forwarder that counts calls and replays a scripted outcome, so a test
    /// can assert "exactly one refresh happened" rather than infer it.
    #[derive(Clone)]
    struct CountingForwarder {
        calls: Arc<AtomicU64>,
        outcome: Outcome,
    }

    #[derive(Clone, Copy)]
    enum Outcome {
        Answer,
        ServFail,
        Error,
    }

    impl Forwarder for CountingForwarder {
        async fn forward(&self, query: &Message) -> std::io::Result<ForwardOutcome> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.outcome {
                Outcome::Error => Err(std::io::Error::other("upstream down")),
                Outcome::ServFail => {
                    let mut response = Message::response(query.metadata.id, OpCode::Query);
                    response.queries = query.queries.clone();
                    response.metadata.response_code = ResponseCode::ServFail;
                    Ok(ForwardOutcome::new(response, 0))
                }
                Outcome::Answer => {
                    let mut response = Message::response(query.metadata.id, OpCode::Query);
                    response.queries = query.queries.clone();
                    response.add_answer(Record::from_rdata(
                        Name::from_ascii("swr.example.").unwrap(),
                        300,
                        RData::A(A(Ipv4Addr::new(10, 0, 0, 1))),
                    ));
                    Ok(ForwardOutcome::new(response, 0))
                }
            }
        }
    }

    fn forwarder(outcome: Outcome) -> (CountingForwarder, Arc<AtomicU64>) {
        let calls = Arc::new(AtomicU64::new(0));
        (
            CountingForwarder {
                calls: Arc::clone(&calls),
                outcome,
            },
            calls,
        )
    }

    /// A cache holding one entry that is past its TTL but inside the stale
    /// window — the only state SWR ever acts on.
    ///
    /// Requires `start_paused`: the entry is aged by advancing the virtual
    /// clock rather than by clamping TTLs to zero, because a zero `max_ttl`
    /// would make the *refreshed* answer instantly stale too and quietly
    /// invalidate every assertion that a refresh restored freshness.
    async fn cache_with_stale_entry() -> (Arc<DnsCache>, CacheKey) {
        const SEED_TTL: u32 = 10;

        let cache = Arc::new(DnsCache::new(&fah_config::DnsCacheConfig::default()));
        let key = cache.key("swr.example.", RecordType::A, DNSClass::IN);

        let mut seed = Message::response(0, OpCode::Query);
        seed.add_answer(Record::from_rdata(
            Name::from_ascii("swr.example.").unwrap(),
            SEED_TTL,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));
        assert!(cache.store(&key, &seed), "test seed must be cacheable");

        tokio::time::advance(Duration::from_secs(u64::from(SEED_TTL) + 1)).await;
        assert!(
            matches!(cache.lookup(&key), Lookup::Stale { .. }),
            "the seed must be stale before a refresh test means anything"
        );
        (cache, key)
    }

    #[test]
    fn zero_workers_disables_the_pool_entirely() {
        assert!(SwrPool::new(0).is_none());
        assert!(SwrPool::new(1).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_replaces_the_stale_entry_and_counts_once() {
        let (cache, key) = cache_with_stale_entry().await;
        let (forwarder, calls) = forwarder(Outcome::Answer);
        let pool = Arc::new(SwrPool::new(1).unwrap());

        pool.refresh(&cache, &forwarder, &key).await;

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(pool.stats().completed, 1);
        assert_eq!(pool.stats().failed, 0);
        let Lookup::Fresh(answer, _) = cache.lookup(&key) else {
            panic!("a refreshed entry must be fresh again");
        };
        assert!(
            matches!(answer.records[0].data, RData::A(A(ip)) if ip == Ipv4Addr::new(10, 0, 0, 1)),
            "the refreshed answer must have replaced the stale one"
        );
    }

    /// The uncacheable-answer trap: the forward *succeeded*, so a naive
    /// implementation counts it done and clears the claim — leaving the entry
    /// stale and the next hit free to enqueue the same doomed refresh.
    #[tokio::test(start_paused = true)]
    async fn a_servfail_refresh_counts_as_failed_and_takes_the_cooldown() {
        let (cache, key) = cache_with_stale_entry().await;
        let (forwarder, _) = forwarder(Outcome::ServFail);
        let pool = Arc::new(SwrPool::new(1).unwrap());

        pool.refresh(&cache, &forwarder, &key).await;

        assert_eq!(pool.stats().completed, 0);
        assert_eq!(pool.stats().failed, 1);
        assert!(
            matches!(
                cache.lookup_and_claim_refresh(&key),
                Lookup::Stale {
                    claimed_refresh: false,
                    ..
                }
            ),
            "a failed refresh must suppress the next one, not invite it"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_forward_counts_as_failed_and_takes_the_cooldown() {
        let (cache, key) = cache_with_stale_entry().await;
        let (forwarder, _) = forwarder(Outcome::Error);
        let pool = Arc::new(SwrPool::new(1).unwrap());

        pool.refresh(&cache, &forwarder, &key).await;

        assert_eq!(pool.stats().failed, 1);
        assert!(matches!(
            cache.lookup_and_claim_refresh(&key),
            Lookup::Stale {
                claimed_refresh: false,
                ..
            }
        ));
    }

    /// A full queue must not strand the key: no worker saw the job, so nothing
    /// is refreshing it and the claim has to go back.
    #[tokio::test(start_paused = true)]
    async fn a_full_queue_drops_the_job_and_releases_the_claim() {
        let (cache, key) = cache_with_stale_entry().await;
        let pool = SwrPool::new(1).unwrap();

        // Fill the queue without starting any workers to drain it.
        for _ in 0..(QUEUE_DEPTH_PER_WORKER) {
            pool.offer(&cache, key.clone());
        }
        assert_eq!(pool.stats().enqueued as usize, QUEUE_DEPTH_PER_WORKER);
        assert_eq!(pool.stats().dropped, 0);

        // Claim it, as the pipeline would, then find the queue full.
        assert!(matches!(
            cache.lookup_and_claim_refresh(&key),
            Lookup::Stale {
                claimed_refresh: true,
                ..
            }
        ));
        pool.offer(&cache, key.clone());

        assert_eq!(pool.stats().dropped, 1);
        assert!(
            matches!(
                cache.lookup_and_claim_refresh(&key),
                Lookup::Stale {
                    claimed_refresh: true,
                    ..
                }
            ),
            "a dropped job must hand the claim back, not sit out its lease"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn workers_drain_the_queue_and_only_start_once() {
        let (cache, key) = cache_with_stale_entry().await;
        let (forwarder, calls) = forwarder(Outcome::Answer);
        let pool = Arc::new(SwrPool::new(2).unwrap());

        let handles = pool.spawn_workers(Arc::clone(&cache), forwarder.clone());
        assert_eq!(handles.len(), 2);
        assert!(
            pool.spawn_workers(Arc::clone(&cache), forwarder).is_empty(),
            "the receiver is taken once; a second start must be a no-op"
        );

        pool.offer(&cache, key.clone());

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while pool.stats().completed == 0 {
            assert!(std::time::Instant::now() < deadline, "refresh never landed");
            tokio::task::yield_now().await;
        }
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        for handle in handles {
            handle.abort();
        }
    }

    #[test]
    fn the_refresh_query_reproduces_the_cached_question() {
        let cache = DnsCache::new(&fah_config::DnsCacheConfig::default());
        let key = cache.key("swr.example.", RecordType::AAAA, DNSClass::CH);

        let request = refresh_query(&key);

        let question = &request.queries[0];
        assert_eq!(question.name().to_ascii(), "swr.example.");
        assert_eq!(question.query_type(), RecordType::AAAA);
        assert_eq!(
            question.query_class(),
            DNSClass::CH,
            "the class is part of the key and must survive the round trip"
        );
        assert!(request.metadata.recursion_desired);
    }
}
