//! The full pipeline: decode -> Rule Engine verdict -> Block synthesizes an
//! answer directly, Allow/Pass check the cache before falling through to the
//! [`Forwarder`] (in production [`crate::upstream::UpstreamPool`]).
//! ADR-0001: the Rule Engine runs on every query, before anything else, and
//! the cache stores upstream answers only — never verdicts.
//!
//! One [`Pipeline`] is shared (cheaply cloned — everything it owns is an
//! `Arc`) across every listener task; ARCHITECTURE.md's runtime model has no
//! central dispatcher, so each UDP/TCP task calls straight into it.

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_config::DnsCacheConfig;
use fah_model::{
    AnswerOutcome, ClientTransport, Event, Query as FahQuery, QueryEvent, StaleServe, Verdict,
};
use fah_rules::{ListManager, MatchDecision, PolicyState};
use hickory_proto::op::{Message, MessageType, OpCode, Query as WireQuery, ResponseCode};
use tokio::sync::mpsc;
use tracing::trace;

use crate::cache::{DnsCache, Lookup, STALE_SERVE_TTL};
use crate::qtype::{domain_of, to_fah_query_type};
use crate::response;
use crate::swr::SwrPool;
use crate::upstream::Forwarder;

/// Which listener received the request — decides the reply's size ceiling:
/// UDP truncates to the request's own EDNS payload size (or 512 without
/// EDNS), TCP has no practical ceiling (RFC 1035's 65535-byte length prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
    Dot,
    Doh,
}

impl From<Transport> for ClientTransport {
    fn from(transport: Transport) -> Self {
        match transport {
            Transport::Udp => ClientTransport::Udp,
            Transport::Tcp => ClientTransport::Tcp,
            Transport::Dot => ClientTransport::Dot,
            Transport::Doh => ClientTransport::Doh,
        }
    }
}

struct Resolved {
    response: Message,
    cache_hit: bool,
    upstream_used: bool,
    stale: Option<StaleServe>,
    outcome: AnswerOutcome,
    endpoint: Option<u8>,
}

impl Resolved {
    fn blocked(response: Message) -> Self {
        Self {
            response,
            cache_hit: false,
            upstream_used: false,
            stale: None,
            outcome: AnswerOutcome::Answered,
            endpoint: None,
        }
    }

    fn from_cache(response: Message, stale: Option<StaleServe>) -> Self {
        Self {
            response,
            cache_hit: true,
            upstream_used: false,
            stale,
            outcome: AnswerOutcome::Answered,
            endpoint: None,
        }
    }

    fn forwarded(response: Message, outcome: AnswerOutcome, endpoint: u8) -> Self {
        Self {
            response,
            cache_hit: false,
            upstream_used: true,
            stale: None,
            outcome,
            endpoint: Some(endpoint),
        }
    }

    fn synthesized_servfail(response: Message) -> Self {
        Self {
            response,
            cache_hit: false,
            upstream_used: false,
            stale: None,
            outcome: AnswerOutcome::ServfailSynthesized,
            endpoint: None,
        }
    }
}

#[derive(Clone)]
pub struct Pipeline<F: Forwarder> {
    rules: Arc<ListManager>,
    /// The client → policy map the binary keeps current (p2-06). Read per
    /// query, never computed here: an atomic-swap read, no clock, no lock.
    policies: Arc<PolicyState>,
    forwarder: F,
    blocking_ttl: u32,
    cache: Arc<DnsCache>,
    /// The stale-while-refresh pool (ADR-0005), or `None` when
    /// `[dns.cache] swr_workers = 0`. The query path only ever *offers* to it
    /// and never awaits it, so the two are fully decoupled.
    swr: Option<Arc<SwrPool>>,
    /// How often the scheduled sweep runs, or `None` when
    /// `[dns.cache] cleanup_interval_seconds = 0`. Held rather than acted on
    /// here: the task is spawned by [`Pipeline::spawn_cache_cleanup`], never
    /// by the constructor.
    cleanup_interval: Option<Duration>,
    events: mpsc::Sender<Event>,
    /// Count of `QueryEvent`s dropped because the channel was full
    /// (ARCHITECTURE.md §Runtime Model: "a slow consumer drops events rather
    /// than back-pressuring the pipeline"). Exposed for p1-08's metrics.
    dropped_events: Arc<AtomicU64>,
}

impl<F: Forwarder> Pipeline<F> {
    pub fn new(
        rules: Arc<ListManager>,
        forwarder: F,
        blocking_ttl: u32,
        cache_config: &DnsCacheConfig,
        refresh_claim_lease: Duration,
        events: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            rules,
            policies: Arc::new(PolicyState::default()),
            forwarder,
            blocking_ttl,
            cache: Arc::new(DnsCache::new(cache_config, refresh_claim_lease)),
            swr: SwrPool::new(cache_config.swr_workers).map(Arc::new),
            cleanup_interval: match cache_config.cleanup_interval_seconds {
                0 => None,
                seconds => Some(Duration::from_secs(seconds as u64)),
            },
            events,
            dropped_events: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Attaches the shared policy state. Without it every client is judged
    /// under the default policy, which is the pre-p2-06 behaviour.
    pub fn with_policies(mut self, policies: Arc<PolicyState>) -> Self {
        self.policies = policies;
        self
    }

    /// Starts the stale-while-refresh pool, returning one handle per worker for
    /// the caller to abort on shutdown (empty when `swr_workers = 0`, or if the
    /// pool has already been started).
    ///
    /// Separate from [`Pipeline::new`] because a constructor must not spawn:
    /// the binary owns task lifetimes, and these belong in the same list as
    /// every other long-lived task — see `ListManager::spawn_scheduler` for the
    /// same split.
    pub fn spawn_swr_workers(&self) -> Vec<tokio::task::JoinHandle<()>> {
        match &self.swr {
            Some(swr) => swr.spawn_workers(Arc::clone(&self.cache), self.forwarder.clone()),
            None => Vec::new(),
        }
    }

    /// Stale-while-refresh counters for `/api/v1/telemetry`. All-zero when the pool is
    /// disabled, which is indistinguishable from an enabled pool that has had
    /// no stale hits yet — both mean "no refreshes are happening", which is
    /// what the counters are there to say.
    pub fn swr_stats(&self) -> crate::swr::SwrStats {
        self.swr.as_ref().map(|swr| swr.stats()).unwrap_or_default()
    }

    /// Starts the scheduled cache sweep, returning its handle for the caller to
    /// abort on shutdown (`None` when `cleanup_interval_seconds = 0`). Split
    /// from [`Pipeline::new`] for the same reason
    /// [`Pipeline::spawn_swr_workers`] is: the binary owns task lifetimes.
    ///
    /// The sweep removes only entries past the serve-stale window — it calls
    /// the same `clean(false)` the admin endpoint does, so stale entries stay
    /// exactly as servable as ADR-0005 needs them to be. There is no cheaper
    /// "expired only" variant to write; that is already what `false` means.
    pub fn spawn_cache_cleanup(&self) -> Option<tokio::task::JoinHandle<()>> {
        let interval = self.cleanup_interval?;
        let cache = Arc::clone(&self.cache);
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // The first tick fires immediately; skipping it means the sweep
            // does not run against a cache that has been up for milliseconds
            // and cannot hold anything expired yet.
            ticker.tick().await;
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let cache = Arc::clone(&cache);
                // On the blocking pool, not a DNS worker: `clean` is
                // synchronous and O(entries), so at a raised `max_entries` a
                // full walk is exactly the unbounded tail PERFORMANCE.md's
                // golden rule 8 keeps off the query path — the same treatment,
                // for the same reason, as the p2-07 memory pass. Shard lock
                // hold time is unaffected either way: one shard at a time, so a
                // concurrent resolve waits at most one shard's walk.
                let outcome = match tokio::task::spawn_blocking(move || cache.clean(false)).await {
                    Ok(outcome) => outcome,
                    // Only reachable if the sweep panicked. The scheduler
                    // outliving one bad sweep is worth more than the entries it
                    // would have removed; the next tick retries.
                    Err(err) => {
                        tracing::warn!(error = %err, "cache cleanup sweep failed");
                        continue;
                    }
                };
                // The whole `CacheClean`, every sweep — a line that reports only
                // what it removed cannot answer "did the sweep run and find
                // nothing?" versus "did the sweep not run?", which at default
                // settings is the question actually being asked. `stale_removed`
                // is in there despite being structurally always 0 for a
                // scheduled sweep: that zero is the on-device proof that
                // serve-stale entries (ADR-0005) are being left alone.
                //
                // Nothing-removed is the common case at default settings, and a
                // `debug` line every interval is one an operator can leave on;
                // an `info` one is 240 a day saying nothing, which on a RouterOS
                // log buffer costs real history.
                // `tracing` builds a callsite whose level is const, so the two
                // levels cannot collapse into one call — but they share one
                // field list rather than two copies that can drift apart.
                macro_rules! sweep {
                    ($level:ident) => {
                        tracing::$level!(
                            expired_removed = outcome.removed_expired,
                            stale_removed = outcome.removed_stale,
                            bytes_freed = outcome.freed_bytes,
                            entries_before = outcome.entries_before,
                            entries_after = outcome.entries_after,
                            duration_us = outcome.duration.as_micros() as u64,
                            "cache cleanup complete"
                        )
                    };
                }
                if outcome.removed_expired + outcome.removed_stale > 0 {
                    sweep!(info);
                } else {
                    sweep!(debug);
                }
            }
        }))
    }

    /// Cleanup counters for `/api/v1/telemetry`. Counts the admin
    /// `POST /api/v1/cache/clean` too — both go through one `clean`, which is
    /// what keeps the counters from disagreeing with what happened to the
    /// cache.
    pub fn cache_cleanup_stats(&self) -> crate::cache::CacheCleanupStats {
        self.cache.cleanup_stats()
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Admin-plane cache introspection (`GET /api/v1/cache`, reached through
    /// the binary's `CacheSource` adapter — `fah-api` never sees this crate).
    pub fn cache_stats(&self) -> crate::cache::CacheStats {
        self.cache.stats()
    }

    /// Admin-plane cache maintenance (`POST /api/v1/cache/clean`): drops
    /// expired entries, and stale-window entries too when `purge_stale`.
    pub fn cache_clean(&self, purge_stale: bool) -> crate::cache::CacheClean {
        self.cache.clean(purge_stale)
    }

    /// Handles one raw wire-format request received over `transport`.
    /// Returns `None` when there is nothing to send back: a malformed packet
    /// (dropped silently, never a panic — a malformed reply could feed a
    /// reflection attack, and no correctly decodable header means no id to
    /// answer to as safely) or a message that isn't a query
    /// (responses/updates aren't ours to answer).
    pub async fn handle(
        &self,
        raw: &[u8],
        client_ip: IpAddr,
        transport: Transport,
    ) -> Option<Vec<u8>> {
        // The dual-stack listener reports IPv4 peers as v4-mapped IPv6
        // (`::ffff:192.168.10.15`). Canonicalized once here — the single
        // entry point for every transport — so stats, client names, the events
        // socket and future client-scoped rules all see one address per
        // client. A handful of integer compares, free for the common case.
        let client_ip = client_ip.to_canonical();
        let request = match Message::from_vec(raw) {
            Ok(message) => message,
            Err(err) => {
                trace!(error = %err, "dropping malformed DNS packet");
                return None;
            }
        };
        let budget = match transport {
            Transport::Udp => response::max_udp_payload(&request),
            Transport::Tcp | Transport::Dot | Transport::Doh => u16::MAX,
        };

        if request.metadata.message_type != MessageType::Query {
            return None;
        }
        if request.metadata.op_code != OpCode::Query {
            return Some(response::encode_for_transport(
                &response::error(&request, ResponseCode::NotImp),
                budget,
            ));
        }
        let Some(query) = request.queries.first().cloned() else {
            return Some(response::encode_for_transport(
                &response::error(&request, ResponseCode::FormErr),
                budget,
            ));
        };

        let started = Instant::now();
        // Folded once here — the matcher is case-insensitive either way, and
        // the cache requires pre-lowercased keys so it never case-folds (an
        // allocation) on the per-query path.
        let mut domain = domain_of(query.name());
        domain.make_ascii_lowercase();
        let fah_qtype = to_fah_query_type(query.query_type());
        let matcher = self.rules.matcher();
        let active = self.policies.current();
        let ctx = matcher.context_for(client_ip, &active);
        let policy = active.id_of(ctx.policy);
        let decision = matcher.lookup_in(&domain, &fah_qtype, &ctx);

        let (verdict, local_response) = match decision {
            MatchDecision::Block(rule_ref) => {
                let rewrite = matcher.rewrite(rule_ref);
                let response = response::blocked(&request, &query, self.blocking_ttl, rewrite);
                (
                    Verdict::Block(matcher.decisive_rule(rule_ref)),
                    Some(response),
                )
            }
            MatchDecision::Allow(rule_ref) => {
                (Verdict::Allow(matcher.decisive_rule(rule_ref)), None)
            }
            MatchDecision::Pass => (Verdict::Pass, None),
        };
        // Release the ruleset handle before the upstream await: a slow
        // forward must not pin a swapped-out ruleset's memory for its
        // duration (the handle is an `Arc`, so holding it is safe — just
        // needlessly retentive). `active` goes with it — `ctx` is `Copy` and
        // borrows it, so it has to be dead by here.
        drop(active);
        drop(matcher);

        let resolved = match local_response {
            Some(blocked) => Resolved::blocked(blocked),
            None => {
                let resolved = self
                    .resolve(
                        &request,
                        &query,
                        &domain,
                        query.query_type(),
                        query.query_class(),
                    )
                    .await;
                // Blocked queries never reach the cache, so only resolves
                // count toward the admin cache stats' hit/miss figures.
                self.cache.note_lookup(resolved.cache_hit);
                resolved
            }
        };

        self.emit_event(
            FahQuery::new(domain, fah_qtype, client_ip, std::time::SystemTime::now()),
            verdict,
            started.elapsed(),
            &resolved,
            policy,
            transport,
        );

        Some(response::encode_for_transport(&resolved.response, budget))
    }

    /// The Allow/Pass path: cache first (ADR-0001 — the verdict already ran
    /// in `handle`, this only ever sees queries the Rule Engine let through),
    /// then the forwarder on a miss, then serve-stale when resolution fails —
    /// a transport error *or* the upstream answering `SERVFAIL` (RFC 8767's
    /// "failure" covers both; a reachable-but-broken recursive is the common
    /// outage shape). Returns a [`Resolved`] for the caller's `QueryEvent`.
    ///
    /// **A stale hit does not reach the forwarder at all** when
    /// stale-while-refresh is on (ADR-0005): it is answered from cache at
    /// cache-hit latency and a refresh job goes to the detached pool, which the
    /// client never waits on — [`StaleServe::FromSwr`]. With `[dns.cache]
    /// swr_workers = 0` the pool is absent and a stale entry falls through to
    /// the forward below, answering only if that fails
    /// ([`StaleServe::AfterForwardFailure`]) — the pre-ADR-0005 behaviour,
    /// kept intact. Metrics splits the two on exactly this distinction.
    async fn resolve(
        &self,
        request: &Message,
        query: &WireQuery,
        domain: &str,
        qtype: hickory_proto::rr::RecordType,
        qclass: hickory_proto::rr::DNSClass,
    ) -> Resolved {
        let key = self.cache.key(domain, qtype, qclass);
        // One lookup serves both paths. It claims the refresh only when there
        // is a pool to consume it, so a disabled pool takes no claim and leaves
        // the entry exactly as it found it.
        let cached = match &self.swr {
            Some(_) => self.cache.lookup_and_claim_refresh(&key),
            None => self.cache.lookup(&key),
        };
        match cached {
            Lookup::Fresh(answer, remaining_ttl) => {
                let response = response::from_cache(request, query, &answer, remaining_ttl);
                return Resolved::from_cache(response, None);
            }
            Lookup::Stale {
                answer,
                claimed_refresh,
            } => {
                if let Some(swr) = &self.swr {
                    if claimed_refresh {
                        swr.offer(&self.cache, key.clone());
                    } else {
                        swr.note_deduplicated();
                    }
                    let response = response::from_cache(request, query, &answer, STALE_SERVE_TTL);
                    return Resolved::from_cache(response, Some(StaleServe::FromSwr));
                }
            }
            Lookup::Miss => {}
        }

        match self.forwarder.forward(request).await {
            Ok(forwarded) => {
                let endpoint = forwarded.endpoint;
                let mut upstream_response = forwarded.message;
                if upstream_response.metadata.response_code == ResponseCode::ServFail {
                    // Non-claiming: this is already returning, so scheduling a
                    // refresh here would enqueue work nobody is waiting for
                    // against an upstream that just failed.
                    if let Lookup::Stale { answer, .. } = self.cache.lookup(&key) {
                        let response =
                            response::from_cache(request, query, &answer, STALE_SERVE_TTL);
                        return Resolved::from_cache(
                            response,
                            Some(StaleServe::AfterForwardFailure),
                        );
                    }
                    // `REFUSED` and friends are deliberate upstream policy,
                    // not an outage — they relay below without stale fallback.
                }
                // The client is answered either way; whether the answer was
                // cacheable only matters to the refresh path (ADR-0005).
                let _ = self.cache.store(&key, &upstream_response);
                // Wire ID is per-hop; always answer with the client's own.
                upstream_response.metadata.id = request.metadata.id;
                let outcome = match upstream_response.metadata.response_code {
                    ResponseCode::ServFail => AnswerOutcome::ServfailRelayed,
                    ResponseCode::Refused => AnswerOutcome::RefusedRelayed,
                    _ => AnswerOutcome::Answered,
                };
                Resolved::forwarded(upstream_response, outcome, endpoint)
            }
            Err(err) => {
                trace!(error = %err, "upstream forward failed");
                // Non-claiming, as above.
                if let Lookup::Stale { answer, .. } = self.cache.lookup(&key) {
                    let response = response::from_cache(request, query, &answer, STALE_SERVE_TTL);
                    return Resolved::from_cache(response, Some(StaleServe::AfterForwardFailure));
                }
                Resolved::synthesized_servfail(response::error(request, ResponseCode::ServFail))
            }
        }
    }

    fn emit_event(
        &self,
        query: FahQuery,
        verdict: Verdict,
        duration: std::time::Duration,
        resolved: &Resolved,
        policy: Option<Arc<str>>,
        transport: Transport,
    ) {
        let event = QueryEvent::new(
            query,
            verdict,
            duration,
            resolved.cache_hit,
            resolved.upstream_used,
            resolved.stale,
            transport.into(),
        )
        .under_policy(policy)
        .with_outcome(resolved.outcome, resolved.endpoint);
        // One channel for both pipelines (`fah_model::Event`), so the shed
        // figure stays a single number rather than two that cannot be added.
        if self.events.try_send(Event::dns(event)).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Unwraps the DNS half of the shared event channel. Every event this
    /// pipeline emits is `Event::Dns`; anything else is a wiring bug and
    /// should fail loudly rather than be skipped.
    fn dns_event(event: fah_model::Event) -> QueryEvent {
        match event {
            fah_model::Event::Dns(event) => *event,
            fah_model::Event::Http(_)
            | fah_model::Event::HttpsSni(_)
            | fah_model::Event::Https(_) => {
                panic!("the DNS pipeline emitted an HTTP event")
            }
        }
    }

    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::time::Duration;

    use fah_config::RulesConfig;
    use hickory_proto::op::Query as WireQuery;
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{Name, RData, Record, RecordType};

    use super::*;
    use crate::cache::DEFAULT_REFRESH_CLAIM_LEASE;
    use crate::upstream::ForwardOutcome;

    #[derive(Clone)]
    struct SpyForwarder {
        calls: Arc<AtomicU64>,
        outcome: ForwarderOutcome,
    }

    #[derive(Clone)]
    enum ForwarderOutcome {
        Ok,
        /// A positive answer with the given TTL, so the reply is cacheable and
        /// the entry can be aged into the stale window.
        Answer(u32),
        Err,
    }

    impl Forwarder for SpyForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.outcome {
                ForwarderOutcome::Ok => {
                    let mut response =
                        Message::response(request.metadata.id, request.metadata.op_code);
                    response.metadata.response_code = ResponseCode::NoError;
                    Ok(ForwardOutcome::new(response, 0))
                }
                ForwarderOutcome::Answer(ttl) => {
                    let mut response =
                        Message::response(request.metadata.id, request.metadata.op_code);
                    response.metadata.response_code = ResponseCode::NoError;
                    response.queries = request.queries.clone();
                    response.add_answer(Record::from_rdata(
                        request.queries[0].name().clone(),
                        ttl,
                        RData::A(A(Ipv4Addr::new(203, 0, 113, 1))),
                    ));
                    Ok(ForwardOutcome::new(response, 0))
                }
                ForwarderOutcome::Err => Err(std::io::Error::other("boom")),
            }
        }
    }

    /// Returns the manager plus its backing tempdir — the caller must keep
    /// the guard alive for the test's duration so the directory is cleaned
    /// up on drop instead of leaking into the OS temp dir.
    async fn manager_with_user_rules(rules_text: &str) -> (Arc<ListManager>, tempfile::TempDir) {
        let data_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            ListManager::new(
                &RulesConfig {
                    refresh_hours_default: 24,
                    lists: vec![],
                },
                data_dir.path().to_path_buf(),
            )
            .unwrap(),
        );
        manager.set_user_rules(rules_text.to_string()).await;
        (manager, data_dir)
    }

    fn encode_query(name: &str, qtype: RecordType) -> Vec<u8> {
        let mut message = Message::query();
        message.add_query(WireQuery::query(Name::from_str(name).unwrap(), qtype));
        message.to_vec().unwrap()
    }

    fn client_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))
    }

    #[tokio::test]
    async fn blocked_domain_never_touches_the_forwarder() {
        let (rules, _data_dir) = manager_with_user_rules("||ads.example.com^\n").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("ads.example.com.", RecordType::A);
        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();

        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(response.answers.len(), 1);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "blocked queries must never reach the forwarder"
        );
    }

    #[tokio::test]
    async fn passed_domain_is_forwarded() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("example.com.", RecordType::A);
        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();

        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.verdict, Verdict::Pass);
        assert!(event.upstream_used);
        assert!(!event.cache_hit);
    }

    #[tokio::test]
    async fn v4_mapped_client_address_is_canonicalized_in_events() {
        // What a dual-stack `::` listener hands us for an IPv4 peer. The
        // event must carry plain IPv4, or the same phone would appear as two
        // different clients depending on which stack its query came in on.
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let mapped: IpAddr = "::ffff:192.168.10.15".parse().unwrap();
        let raw = encode_query("example.com.", RecordType::A);
        pipeline.handle(&raw, mapped, Transport::Udp).await.unwrap();

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(
            event.query.client_ip,
            "192.168.10.15".parse::<IpAddr>().unwrap()
        );
    }

    #[tokio::test]
    async fn forwarder_failure_yields_servfail_not_a_panic() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Err,
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("example.com.", RecordType::A);
        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn malformed_packet_is_dropped_without_panic() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        for garbage in fuzz_corpus() {
            let result = pipeline.handle(&garbage, client_ip(), Transport::Tcp).await;
            // Either dropped (None) or answered with a well-formed error —
            // both are fine; a panic is the only failure this test catches.
            if let Some(reply) = result {
                let _ = Message::from_vec(&reply);
            }
        }
    }

    fn fuzz_corpus() -> Vec<Vec<u8>> {
        vec![
            vec![],
            vec![0u8],
            vec![0u8; 11],  // one short of a full header
            vec![0xFF; 12], // header-shaped garbage, no valid question
            vec![0xFF; 512],
            {
                // A truncated-mid-name packet: valid header, question length
                // claims a label longer than the buffer actually holds.
                let mut bytes = encode_query("a.example.com.", RecordType::A);
                bytes.truncate(bytes.len() - 3);
                bytes
            },
        ]
    }

    #[tokio::test]
    async fn dropped_events_counter_increments_when_channel_is_full() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, _rx) = mpsc::channel(1);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );
        // _rx is held (not dropped) but never polled: capacity 1 fills, then
        // every subsequent try_send fails until something drains it.
        let raw = encode_query("example.com.", RecordType::A);
        for _ in 0..5 {
            pipeline.handle(&raw, client_ip(), Transport::Tcp).await;
        }
        assert!(pipeline.dropped_events() > 0);
    }

    #[tokio::test]
    async fn non_query_message_type_is_ignored() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Ok,
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let mut response = Message::response(1, OpCode::Query);
        response.add_query(WireQuery::query(
            Name::from_str("example.com.").unwrap(),
            RecordType::A,
        ));
        let raw = response.to_vec().unwrap();

        assert!(pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn udp_truncates_an_oversized_reply_while_tcp_does_not() {
        // A forwarder that answers with many records — bigger than any
        // request's default 512-byte no-EDNS budget.
        #[derive(Clone)]
        struct FatForwarder;
        impl Forwarder for FatForwarder {
            async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
                let mut response = Message::response(request.metadata.id, request.metadata.op_code);
                response.metadata.response_code = ResponseCode::NoError;
                for i in 0..40 {
                    response.add_answer(hickory_proto::rr::Record::from_rdata(
                        Name::from_str(&format!("padding{i}.example.com.")).unwrap(),
                        10,
                        hickory_proto::rr::RData::A(hickory_proto::rr::rdata::A(Ipv4Addr::new(
                            1, 2, 3, 4,
                        ))),
                    ));
                }
                Ok(ForwardOutcome::new(response, 0))
            }
        }

        let (rules, _data_dir) = manager_with_user_rules("").await;
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            FatForwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );
        let raw = encode_query("example.com.", RecordType::A);

        let udp_reply = pipeline
            .handle(&raw, client_ip(), Transport::Udp)
            .await
            .unwrap();
        let udp_decoded = Message::from_vec(&udp_reply).unwrap();
        assert!(
            udp_decoded.metadata.truncation,
            "oversized UDP reply must set TC"
        );
        assert!(udp_decoded.answers.is_empty());

        let tcp_reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let tcp_decoded = Message::from_vec(&tcp_reply).unwrap();
        assert!(
            !tcp_decoded.metadata.truncation,
            "TCP has no truncation ceiling"
        );
        assert_eq!(tcp_decoded.answers.len(), 40);
    }

    #[derive(Clone)]
    struct AnswerOnceForwarder {
        calls: Arc<AtomicU64>,
    }

    impl Forwarder for AnswerOnceForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.response_code = ResponseCode::NoError;
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                300,
                RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
            ));
            Ok(ForwardOutcome::new(response, 0))
        }
    }

    #[tokio::test]
    async fn second_identical_query_is_served_from_cache_without_forwarding() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = AnswerOnceForwarder {
            calls: calls.clone(),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let first_event = dns_event(rx.try_recv().unwrap());
        assert!(!first_event.cache_hit);

        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let second_event = dns_event(rx.try_recv().unwrap());
        assert!(second_event.cache_hit);
        assert_eq!(second_event.stale, None);

        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "second query must be answered from the cache, not forwarded again"
        );
    }

    #[tokio::test]
    async fn cache_stats_count_resolves_but_not_blocked_queries() {
        let (rules, _data_dir) = manager_with_user_rules("||ads.example.com^\n").await;
        let forwarder = AnswerOnceForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        // Blocked: never touches the cache, counts neither hit nor miss.
        let blocked = encode_query("ads.example.com.", RecordType::A);
        pipeline.handle(&blocked, client_ip(), Transport::Tcp).await;

        // Miss (forwarded + stored), then a fresh hit.
        let raw = encode_query("example.com.", RecordType::A);
        pipeline.handle(&raw, client_ip(), Transport::Tcp).await;
        pipeline.handle(&raw, client_ip(), Transport::Tcp).await;

        let stats = pipeline.cache_stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.fresh, 1);

        let outcome = pipeline.cache_clean(false);
        assert_eq!(outcome.entries_before, 1);
        assert_eq!(outcome.entries_after, 1, "a fresh entry survives a clean");
    }

    #[derive(Clone)]
    struct FailAfterFirstForwarder {
        calls: Arc<AtomicU64>,
    }

    impl Forwarder for FailAfterFirstForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            let n = self.calls.fetch_add(1, Ordering::Relaxed);
            if n > 0 {
                return Err(std::io::Error::other("upstream down"));
            }
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.response_code = ResponseCode::NoError;
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                1,
                RData::A(A(Ipv4Addr::new(1, 2, 3, 4))),
            ));
            Ok(ForwardOutcome::new(response, 0))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_failure_serves_stale_cache_entry_when_available() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = FailAfterFirstForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, mut rx) = mpsc::channel(8);
        // Pool off, or the stale entry is served by SWR before the forwarder is
        // ever asked — which is not the path this test is named for.
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(0),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let _ = dns_event(rx.try_recv().unwrap());

        // 1s TTL now expired, still inside the 24h stale window.
        tokio::time::advance(std::time::Duration::from_secs(2)).await;

        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
        assert_eq!(decoded.answers.len(), 1);

        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.cache_hit);
        assert_eq!(
            event.stale,
            Some(StaleServe::AfterForwardFailure),
            "the forward was attempted and failed — this duration carries its timeout"
        );
        assert!(!event.upstream_used);
    }

    /// Answers once, then `SERVFAIL`s — the reachable-but-broken upstream
    /// shape RFC 8767 counts as a resolution failure.
    #[derive(Clone)]
    struct ServfailAfterFirstForwarder {
        calls: Arc<AtomicU64>,
    }

    impl Forwarder for ServfailAfterFirstForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            let n = self.calls.fetch_add(1, Ordering::Relaxed);
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            if n > 0 {
                response.metadata.response_code = ResponseCode::ServFail;
                return Ok(ForwardOutcome::new(response, 0));
            }
            response.metadata.response_code = ResponseCode::NoError;
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                1,
                RData::A(A(Ipv4Addr::new(1, 2, 3, 4))),
            ));
            Ok(ForwardOutcome::new(response, 0))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_servfail_serves_stale_cache_entry_when_available() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = ServfailAfterFirstForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, mut rx) = mpsc::channel(8);
        // Pool off, as above: the SERVFAIL fallback lives past the forward.
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(0),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let _ = dns_event(rx.try_recv().unwrap());

        // 1s TTL now expired; the upstream answers SERVFAIL from here on.
        tokio::time::advance(std::time::Duration::from_secs(2)).await;

        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(
            decoded.metadata.response_code,
            ResponseCode::NoError,
            "stale entry must be served instead of relaying the SERVFAIL"
        );
        assert_eq!(decoded.answers.len(), 1);

        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.cache_hit);
        assert_eq!(event.stale, Some(StaleServe::AfterForwardFailure));
    }

    #[tokio::test]
    async fn mixed_case_repeat_query_hits_the_cache() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = AnswerOnceForwarder {
            calls: calls.clone(),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let _ = dns_event(rx.try_recv().unwrap());

        pipeline
            .handle(
                &encode_query("EXAMPLE.Com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.cache_hit, "DNS names are case-insensitive (RFC 1035)");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    // ── stale-while-refresh (ADR-0005) ────────────────────────────────────

    const SWR_TTL: u32 = 10;

    fn swr_cache_config(swr_workers: u32) -> DnsCacheConfig {
        DnsCacheConfig {
            swr_workers,
            ..DnsCacheConfig::default()
        }
    }

    /// Warms one cacheable entry, then ages it past its TTL and into the stale
    /// window. Returns the forwarder's call count so far, which is the number a
    /// test compares against — a stale hit must not add to it.
    async fn warm_then_age<F: Forwarder>(pipeline: &Pipeline<F>) {
        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        tokio::time::advance(Duration::from_secs(u64::from(SWR_TTL) + 1)).await;
    }

    /// The whole point of ADR-0005: a stale entry is answered from cache, and
    /// the client's request never reaches an upstream. Before it, this second
    /// query cost a full forward.
    #[tokio::test(start_paused = true)]
    async fn a_stale_hit_answers_from_cache_without_forwarding() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(3),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;
        let _warm_event = dns_event(rx.try_recv().unwrap());
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the request path must not forward on a stale hit"
        );
        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.answers.len(), 1);
        assert_eq!(
            response.answers[0].ttl,
            crate::cache::STALE_SERVE_TTL,
            "a stale answer carries the short retry-soon TTL"
        );

        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.cache_hit);
        assert_eq!(
            event.stale,
            Some(StaleServe::FromSwr),
            "the forwarder was never asked, so this must not be timed as a forward"
        );
        assert!(!event.upstream_used);

        // The refresh was queued, exactly once, and nothing consumed it — no
        // workers were started, which is what proves the query path does not
        // depend on them.
        let stats = pipeline.swr_stats();
        assert_eq!(stats.enqueued, 1);
        assert_eq!(stats.deduplicated, 0);
        assert_eq!(stats.dropped, 0);
    }

    /// The deduplication requirement end to end: many simultaneous requests for
    /// one stale name must schedule one refresh between them, not one each.
    #[tokio::test(start_paused = true)]
    async fn many_stale_hits_for_one_name_schedule_a_single_refresh() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, _rx) = mpsc::channel(256);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(3),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;

        const HITS: u64 = 50;
        for _ in 0..HITS {
            pipeline
                .handle(
                    &encode_query("example.com.", RecordType::A),
                    client_ip(),
                    Transport::Tcp,
                )
                .await
                .unwrap();
        }

        let stats = pipeline.swr_stats();
        assert_eq!(stats.enqueued, 1, "{HITS} stale hits must enqueue one job");
        assert_eq!(stats.deduplicated, HITS - 1);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "and none of them may forward"
        );
    }

    /// `swr_workers = 0` must reproduce the pre-ADR-0005 behaviour exactly: the
    /// stale entry does not answer until a forward has actually failed.
    #[tokio::test(start_paused = true)]
    async fn with_the_pool_disabled_a_stale_hit_forwards_as_before() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(0),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;
        let _warm_event = dns_event(rx.try_recv().unwrap());

        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "with no pool, a stale entry still goes to the upstream first"
        );
        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.upstream_used);
        assert_eq!(event.stale, None);
        assert_eq!(
            pipeline.swr_stats(),
            crate::swr::SwrStats::default(),
            "a disabled pool reports nothing"
        );
    }

    /// With the pool off, the failed-forward fallback must still serve stale —
    /// the RFC 8767 behaviour that predates this change and is what
    /// `swr_workers = 0` exists to preserve.
    #[tokio::test(start_paused = true)]
    async fn with_the_pool_disabled_a_failed_forward_still_serves_stale() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let mut pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(0),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;
        let _warm_event = dns_event(rx.try_recv().unwrap());

        // The upstream dies after the entry was cached.
        pipeline.forwarder.outcome = ForwarderOutcome::Err;

        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(response.answers.len(), 1);
        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.stale, Some(StaleServe::AfterForwardFailure));
        assert!(event.cache_hit);
    }

    /// A refresh that reaches a worker replaces the entry, and the next query is
    /// a plain fresh hit — the loop closing without the client ever waiting.
    #[derive(Clone)]
    struct StallAfterFirstForwarder {
        inner: SpyForwarder,
        started: Arc<AtomicU64>,
        stall: Duration,
    }

    impl Forwarder for StallAfterFirstForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            if self.started.fetch_add(1, Ordering::Relaxed) > 0 {
                tokio::time::sleep(self.stall).await;
            }
            self.inner.forward(request).await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_walk_longer_than_the_default_lease_is_covered_by_a_derived_one() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let started = Arc::new(AtomicU64::new(0));
        let forwarder = StallAfterFirstForwarder {
            inner: SpyForwarder {
                calls: calls.clone(),
                outcome: ForwarderOutcome::Answer(SWR_TTL),
            },
            started: started.clone(),
            stall: Duration::from_secs(30),
        };
        let lease = Duration::from_secs(10);
        assert!(lease > DEFAULT_REFRESH_CLAIM_LEASE);
        let (tx, _rx) = mpsc::channel(16);
        let pipeline = Pipeline::new(rules, forwarder, 10, &swr_cache_config(1), lease, tx);
        let workers = pipeline.spawn_swr_workers();

        warm_then_age(&pipeline).await;
        let query = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&query, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        while started.load(Ordering::Relaxed) < 2 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pipeline.swr_stats().enqueued, 1);

        tokio::time::advance(DEFAULT_REFRESH_CLAIM_LEASE + Duration::from_secs(1)).await;
        for _ in 0..3 {
            pipeline
                .handle(&query, client_ip(), Transport::Tcp)
                .await
                .unwrap();
        }
        let stats = pipeline.swr_stats();
        assert_eq!(
            (stats.enqueued, stats.deduplicated),
            (1, 3),
            "past the 5 s default but inside the derived lease, the walk in flight still owns the claim"
        );

        tokio::time::advance(lease).await;
        pipeline
            .handle(&query, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        assert_eq!(
            pipeline.swr_stats().enqueued,
            2,
            "once the derived lease lapses the claim is reclaimable again"
        );
        assert_eq!(
            (
                started.load(Ordering::Relaxed),
                calls.load(Ordering::Relaxed)
            ),
            (2, 1),
            "the single worker is still inside the first walk; the second job waits in the queue"
        );

        for worker in workers {
            worker.abort();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_queued_refresh_is_consumed_and_makes_the_entry_fresh_again() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(1),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );
        let workers = pipeline.spawn_swr_workers();
        assert_eq!(workers.len(), 1);

        warm_then_age(&pipeline).await;
        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while pipeline.swr_stats().completed == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the queued refresh never ran"
            );
            tokio::task::yield_now().await;
        }
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "one warm-up forward plus one background refresh"
        );

        // Same query again: fresh now, so no forward and no new job.
        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(pipeline.swr_stats().enqueued, 1);

        for worker in workers {
            worker.abort();
        }
    }

    // ── scheduled cache cleanup ───────────────────────────────────────────

    /// `swr_workers` off by default here: these tests are about the cleaner,
    /// and a live pool would refresh the very entries a sweep is meant to find.
    fn cleanup_cache_config(cleanup_interval_seconds: u32) -> DnsCacheConfig {
        DnsCacheConfig {
            swr_workers: 0,
            cleanup_interval_seconds,
            ..DnsCacheConfig::default()
        }
    }

    /// Spins until `predicate` holds, driving the paused clock forward by
    /// `advance` each turn so the sweep's `interval` actually fires — tokio's
    /// auto-advance only kicks in when the runtime is *idle*, and a spin loop
    /// never is. `Duration::ZERO` means "just yield", for waits whose progress
    /// comes from another task rather than from time.
    ///
    /// Bounded by real wall time, not by the test clock, so a scheduler that
    /// never fires fails instead of hanging.
    async fn wait_for(advance: Duration, label: &str, mut predicate: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline, "{label}");
            if advance > Duration::ZERO {
                tokio::time::advance(advance).await;
            }
            tokio::task::yield_now().await;
        }
    }

    /// The sweep interval every cleanup test below runs at.
    const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

    /// Nothing asks for this: no query, no API call, no eviction pressure. An
    /// entry past its stale window simply stops being resident.
    #[tokio::test(start_paused = true)]
    async fn the_scheduled_sweep_removes_dead_entries_unprompted() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, _rx) = mpsc::channel(256);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &cleanup_cache_config(60),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;
        assert_eq!(pipeline.cache_stats().entries, 1);

        // Past the stale window: now genuinely dead, and nothing but the
        // sweep is going to notice.
        tokio::time::advance(crate::cache::MAX_STALE).await;
        let cleanup = pipeline
            .spawn_cache_cleanup()
            .expect("interval is non-zero");

        wait_for(
            CLEANUP_INTERVAL,
            "the sweep never removed the dead entry",
            || pipeline.cache_cleanup_stats().entries_removed == 1,
        )
        .await;

        assert_eq!(pipeline.cache_stats().entries, 0);
        let stats = pipeline.cache_cleanup_stats();
        assert!(stats.runs >= 1);
        assert!(stats.bytes_freed > 0, "a removed entry freed no heap");
        cleanup.abort();
    }

    /// The requirement the whole feature is gated on. With SWR, the cache has
    /// three states, and the cleaner owns only the last one: sweeping a stale
    /// entry would delete precisely the copy ADR-0005 exists to serve, turning
    /// a cache-hit into an upstream round trip.
    #[tokio::test(start_paused = true)]
    async fn the_scheduled_sweep_never_touches_a_serve_stale_entry() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, mut rx) = mpsc::channel(256);
        let mut pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &cleanup_cache_config(60),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        // Expired but inside the stale window, and left there.
        warm_then_age(&pipeline).await;
        let _warm_event = dns_event(rx.try_recv().unwrap());
        let cleanup = pipeline
            .spawn_cache_cleanup()
            .expect("interval is non-zero");

        // Several sweeps, so this cannot pass by the cleaner simply not having
        // run yet.
        wait_for(CLEANUP_INTERVAL, "the sweep never ran", || {
            pipeline.cache_cleanup_stats().runs >= 3
        })
        .await;

        let stats = pipeline.cache_cleanup_stats();
        assert_eq!(stats.entries_removed, 0);
        assert_eq!(stats.bytes_freed, 0);
        assert_eq!(pipeline.cache_stats().stale, 1);

        // And it is still *usable*, not merely present. With the pool off, the
        // way to reach the stale copy is the pre-ADR-0005 fallback: kill the
        // upstream and the entry has to answer. That is the outage this
        // insurance exists for, and it is exactly what a sweep would have
        // destroyed.
        pipeline.forwarder.outcome = ForwarderOutcome::Err;
        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(response.answers.len(), 1);
        let event = dns_event(rx.try_recv().unwrap());
        assert!(event.cache_hit);
        assert_eq!(event.stale, Some(StaleServe::AfterForwardFailure));

        cleanup.abort();
    }

    /// The two background systems run against the same shards. A sweep must not
    /// strand an entry a worker has claimed, and the refresh must still land.
    #[tokio::test(start_paused = true)]
    async fn a_sweep_and_a_pending_swr_refresh_do_not_fight() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = SpyForwarder {
            calls: calls.clone(),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, _rx) = mpsc::channel(256);
        let config = DnsCacheConfig {
            swr_workers: 1,
            cleanup_interval_seconds: 60,
            ..DnsCacheConfig::default()
        };
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &config,
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );
        let workers = pipeline.spawn_swr_workers();
        let cleanup = pipeline
            .spawn_cache_cleanup()
            .expect("interval is non-zero");

        warm_then_age(&pipeline).await;
        // A stale hit: served from cache, refresh claimed and queued.
        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        // No clock advance: the refresh is driven by the worker task, and
        // pushing time forward here could age the entry out from under the
        // very interaction being tested.
        wait_for(Duration::ZERO, "the refresh never completed", || {
            pipeline.swr_stats().completed == 1
        })
        .await;

        assert_eq!(
            pipeline.cache_cleanup_stats().entries_removed,
            0,
            "the cleaner removed an entry SWR was refreshing"
        );
        assert_eq!(pipeline.cache_stats().entries, 1);

        cleanup.abort();
        for worker in workers {
            worker.abort();
        }
    }

    /// `0` is the documented off switch, and off has to mean no task at all —
    /// not a task that ticks and does nothing.
    #[tokio::test]
    async fn cleanup_interval_of_zero_spawns_no_task() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &cleanup_cache_config(0),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        assert!(pipeline.spawn_cache_cleanup().is_none());
        assert_eq!(
            pipeline.cache_cleanup_stats(),
            crate::cache::CacheCleanupStats::default(),
            "no scheduler means no sweeps to count"
        );
    }

    #[derive(Clone)]
    struct RcodeForwarder {
        code: ResponseCode,
        endpoint: u8,
    }

    impl Forwarder for RcodeForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.queries = request.queries.clone();
            response.metadata.response_code = self.code;
            Ok(ForwardOutcome::new(response, self.endpoint))
        }
    }

    #[tokio::test]
    async fn forward_failure_without_a_stale_entry_is_a_synthesized_servfail() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Err,
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        assert_eq!(
            Message::from_vec(&reply).unwrap().metadata.response_code,
            ResponseCode::ServFail
        );

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.answer, AnswerOutcome::ServfailSynthesized);
        assert_eq!(
            event.endpoint, None,
            "nothing answered, so no endpoint may be named"
        );
        assert_eq!(event.verdict, Verdict::Pass);
        assert!(!event.cache_hit);
        assert!(!event.upstream_used);
    }

    #[tokio::test]
    async fn a_relayed_servfail_is_distinguishable_and_names_its_endpoint() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            RcodeForwarder {
                code: ResponseCode::ServFail,
                endpoint: 0,
            },
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        assert_eq!(
            Message::from_vec(&reply).unwrap().metadata.response_code,
            ResponseCode::ServFail
        );

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.answer, AnswerOutcome::ServfailRelayed);
        assert_ne!(event.answer, AnswerOutcome::ServfailSynthesized);
        assert_eq!(event.endpoint, Some(0));
        assert!(event.upstream_used);
        assert!(!event.cache_hit);
    }

    #[tokio::test]
    async fn a_relayed_refused_is_counted_apart_and_reaches_the_client_unchanged() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            RcodeForwarder {
                code: ResponseCode::Refused,
                endpoint: 2,
            },
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        let reply = pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        assert_eq!(
            Message::from_vec(&reply).unwrap().metadata.response_code,
            ResponseCode::Refused
        );

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.answer, AnswerOutcome::RefusedRelayed);
        assert_eq!(event.endpoint, Some(2));
        assert!(event.upstream_used);
    }

    #[tokio::test]
    async fn a_block_and_a_cache_hit_name_no_endpoint() {
        let (rules, _data_dir) = manager_with_user_rules("||ads.example.com^").await;
        let forwarder = AnswerOnceForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &DnsCacheConfig::default(),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        pipeline
            .handle(
                &encode_query("ads.example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let blocked = dns_event(rx.try_recv().unwrap());
        assert_eq!(blocked.answer, AnswerOutcome::Answered);
        assert_eq!(blocked.endpoint, None);

        for _ in 0..2 {
            pipeline
                .handle(
                    &encode_query("example.com.", RecordType::A),
                    client_ip(),
                    Transport::Tcp,
                )
                .await
                .unwrap();
        }
        let forwarded = dns_event(rx.try_recv().unwrap());
        assert_eq!(forwarded.answer, AnswerOutcome::Answered);
        assert_eq!(forwarded.endpoint, Some(0));

        let cached = dns_event(rx.try_recv().unwrap());
        assert!(cached.cache_hit);
        assert_eq!(cached.answer, AnswerOutcome::Answered);
        assert_eq!(
            cached.endpoint, None,
            "no upstream was asked, so none may be named"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_swr_stale_serve_names_no_endpoint() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = SpyForwarder {
            calls: Arc::new(AtomicU64::new(0)),
            outcome: ForwarderOutcome::Answer(SWR_TTL),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(
            rules,
            forwarder,
            10,
            &swr_cache_config(3),
            DEFAULT_REFRESH_CLAIM_LEASE,
            tx,
        );

        warm_then_age(&pipeline).await;
        let warmed = dns_event(rx.try_recv().unwrap());
        assert_eq!(warmed.endpoint, Some(0));

        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();

        let event = dns_event(rx.try_recv().unwrap());
        assert_eq!(event.stale, Some(StaleServe::FromSwr));
        assert_eq!(event.answer, AnswerOutcome::Answered);
        assert_eq!(event.endpoint, None);
    }
}
