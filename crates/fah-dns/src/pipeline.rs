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
use std::time::Instant;

use fah_config::DnsCacheConfig;
use fah_model::{Query as FahQuery, QueryEvent, Verdict};
use fah_rules::{ListManager, MatchDecision};
use hickory_proto::op::{Message, MessageType, OpCode, Query as WireQuery, ResponseCode};
use tokio::sync::mpsc;
use tracing::trace;

use crate::cache::{DnsCache, Lookup, STALE_SERVE_TTL};
use crate::qtype::{domain_of, to_fah_query_type};
use crate::response;
use crate::upstream::Forwarder;

/// Which listener received the request — decides the reply's size ceiling:
/// UDP truncates to the request's own EDNS payload size (or 512 without
/// EDNS), TCP has no practical ceiling (RFC 1035's 65535-byte length prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
}

#[derive(Clone)]
pub struct Pipeline<F: Forwarder> {
    rules: Arc<ListManager>,
    forwarder: F,
    blocking_ttl: u32,
    cache: Arc<DnsCache>,
    events: mpsc::Sender<QueryEvent>,
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
        events: mpsc::Sender<QueryEvent>,
    ) -> Self {
        Self {
            rules,
            forwarder,
            blocking_ttl,
            cache: Arc::new(DnsCache::new(cache_config)),
            events,
            dropped_events: Arc::new(AtomicU64::new(0)),
        }
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
        // entry point for every transport — so stats, client names, the
        // query log and future client-scoped rules all see one address per
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
            Transport::Tcp => u16::MAX,
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
        let decision = matcher.lookup(&domain, &fah_qtype);

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
        // needlessly retentive).
        drop(matcher);

        let (response_message, cache_hit, upstream_used, stale) = match local_response {
            Some(blocked) => (blocked, false, false, false),
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
                self.cache.note_lookup(resolved.1);
                resolved
            }
        };

        self.emit_event(
            FahQuery::new(domain, fah_qtype, client_ip, std::time::SystemTime::now()),
            verdict,
            started.elapsed(),
            cache_hit,
            upstream_used,
            stale,
        );

        Some(response::encode_for_transport(&response_message, budget))
    }

    /// The Allow/Pass path: cache first (ADR-0001 — the verdict already ran
    /// in `handle`, this only ever sees queries the Rule Engine let through),
    /// then the forwarder on a miss/stale entry, then RFC 8767 serve-stale
    /// when resolution fails — a transport error *or* the upstream answering
    /// `SERVFAIL` (the RFC's "failure" covers both; a reachable-but-broken
    /// recursive is the common outage shape). Returns `(reply, cache_hit,
    /// upstream_used, stale)` for the caller's `QueryEvent`.
    async fn resolve(
        &self,
        request: &Message,
        query: &WireQuery,
        domain: &str,
        qtype: hickory_proto::rr::RecordType,
        qclass: hickory_proto::rr::DNSClass,
    ) -> (Message, bool, bool, bool) {
        let key = self.cache.key(domain, qtype, qclass);
        if let Lookup::Fresh(answer, remaining_ttl) = self.cache.lookup(&key) {
            let response = response::from_cache(request, query, &answer, remaining_ttl);
            return (response, true, false, false);
        }

        match self.forwarder.forward(request).await {
            Ok(mut upstream_response) => {
                if upstream_response.metadata.response_code == ResponseCode::ServFail {
                    if let Lookup::Stale(answer) = self.cache.lookup(&key) {
                        let response =
                            response::from_cache(request, query, &answer, STALE_SERVE_TTL);
                        return (response, true, false, true);
                    }
                    // `REFUSED` and friends are deliberate upstream policy,
                    // not an outage — they relay below without stale fallback.
                }
                self.cache.store(&key, &upstream_response);
                // Wire ID is per-hop; always answer with the client's own.
                upstream_response.metadata.id = request.metadata.id;
                (upstream_response, false, true, false)
            }
            Err(err) => {
                trace!(error = %err, "upstream forward failed");
                if let Lookup::Stale(answer) = self.cache.lookup(&key) {
                    let response = response::from_cache(request, query, &answer, STALE_SERVE_TTL);
                    return (response, true, false, true);
                }
                (
                    response::error(request, ResponseCode::ServFail),
                    false,
                    false,
                    false,
                )
            }
        }
    }

    fn emit_event(
        &self,
        query: FahQuery,
        verdict: Verdict,
        duration: std::time::Duration,
        cache_hit: bool,
        upstream_used: bool,
        stale: bool,
    ) {
        let event = QueryEvent::new(query, verdict, duration, cache_hit, upstream_used, stale);
        if self.events.try_send(event).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    use fah_config::RulesConfig;
    use hickory_proto::op::Query as WireQuery;
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{Name, RData, Record, RecordType};

    use super::*;

    #[derive(Clone)]
    struct SpyForwarder {
        calls: Arc<AtomicU64>,
        outcome: ForwarderOutcome,
    }

    #[derive(Clone)]
    enum ForwarderOutcome {
        Ok,
        Err,
    }

    impl Forwarder for SpyForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<Message> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.outcome {
                ForwarderOutcome::Ok => {
                    let mut response =
                        Message::response(request.metadata.id, request.metadata.op_code);
                    response.metadata.response_code = ResponseCode::NoError;
                    Ok(response)
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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        let raw = encode_query("example.com.", RecordType::A);
        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();

        let response = Message::from_vec(&reply).unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        let event = rx.try_recv().unwrap();
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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        let mapped: IpAddr = "::ffff:192.168.10.15".parse().unwrap();
        let raw = encode_query("example.com.", RecordType::A);
        pipeline.handle(&raw, mapped, Transport::Udp).await.unwrap();

        let event = rx.try_recv().unwrap();
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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);
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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

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
            async fn forward(&self, request: &Message) -> std::io::Result<Message> {
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
                Ok(response)
            }
        }

        let (rules, _data_dir) = manager_with_user_rules("").await;
        let (tx, _rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(rules, FatForwarder, 10, &DnsCacheConfig::default(), tx);
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
        async fn forward(&self, request: &Message) -> std::io::Result<Message> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.response_code = ResponseCode::NoError;
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                300,
                RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
            ));
            Ok(response)
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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let first_event = rx.try_recv().unwrap();
        assert!(!first_event.cache_hit);

        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let second_event = rx.try_recv().unwrap();
        assert!(second_event.cache_hit);
        assert!(!second_event.stale);

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
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

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
        async fn forward(&self, request: &Message) -> std::io::Result<Message> {
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
            Ok(response)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_failure_serves_stale_cache_entry_when_available() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = FailAfterFirstForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let _ = rx.try_recv().unwrap();

        // 1s TTL now expired, still inside the 24h stale window.
        tokio::time::advance(std::time::Duration::from_secs(2)).await;

        let reply = pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
        assert_eq!(decoded.answers.len(), 1);

        let event = rx.try_recv().unwrap();
        assert!(event.cache_hit);
        assert!(event.stale);
        assert!(!event.upstream_used);
    }

    /// Answers once, then `SERVFAIL`s — the reachable-but-broken upstream
    /// shape RFC 8767 counts as a resolution failure.
    #[derive(Clone)]
    struct ServfailAfterFirstForwarder {
        calls: Arc<AtomicU64>,
    }

    impl Forwarder for ServfailAfterFirstForwarder {
        async fn forward(&self, request: &Message) -> std::io::Result<Message> {
            let n = self.calls.fetch_add(1, Ordering::Relaxed);
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            if n > 0 {
                response.metadata.response_code = ResponseCode::ServFail;
                return Ok(response);
            }
            response.metadata.response_code = ResponseCode::NoError;
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                1,
                RData::A(A(Ipv4Addr::new(1, 2, 3, 4))),
            ));
            Ok(response)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_servfail_serves_stale_cache_entry_when_available() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let forwarder = ServfailAfterFirstForwarder {
            calls: Arc::new(AtomicU64::new(0)),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        let raw = encode_query("example.com.", RecordType::A);
        pipeline
            .handle(&raw, client_ip(), Transport::Tcp)
            .await
            .unwrap();
        let _ = rx.try_recv().unwrap();

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

        let event = rx.try_recv().unwrap();
        assert!(event.cache_hit);
        assert!(event.stale);
    }

    #[tokio::test]
    async fn mixed_case_repeat_query_hits_the_cache() {
        let (rules, _data_dir) = manager_with_user_rules("").await;
        let calls = Arc::new(AtomicU64::new(0));
        let forwarder = AnswerOnceForwarder {
            calls: calls.clone(),
        };
        let (tx, mut rx) = mpsc::channel(8);
        let pipeline = Pipeline::new(rules, forwarder, 10, &DnsCacheConfig::default(), tx);

        pipeline
            .handle(
                &encode_query("example.com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let _ = rx.try_recv().unwrap();

        pipeline
            .handle(
                &encode_query("EXAMPLE.Com.", RecordType::A),
                client_ip(),
                Transport::Tcp,
            )
            .await
            .unwrap();
        let event = rx.try_recv().unwrap();
        assert!(event.cache_hit, "DNS names are case-insensitive (RFC 1035)");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
