//! DoT/DoH transports (ARCHITECTURE.md §Upstreams: Hickory + rustls): one
//! lazily-connected, persistent, multiplexed [`DnsExchange`] per upstream.
//! Reconnection happens only after a transport error — never per query — so
//! steady-state traffic performs zero TLS handshakes (p1-06 acceptance;
//! [`ExchangeConn::handshakes`] counts connects to prove it).

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hickory_net::h2::HttpsClientStream;
use hickory_net::runtime::TokioRuntimeProvider;
use hickory_net::tls::tls_exchange;
use hickory_net::xfer::{DnsExchange, DnsHandle, FirstAnswer};
use hickory_net::NetError;
use hickory_proto::op::{DnsRequest, DnsRequestOptions, Message};
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Clone)]
pub(super) enum ConnectTarget {
    Dot {
        addr: SocketAddr,
        server_name: ServerName<'static>,
    },
    Doh {
        host: Arc<str>,
        port: u16,
        /// TLS certificate name — the URL host unless config `hostname`
        /// overrides it.
        server_name: Arc<str>,
        path: Arc<str>,
    },
}

pub(super) struct ExchangeConn {
    target: ConnectTarget,
    tls: Arc<ClientConfig>,
    /// The live connection; `None` until the first query needs it. The lock
    /// is held across the connect on purpose: concurrent first queries share
    /// one handshake instead of stampeding.
    slot: Mutex<Slot>,
    /// Owns the JoinSet the exchanges' background I/O tasks spawn into —
    /// it must live as long as this upstream, or the tasks are aborted.
    provider: TokioRuntimeProvider,
    runtime: Option<Handle>,
    handshakes: AtomicU64,
}

#[derive(Default)]
pub(super) struct Slot {
    generation: u64,
    exchange: Option<DnsExchange<TokioRuntimeProvider>>,
}

impl Slot {
    fn store(&mut self, exchange: DnsExchange<TokioRuntimeProvider>) {
        self.generation += 1;
        self.exchange = Some(exchange);
    }
}

struct Held {
    exchange: DnsExchange<TokioRuntimeProvider>,
    generation: u64,
    fresh: bool,
}

impl ExchangeConn {
    pub(super) fn new(target: ConnectTarget, tls: Arc<ClientConfig>) -> Self {
        Self {
            target,
            tls,
            slot: Mutex::new(Slot::default()),
            provider: TokioRuntimeProvider::new(),
            runtime: Handle::try_current().ok(),
            handshakes: AtomicU64::new(0),
        }
    }

    /// TLS handshakes attempted so far (the connection-reuse counter).
    pub(super) fn handshakes(&self) -> u64 {
        self.handshakes.load(Ordering::Relaxed)
    }

    /// Test-only introspection of what the constructor parsed.
    #[cfg(test)]
    pub(super) fn target(&self) -> &ConnectTarget {
        &self.target
    }

    #[cfg(test)]
    pub(super) async fn hold_slot(&self) -> tokio::sync::MutexGuard<'_, Slot> {
        self.slot.lock().await
    }

    pub(super) async fn query(
        &self,
        request: &Message,
        attempt_timeout: Duration,
    ) -> io::Result<Message> {
        // Pass-through: the message goes in as-is (DO bit, EDNS and all);
        // hickory's multiplexer only randomizes the wire ID. The clone is
        // per-attempt rather than up-front so the common single-send path
        // deep-copies the message exactly once.
        let as_request = || DnsRequest::new(request.clone(), DnsRequestOptions::default());
        let held = self.acquire(attempt_timeout, None).await?;
        let err = match send_once(&held.exchange, as_request(), attempt_timeout).await {
            Ok(response) => return Ok(response),
            Err(err) => err,
        };
        if held.fresh || err.kind() == io::ErrorKind::TimedOut {
            self.invalidate_if_current(held.generation).await;
            return Err(err);
        }
        let held = self.acquire(attempt_timeout, Some(held.generation)).await?;
        match send_once(&held.exchange, as_request(), attempt_timeout).await {
            Ok(response) => Ok(response),
            Err(err) => {
                self.invalidate_if_current(held.generation).await;
                Err(err)
            }
        }
    }

    async fn acquire(&self, attempt_timeout: Duration, stale: Option<u64>) -> io::Result<Held> {
        let mut slot = self.slot.lock().await;
        if stale != Some(slot.generation) {
            if let Some(exchange) = slot.exchange.as_ref() {
                return Ok(Held {
                    exchange: exchange.clone(),
                    generation: slot.generation,
                    fresh: false,
                });
            }
        }
        let exchange = self.connect(attempt_timeout).await?;
        slot.store(exchange.clone());
        Ok(Held {
            exchange,
            generation: slot.generation,
            fresh: true,
        })
    }

    async fn invalidate_if_current(&self, generation: u64) {
        let mut slot = self.slot.lock().await;
        if slot.generation == generation {
            slot.exchange = None;
        }
    }

    async fn connect(
        &self,
        attempt_timeout: Duration,
    ) -> io::Result<DnsExchange<TokioRuntimeProvider>> {
        self.handshakes.fetch_add(1, Ordering::Relaxed);
        let mux_timeout = attempt_timeout * 2;
        let target = self.target.clone();
        let tls = Arc::clone(&self.tls);
        let provider = self.provider.clone();
        let connecting = async move {
            match target {
                ConnectTarget::Dot { addr, server_name } => tls_exchange(
                    addr,
                    server_name,
                    (*tls).clone(),
                    mux_timeout,
                    None,
                    provider,
                )
                .await
                .map_err(transport_error),
                ConnectTarget::Doh {
                    host,
                    port,
                    server_name,
                    path,
                } => {
                    let addr = resolve(&host, port).await?;
                    HttpsClientStream::builder(tls, provider)
                        .exchange(addr, server_name, path)
                        .await
                        .map_err(transport_error)
                }
            }
        };
        let timed_out = || io::Error::new(io::ErrorKind::TimedOut, "upstream connect timed out");
        match &self.runtime {
            Some(runtime) => {
                let task = runtime.spawn(connecting);
                let abort = task.abort_handle();
                match timeout(attempt_timeout, task).await {
                    Ok(joined) => joined.map_err(io::Error::other)?,
                    Err(_) => {
                        abort.abort();
                        Err(timed_out())
                    }
                }
            }
            None => timeout(attempt_timeout, connecting)
                .await
                .map_err(|_| timed_out())?,
        }
    }
}

/// The DoH bootstrap: a hostname URL needs the OS resolver once per
/// (re)connect — our own DNS can't answer before it's up. IP-literal URLs
/// (or `hostname`-pinned certs) skip this entirely.
async fn resolve(host: &str, port: u16) -> io::Result<SocketAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    tokio::net::lookup_host((host, port))
        .await?
        .next()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("DoH host {host:?} resolved to no addresses"),
            )
        })
}

async fn send_once(
    exchange: &DnsExchange<TokioRuntimeProvider>,
    request: DnsRequest,
    attempt_timeout: Duration,
) -> io::Result<Message> {
    let response = timeout(attempt_timeout, exchange.send(request).first_answer())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream timed out"))?
        .map_err(transport_error)?;
    Ok(response.into_message())
}

fn transport_error(err: NetError) -> io::Error {
    io::Error::new(transport_error_kind(&err), fah_common::error_chain(&err))
}

fn transport_error_kind(err: &NetError) -> io::ErrorKind {
    match err {
        NetError::Io(io_err) => return io_err.kind(),
        NetError::H2(h2_err) => {
            if let Some(io_err) = h2_err.get_io() {
                return io_err.kind();
            }
        }
        _ => {}
    }
    let mut tls = false;
    let mut next = std::error::Error::source(err);
    while let Some(cause) = next {
        if let Some(io_err) = cause.downcast_ref::<io::Error>() {
            return io_err.kind();
        }
        tls |= cause.is::<rustls::Error>();
        next = cause.source();
    }
    if tls {
        io::ErrorKind::InvalidData
    } else {
        io::ErrorKind::Other
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use fah_config::{
        DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy,
    };
    use hickory_proto::op::{OpCode, Query, ResponseCode};
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::super::{Forwarder, UpstreamPool};
    use super::*;

    fn a_query() -> Message {
        let mut message = Message::query();
        message.add_query(Query::query(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
        ));
        message
    }

    fn answer_for(request: &Message) -> Message {
        let mut response = Message::response(request.metadata.id, OpCode::Query);
        response.metadata.response_code = ResponseCode::NoError;
        response.queries = request.queries.clone();
        response.add_answer(Record::from_rdata(
            Name::from_str("example.com.").unwrap(),
            300,
            RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
        ));
        response
    }

    struct DotServer {
        addr: SocketAddr,
        accepts: Arc<AtomicU64>,
        cert: CertificateDer<'static>,
        blackhole: Arc<AtomicBool>,
    }

    /// A minimal DoT server on an ephemeral port: rcgen self-signed cert for
    /// "localhost", counting accepted connections. `queries_per_connection`
    /// simulates an upstream's idle-close policy: `None` serves a connection
    /// forever, `Some(n)` closes it after `n` answers.
    async fn dot_server(queries_per_connection: Option<usize>) -> DotServer {
        let signed = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert = signed.cert.der().clone();
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signed.signing_key.serialize_der()));
        let server_tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key)
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_tls));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepts = Arc::new(AtomicU64::new(0));
        let accepts_counter = Arc::clone(&accepts);
        let blackhole = Arc::new(AtomicBool::new(false));
        let connection_blackhole = Arc::clone(&blackhole);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                accepts_counter.fetch_add(1, Ordering::Relaxed);
                let acceptor = acceptor.clone();
                let blackhole = Arc::clone(&connection_blackhole);
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let mut served = 0usize;
                    loop {
                        let mut len_buf = [0u8; 2];
                        if tls.read_exact(&mut len_buf).await.is_err() {
                            return;
                        }
                        let mut request_buf = vec![0u8; u16::from_be_bytes(len_buf) as usize];
                        if tls.read_exact(&mut request_buf).await.is_err() {
                            return;
                        }
                        if blackhole.load(Ordering::Relaxed) {
                            continue;
                        }
                        let request = Message::from_vec(&request_buf).unwrap();
                        let reply = answer_for(&request).to_vec().unwrap();
                        let reply_len = u16::try_from(reply.len()).unwrap().to_be_bytes();
                        if tls.write_all(&reply_len).await.is_err()
                            || tls.write_all(&reply).await.is_err()
                        {
                            return;
                        }
                        served += 1;
                        if queries_per_connection.is_some_and(|limit| served >= limit) {
                            return; // connection closed by "upstream policy"
                        }
                    }
                });
            }
        });
        DotServer {
            addr,
            accepts,
            cert,
            blackhole,
        }
    }

    fn client_tls(roots: &[&CertificateDer<'static>]) -> Arc<ClientConfig> {
        let mut store = rustls::RootCertStore::empty();
        for root in roots {
            store.add((*root).clone()).unwrap();
        }
        Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(store)
                .with_no_client_auth(),
        )
    }

    fn dot_pool_from(
        addresses: Vec<String>,
        tls: Arc<ClientConfig>,
        timeout_ms: u32,
    ) -> UpstreamPool {
        UpstreamPool::with_tls_config(
            &DnsUpstreamsConfig {
                strategy: UpstreamStrategy::Fallback,
                timeout_ms,
                servers: addresses
                    .into_iter()
                    .map(|address| UpstreamServerConfig {
                        address,
                        protocol: UpstreamProtocol::Dot,
                        hostname: Some("localhost".to_string()),
                    })
                    .collect(),
                ..Default::default()
            },
            tls,
        )
        .unwrap()
    }

    fn dot_pool_of(servers: &[&DotServer], timeout_ms: u32) -> UpstreamPool {
        let roots: Vec<&CertificateDer<'static>> =
            servers.iter().map(|server| &server.cert).collect();
        let addresses = servers
            .iter()
            .map(|server| server.addr.to_string())
            .collect();
        dot_pool_from(addresses, client_tls(&roots), timeout_ms)
    }

    fn dot_pool(server: &DotServer) -> UpstreamPool {
        dot_pool_of(&[server], 2000)
    }

    fn dot_pool_adaptive(server: &DotServer) -> UpstreamPool {
        let mut pool = dot_pool(server);
        pool.strategy = UpstreamStrategy::Adaptive;
        pool
    }

    async fn concurrent_forwards(pool: &UpstreamPool, queries: usize) {
        let mut handles = Vec::new();
        for _ in 0..queries {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                pool.forward(&a_query()).await.is_ok()
            }));
        }
        for handle in handles {
            assert!(handle.await.unwrap(), "every query must be answered");
        }
    }

    #[tokio::test]
    async fn an_idle_close_and_reconnect_moves_no_health_under_adaptive() {
        let server = dot_server(Some(1)).await;
        let pool = dot_pool_adaptive(&server);

        for _ in 0..2 {
            let response = pool.forward(&a_query()).await.unwrap().message;
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        }

        let status = pool.status();
        assert_eq!(status[0].failures, 0);
        assert_eq!(status[0].consecutive_failures, 0);
        assert_eq!(
            status[0].tls_handshakes, 2,
            "the reconnect is connection lifecycle, never health"
        );
        assert_eq!(
            super::super::health::unpack(pool.health[0].state.load()).state,
            super::super::health::State::Healthy
        );
        assert_eq!(pool.health[0].penalties.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn concurrent_first_queries_share_one_handshake_under_adaptive() {
        let server = dot_server(None).await;
        let pool = dot_pool_adaptive(&server);

        concurrent_forwards(&pool, 8).await;

        assert_eq!(
            pool.status()[0].tls_handshakes,
            1,
            "selection never bypasses the single-flight handshake"
        );
        assert_eq!(server.accepts.load(Ordering::Relaxed), 1);
    }

    async fn closed_tcp_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    }

    #[tokio::test]
    async fn dot_answers_and_reuses_one_connection_across_queries() {
        let server = dot_server(None).await;
        let pool = dot_pool(&server);

        for _ in 0..3 {
            let response = pool.forward(&a_query()).await.unwrap().message;
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
            assert_eq!(response.answers.len(), 1);
        }

        // The acceptance criterion, asserted from both ends: one TCP accept
        // server-side, one handshake in our own counter.
        assert_eq!(server.accepts.load(Ordering::Relaxed), 1);
        let status = pool.status();
        assert_eq!(status[0].tls_handshakes, 1);
        assert_eq!(status[0].attempts, 3);
        assert_eq!(status[0].failures, 0);
    }

    #[tokio::test]
    async fn dot_reconnects_after_the_upstream_closes_an_idle_connection() {
        // Upstream closes after every answer — each later query finds a dead
        // pooled connection and must transparently reconnect + retry.
        let server = dot_server(Some(1)).await;
        let pool = dot_pool(&server);

        for _ in 0..2 {
            let response = pool.forward(&a_query()).await.unwrap().message;
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        }

        assert_eq!(server.accepts.load(Ordering::Relaxed), 2);
        assert_eq!(
            pool.status()[0].failures,
            0,
            "a transparent reconnect must not count as an upstream failure"
        );
    }

    #[tokio::test]
    async fn dot_fails_closed_when_the_certificate_is_untrusted() {
        let server = dot_server(None).await;
        // Trust store deliberately empty: the handshake must fail — an
        // encrypted upstream never silently downgrades.
        let pool = dot_pool_from(vec![server.addr.to_string()], client_tls(&[]), 1000);
        let err = pool.forward(&a_query()).await.unwrap_err();
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "a rejected certificate is a TLS path failure, not an opaque one"
        );
    }

    #[tokio::test]
    async fn dot_connect_to_a_dead_port_reports_connection_refused() {
        let pool = dot_pool_from(
            vec![closed_tcp_addr().await.to_string()],
            client_tls(&[]),
            10_000,
        );

        let err = pool.forward(&a_query()).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused, "{err}");
    }

    #[test]
    fn transport_error_donates_the_wrapped_io_kind() {
        for kind in [
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::ConnectionReset,
        ] {
            let err = transport_error(NetError::from(io::Error::new(kind, "synthetic")));
            assert_eq!(err.kind(), kind);
            assert!(err.to_string().contains("synthetic"));
        }
    }

    #[test]
    fn transport_error_classifies_a_rustls_failure_as_invalid_data() {
        let err = transport_error(NetError::from(rustls::Error::DecryptError));
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn transport_error_leaves_a_wire_decode_failure_opaque() {
        let err = transport_error(NetError::Message("malformed answer"));
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(err.to_string(), "malformed answer");
    }

    const BLACKHOLE_TIMEOUT_MS: u32 = 400;

    #[tokio::test]
    async fn dot_timeout_invalidates_the_pooled_connection_so_the_next_query_reconnects() {
        let server = dot_server(None).await;
        let pool = dot_pool_of(&[&server], BLACKHOLE_TIMEOUT_MS);

        assert!(pool.forward(&a_query()).await.is_ok());
        assert_eq!(pool.status()[0].tls_handshakes, 1);

        server.blackhole.store(true, Ordering::Relaxed);
        let started = Instant::now();
        let err = pool.forward(&a_query()).await.unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            elapsed < Duration::from_millis(u64::from(BLACKHOLE_TIMEOUT_MS) * 2),
            "a timeout must not retry inside the same query (took {elapsed:?})"
        );

        server.blackhole.store(false, Ordering::Relaxed);
        let response = pool.forward(&a_query()).await.unwrap().message;
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(
            pool.status()[0].tls_handshakes,
            2,
            "the blackholed connection must be replaced, not reused"
        );
        assert_eq!(server.accepts.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn dot_timeout_on_a_fresh_connection_leaves_nothing_pooled() {
        let server = dot_server(None).await;
        server.blackhole.store(true, Ordering::Relaxed);
        let pool = dot_pool_of(&[&server], BLACKHOLE_TIMEOUT_MS);

        for _ in 0..2 {
            let err = pool.forward(&a_query()).await.unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        }

        assert_eq!(
            pool.status()[0].tls_handshakes,
            2,
            "a connection that timed out on its first use must not be pooled"
        );
        assert_eq!(server.accepts.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn a_timed_out_upstream_falls_back_within_one_extra_window() {
        let dead = dot_server(None).await;
        dead.blackhole.store(true, Ordering::Relaxed);
        let live = dot_server(None).await;
        let pool = dot_pool_of(&[&dead, &live], BLACKHOLE_TIMEOUT_MS);

        let started = Instant::now();
        let response = pool.forward(&a_query()).await.unwrap().message;
        let elapsed = started.elapsed();

        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert!(
            elapsed < Duration::from_millis(u64::from(BLACKHOLE_TIMEOUT_MS) * 2),
            "fallback must cost one extra window, not two (took {elapsed:?})"
        );
        let status = pool.status();
        assert_eq!(status[0].failures, 1);
        assert_eq!(status[0].consecutive_failures, 1);
        assert_eq!(status[1].failures, 0);
    }

    #[tokio::test]
    async fn the_exchange_lives_on_the_runtime_that_built_the_conn_not_the_caller() {
        let server = dot_server(None).await;
        let conn = Arc::new(ExchangeConn::new(
            ConnectTarget::Dot {
                addr: server.addr,
                server_name: ServerName::try_from("localhost").unwrap(),
            },
            client_tls(&[&server.cert]),
        ));
        let attempt_timeout = Duration::from_secs(2);

        let caller = Arc::clone(&conn);
        tokio::task::spawn_blocking(move || {
            let other = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let held = other
                .block_on(caller.acquire(attempt_timeout, None))
                .unwrap();
            assert!(held.fresh);
        })
        .await
        .unwrap();

        let held = conn.acquire(attempt_timeout, None).await.unwrap();
        assert!(
            !held.fresh,
            "the connection installed from the other runtime is reused"
        );
        send_once(
            &held.exchange,
            DnsRequest::new(a_query(), DnsRequestOptions::default()),
            attempt_timeout,
        )
        .await
        .expect("the exchange must outlive the runtime that first used it");
        assert_eq!(conn.handshakes.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn invalidation_only_clears_the_generation_that_timed_out() {
        let server = dot_server(None).await;
        let conn = ExchangeConn::new(
            ConnectTarget::Dot {
                addr: server.addr,
                server_name: ServerName::try_from("localhost").unwrap(),
            },
            client_tls(&[&server.cert]),
        );
        let attempt_timeout = Duration::from_secs(2);

        let stale = conn.acquire(attempt_timeout, None).await.unwrap();
        assert!(stale.fresh);
        let current = conn
            .acquire(attempt_timeout, Some(stale.generation))
            .await
            .unwrap();
        assert!(current.fresh);
        assert!(current.generation > stale.generation);

        conn.invalidate_if_current(stale.generation).await;
        assert!(
            conn.slot.lock().await.exchange.is_some(),
            "a stale timeout must never drop a newer connection"
        );

        conn.invalidate_if_current(current.generation).await;
        assert!(conn.slot.lock().await.exchange.is_none());
    }

    #[tokio::test]
    async fn reconnecting_adopts_a_connection_another_query_already_installed() {
        let server = dot_server(None).await;
        let conn = ExchangeConn::new(
            ConnectTarget::Dot {
                addr: server.addr,
                server_name: ServerName::try_from("localhost").unwrap(),
            },
            client_tls(&[&server.cert]),
        );
        let attempt_timeout = Duration::from_secs(2);

        let stale = conn.acquire(attempt_timeout, None).await.unwrap();
        let replaced = conn
            .acquire(attempt_timeout, Some(stale.generation))
            .await
            .unwrap();
        let adopted = conn
            .acquire(attempt_timeout, Some(stale.generation))
            .await
            .unwrap();

        assert!(
            !adopted.fresh,
            "a second query on the same stale connection must not handshake again"
        );
        assert_eq!(adopted.generation, replaced.generation);
        assert_eq!(conn.handshakes(), 2);
        assert_eq!(server.accepts.load(Ordering::Relaxed), 2);
    }

    /// Network smoke tests (`cargo test -- --ignored`): real public
    /// resolvers, excluded from offline/gate runs.
    #[tokio::test]
    #[ignore = "network: real DoT upstream (1.1.1.1:853)"]
    async fn dot_smoke_against_cloudflare() {
        let pool = UpstreamPool::from_config(&DnsUpstreamsConfig {
            strategy: UpstreamStrategy::Fallback,
            timeout_ms: 5000,
            servers: vec![UpstreamServerConfig {
                address: "1.1.1.1".to_string(),
                protocol: UpstreamProtocol::Dot,
                hostname: Some("cloudflare-dns.com".to_string()),
            }],
            ..Default::default()
        })
        .unwrap();
        let response = pool.forward(&a_query()).await.unwrap().message;
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(pool.status()[0].tls_handshakes, 1);
    }

    #[tokio::test]
    #[ignore = "network: real DoH upstream (cloudflare-dns.com)"]
    async fn doh_smoke_against_cloudflare() {
        let pool = UpstreamPool::from_config(&DnsUpstreamsConfig {
            strategy: UpstreamStrategy::Fallback,
            timeout_ms: 5000,
            servers: vec![UpstreamServerConfig {
                address: "https://cloudflare-dns.com/dns-query".to_string(),
                protocol: UpstreamProtocol::Doh,
                hostname: None,
            }],
            ..Default::default()
        })
        .unwrap();
        for _ in 0..2 {
            let response = pool.forward(&a_query()).await.unwrap().message;
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        }
        assert_eq!(
            pool.status()[0].tls_handshakes,
            1,
            "second DoH query must reuse the connection"
        );
    }
}
