use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::ClientHello;
use rustls::sign::CertifiedKey;

use crate::ca::{CaHandle, CLOCK_SKEW_HOURS};
use crate::error::CertError;
use crate::store::CertStore;

pub const LEAF_VALIDITY_DAYS: i64 = 7;
pub const LEAF_CACHE_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeafCacheStats {
    pub size: usize,
    pub capacity: usize,
    pub inflight: usize,
    pub hits: u64,
    pub unwarmed_misses: u64,
    pub prewarm_hits: u64,
    pub coalesced: u64,
    pub minted_total: u64,
    pub evictions: u64,
    pub superseded: u64,
}

struct Entry {
    key: Arc<CertifiedKey>,
    not_after: i64,
    used: u64,
}

struct Inner {
    entries: HashMap<Arc<str>, Entry>,
    inflight: HashSet<Arc<str>>,
    clock: u64,
    epoch: u64,
}

pub(crate) struct LeafCache {
    capacity: usize,
    inner: Mutex<Inner>,
    done: Condvar,
    hits: AtomicU64,
    unwarmed_misses: AtomicU64,
    prewarm_hits: AtomicU64,
    coalesced: AtomicU64,
    minted: AtomicU64,
    evictions: AtomicU64,
    superseded: AtomicU64,
}

impl fmt::Debug for LeafCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeafCache")
            .field("stats", &self.stats())
            .finish()
    }
}

struct InflightGuard<'a> {
    cache: &'a LeafCache,
    host: Arc<str>,
}

enum Lease<'a> {
    Fresh(Arc<CertifiedKey>),
    Mint(InflightGuard<'a>, u64),
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.cache.lock();
        inner.inflight.remove(&self.host);
        drop(inner);
        self.cache.done.notify_all();
    }
}

impl LeafCache {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                inflight: HashSet::new(),
                clock: 0,
                epoch: 0,
            }),
            done: Condvar::new(),
            hits: AtomicU64::new(0),
            unwarmed_misses: AtomicU64::new(0),
            prewarm_hits: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            minted: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            superseded: AtomicU64::new(0),
        }
    }

    pub(crate) fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
        inner.epoch += 1;
    }

    pub(crate) fn stats(&self) -> LeafCacheStats {
        let inner = self.lock();
        let (size, inflight) = (inner.entries.len(), inner.inflight.len());
        drop(inner);
        LeafCacheStats {
            size,
            capacity: self.capacity,
            inflight,
            hits: self.hits.load(Ordering::Relaxed),
            unwarmed_misses: self.unwarmed_misses.load(Ordering::Relaxed),
            prewarm_hits: self.prewarm_hits.load(Ordering::Relaxed),
            coalesced: self.coalesced.load(Ordering::Relaxed),
            minted_total: self.minted.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            superseded: self.superseded.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn cached(&self, host: &str, now: i64) -> Option<Arc<CertifiedKey>> {
        let mut inner = self.lock();
        let clock = inner.clock;
        let hit = take_fresh(&mut inner, host, now, clock);
        if hit.is_some() {
            inner.clock = clock + 1;
        }
        drop(inner);

        match hit {
            Some(key) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(key)
            }
            None => {
                self.unwarmed_misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub(crate) fn prewarm(
        &self,
        ca: &CaHandle,
        host: &str,
        now: i64,
    ) -> Result<Arc<CertifiedKey>, CertError> {
        let (guard, epoch) = match self.lease(host, now) {
            Lease::Fresh(key) => return Ok(key),
            Lease::Mint(guard, epoch) => (guard, epoch),
        };
        let (key, not_after) = mint(ca, host)?;
        self.minted.fetch_add(1, Ordering::Relaxed);
        self.store_minted(guard, epoch, &key, not_after);
        Ok(key)
    }

    fn lease(&self, host: &str, now: i64) -> Lease<'_> {
        let mut inner = self.lock();
        let mut waited = false;
        loop {
            let clock = inner.clock;
            if let Some(key) = take_fresh(&mut inner, host, now, clock) {
                inner.clock = clock + 1;
                drop(inner);
                match waited {
                    true => self.coalesced.fetch_add(1, Ordering::Relaxed),
                    false => self.prewarm_hits.fetch_add(1, Ordering::Relaxed),
                };
                return Lease::Fresh(key);
            }
            if !inner.inflight.contains(host) {
                break;
            }
            waited = true;
            inner = self.wait(inner);
        }

        let owned: Arc<str> = Arc::from(host);
        inner.inflight.insert(Arc::clone(&owned));
        let epoch = inner.epoch;
        drop(inner);

        Lease::Mint(
            InflightGuard {
                cache: self,
                host: owned,
            },
            epoch,
        )
    }

    fn store_minted(
        &self,
        guard: InflightGuard<'_>,
        epoch: u64,
        key: &Arc<CertifiedKey>,
        not_after: i64,
    ) {
        let mut inner = self.lock();
        if inner.epoch == epoch {
            if !inner.entries.contains_key(&guard.host) && inner.entries.len() >= self.capacity {
                self.evict_one(&mut inner);
            }
            let clock = inner.clock;
            inner.clock = clock + 1;
            inner.entries.insert(
                Arc::clone(&guard.host),
                Entry {
                    key: Arc::clone(key),
                    not_after,
                    used: clock,
                },
            );
        } else {
            self.superseded.fetch_add(1, Ordering::Relaxed);
        }
        drop(inner);
        drop(guard);
    }

    fn evict_one(&self, inner: &mut Inner) {
        let victim = inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(host, _)| Arc::clone(host));
        if let Some(victim) = victim {
            inner.entries.remove(&victim);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| {
            self.inner.clear_poison();
            poisoned.into_inner()
        })
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, Inner>) -> MutexGuard<'a, Inner> {
        self.done.wait(guard).unwrap_or_else(|poisoned| {
            self.inner.clear_poison();
            poisoned.into_inner()
        })
    }
}

fn take_fresh(inner: &mut Inner, host: &str, now: i64, clock: u64) -> Option<Arc<CertifiedKey>> {
    match inner.entries.get_mut(host) {
        Some(entry) if entry.not_after > now => {
            entry.used = clock;
            Some(Arc::clone(&entry.key))
        }
        _ => None,
    }
}

fn mint(ca: &CaHandle, host: &str) -> Result<(Arc<CertifiedKey>, i64), CertError> {
    let mut params = match host.parse::<IpAddr>() {
        Ok(_) => rcgen::CertificateParams::new(Vec::<String>::new()),
        Err(_) => rcgen::CertificateParams::new(vec![host.to_string()]),
    }
    .map_err(CertError::Generate)?;
    if let Ok(ip) = host.parse::<IpAddr>() {
        params.subject_alt_names.push(rcgen::SanType::IpAddress(ip));
    }

    params.distinguished_name = {
        let mut dn = rcgen::DistinguishedName::new();
        if host.len() <= 64 {
            dn.push(rcgen::DnType::CommonName, host);
        }
        dn
    };
    params.is_ca = rcgen::IsCa::ExplicitNoCa;
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let issued = time::OffsetDateTime::now_utc();
    params.not_before = issued - time::Duration::hours(CLOCK_SKEW_HOURS);
    params.not_after = match time::OffsetDateTime::from_unix_timestamp(ca.summary().not_after) {
        Ok(ca_not_after) => (issued + time::Duration::days(LEAF_VALIDITY_DAYS)).min(ca_not_after),
        Err(_) => issued + time::Duration::days(LEAF_VALIDITY_DAYS),
    };
    let not_after = params.not_after.unix_timestamp();

    let key_pair = rcgen::KeyPair::generate().map_err(CertError::Generate)?;
    let cert = params
        .signed_by(&key_pair, ca.issuer())
        .map_err(CertError::Generate)?;

    let der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&der)
        .map_err(|error| CertError::parse("the minted leaf key", error))?;
    let certified = CertifiedKey::new(vec![cert.der().clone()], signing_key);
    Ok((Arc::new(certified), not_after))
}

#[derive(Debug)]
pub struct MintingResolver {
    store: Arc<CertStore>,
    fallback: Option<Arc<CertifiedKey>>,
}

impl MintingResolver {
    pub fn new(store: Arc<CertStore>, fallback: Option<Arc<CertifiedKey>>) -> Self {
        Self { store, fallback }
    }
}

impl rustls::server::ResolvesServerCert for MintingResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        match client_hello.server_name() {
            Some(host) => self
                .store
                .cached_leaf(host)
                .or_else(|| self.fallback.clone()),
            None => self.fallback.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ca::{self, CaParams};
    use std::sync::Barrier;

    fn authority() -> CaHandle {
        let generated = ca::generate(&CaParams::default()).unwrap();
        CaHandle::load(&generated.cert_pem, &generated.key_pem).unwrap()
    }

    fn now() -> i64 {
        time::OffsetDateTime::now_utc().unix_timestamp()
    }

    #[test]
    fn a_mint_started_under_a_replaced_authority_never_reaches_the_cache() {
        let old = authority();
        let cache = LeafCache::with_capacity(4);

        let Lease::Mint(guard, epoch) = cache.lease("stale.example", now()) else {
            panic!("an empty cache must hand out a mint lease");
        };
        let (key, not_after) = mint(&old, "stale.example").unwrap();

        cache.clear();
        cache.store_minted(guard, epoch, &key, not_after);

        assert!(
            cache.cached("stale.example", now()).is_none(),
            "a leaf minted under the archived authority must not survive the purge"
        );
        let stats = cache.stats();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.inflight, 0);
        assert_eq!(stats.superseded, 1);
    }

    #[test]
    fn a_mint_that_wins_the_race_against_no_regeneration_is_cached() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        let Lease::Mint(guard, epoch) = cache.lease("fresh.example", now()) else {
            panic!("an empty cache must hand out a mint lease");
        };
        let (key, not_after) = mint(&ca, "fresh.example").unwrap();
        cache.store_minted(guard, epoch, &key, not_after);

        assert!(cache.cached("fresh.example", now()).is_some());
        assert_eq!(cache.stats().superseded, 0);
    }

    #[test]
    fn a_prewarmed_host_is_then_served_without_minting() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        assert!(cache.cached("a.example", now()).is_none());
        assert_eq!(cache.stats().minted_total, 0);
        assert_eq!(cache.stats().unwarmed_misses, 1);

        let warmed = cache.prewarm(&ca, "a.example", now()).unwrap();
        let served = cache.cached("a.example", now()).unwrap();
        assert!(Arc::ptr_eq(&warmed, &served));

        let stats = cache.stats();
        assert_eq!((stats.hits, stats.unwarmed_misses), (1, 1));
        assert_eq!((stats.minted_total, stats.prewarm_hits), (1, 0));
        assert_eq!((stats.size, stats.inflight), (1, 0));
    }

    #[test]
    fn a_second_prewarm_of_a_warm_host_does_not_mint_again() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        let first = cache.prewarm(&ca, "a.example", now()).unwrap();
        let second = cache.prewarm(&ca, "a.example", now()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        let stats = cache.stats();
        assert_eq!((stats.minted_total, stats.prewarm_hits), (1, 1));
        assert_eq!(stats.coalesced, 0);
    }

    #[test]
    fn concurrent_prewarms_of_one_host_mint_exactly_once() {
        const THREADS: usize = 16;
        let ca = authority();
        let cache = LeafCache::with_capacity(4);
        let barrier = Barrier::new(THREADS);

        let keys = std::thread::scope(|scope| {
            let handles = (0..THREADS)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        cache.prewarm(&ca, "shared.example", now()).unwrap()
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });

        for key in &keys {
            assert!(Arc::ptr_eq(&keys[0], key));
        }

        let stats = cache.stats();
        assert_eq!(
            stats.minted_total, 1,
            "single-flight must collapse the mint"
        );
        assert_eq!(stats.coalesced + stats.prewarm_hits, (THREADS - 1) as u64);
        assert_eq!((stats.size, stats.inflight), (1, 0));
    }

    #[test]
    fn concurrent_prewarms_of_distinct_hosts_all_complete() {
        const THREADS: usize = 16;
        let ca = authority();
        let cache = LeafCache::with_capacity(THREADS);
        let barrier = Barrier::new(THREADS);

        std::thread::scope(|scope| {
            for index in 0..THREADS {
                let (cache, ca, barrier) = (&cache, &ca, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    cache
                        .prewarm(ca, &format!("host{index}.example"), now())
                        .unwrap();
                });
            }
        });

        let stats = cache.stats();
        assert_eq!(stats.minted_total, THREADS as u64);
        assert_eq!((stats.size, stats.inflight), (THREADS, 0));
        assert_eq!(stats.coalesced, 0);
    }

    #[test]
    fn the_cache_evicts_the_least_recently_used_host_at_capacity() {
        let ca = authority();
        let cache = LeafCache::with_capacity(2);

        cache.prewarm(&ca, "old.example", now()).unwrap();
        cache.prewarm(&ca, "kept.example", now()).unwrap();
        cache.prewarm(&ca, "old.example", now()).unwrap();
        cache.prewarm(&ca, "new.example", now()).unwrap();

        let stats = cache.stats();
        assert_eq!(stats.size, 2);
        assert_eq!(stats.evictions, 1);

        cache.prewarm(&ca, "old.example", now()).unwrap();
        assert_eq!(
            cache.stats().evictions,
            1,
            "old.example must still be cached"
        );
        cache.prewarm(&ca, "kept.example", now()).unwrap();
        assert_eq!(
            cache.stats().evictions,
            2,
            "kept.example was the least recently used entry and must have been evicted"
        );
    }

    #[test]
    fn an_expired_entry_is_neither_served_nor_reused() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        let fresh = cache.prewarm(&ca, "a.example", now()).unwrap();
        let past_expiry = now() + (LEAF_VALIDITY_DAYS + 1) * 24 * 60 * 60;

        assert!(cache.cached("a.example", past_expiry).is_none());
        let reminted = cache.prewarm(&ca, "a.example", past_expiry).unwrap();
        assert!(!Arc::ptr_eq(&fresh, &reminted));

        let stats = cache.stats();
        assert_eq!(stats.minted_total, 2);
        assert_eq!(stats.size, 1, "the re-mint replaces the expired entry");
    }

    #[test]
    fn clearing_the_cache_drops_every_entry_but_keeps_the_counters() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        cache.prewarm(&ca, "a.example", now()).unwrap();
        cache.clear();

        let stats = cache.stats();
        assert_eq!(stats.size, 0);
        assert_eq!(stats.minted_total, 1);
    }

    #[test]
    fn an_ip_literal_host_is_minted_with_an_ip_san() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);

        let key = cache.prewarm(&ca, "192.168.88.1", now()).unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&key.cert[0]).unwrap();
        let extension = parsed.subject_alternative_name().unwrap().unwrap();
        assert!(extension.value.general_names.iter().any(|name| matches!(
            name,
            x509_parser::extensions::GeneralName::IPAddress(octets) if octets.len() == 4
        )));
    }

    #[test]
    fn a_minted_leaf_is_small_enough_for_the_cache_cap() {
        let ca = authority();
        let cache = LeafCache::with_capacity(4);
        let key = cache.prewarm(&ca, "a.example", now()).unwrap();

        let der = key.cert[0].len();
        assert_eq!(key.cert.len(), 1);
        assert!(
            der < 1024,
            "a P-256 leaf must stay well under 1 KiB; measured {der} bytes"
        );
        println!("minted leaf DER: {der} bytes");
    }
}
