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
use hickory_proto::op::{DnsRequest, DnsRequestOptions, Message};
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::sync::Mutex;
use tokio::time::timeout;

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
    slot: Mutex<Option<DnsExchange<TokioRuntimeProvider>>>,
    /// Owns the JoinSet the exchanges' background I/O tasks spawn into —
    /// it must live as long as this upstream, or the tasks are aborted.
    provider: TokioRuntimeProvider,
    handshakes: AtomicU64,
}

impl ExchangeConn {
    pub(super) fn new(target: ConnectTarget, tls: Arc<ClientConfig>) -> Self {
        Self {
            target,
            tls,
            slot: Mutex::new(None),
            provider: TokioRuntimeProvider::new(),
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
        let (exchange, fresh) = self.connected(attempt_timeout).await?;
        match send_once(&exchange, as_request(), attempt_timeout).await {
            Ok(response) => Ok(response),
            // A fresh connection that immediately errored, or a timeout (the
            // connection is likely fine and the upstream slow): surface it —
            // the pool falls back within the one-extra-window budget.
            Err(err) if fresh || err.kind() == io::ErrorKind::TimedOut => Err(err),
            // A pooled connection the upstream closed while it sat idle —
            // the routine shape after quiet hours. Reconnect once, retry.
            Err(_) => {
                let exchange = self.reconnect(attempt_timeout).await?;
                send_once(&exchange, as_request(), attempt_timeout).await
            }
        }
    }

    async fn connected(
        &self,
        attempt_timeout: Duration,
    ) -> io::Result<(DnsExchange<TokioRuntimeProvider>, bool)> {
        let mut slot = self.slot.lock().await;
        if let Some(exchange) = slot.as_ref() {
            return Ok((exchange.clone(), false));
        }
        let exchange = self.connect(attempt_timeout).await?;
        *slot = Some(exchange.clone());
        Ok((exchange, true))
    }

    async fn reconnect(
        &self,
        attempt_timeout: Duration,
    ) -> io::Result<DnsExchange<TokioRuntimeProvider>> {
        let mut slot = self.slot.lock().await;
        let exchange = self.connect(attempt_timeout).await?;
        *slot = Some(exchange.clone());
        Ok(exchange)
    }

    async fn connect(
        &self,
        attempt_timeout: Duration,
    ) -> io::Result<DnsExchange<TokioRuntimeProvider>> {
        self.handshakes.fetch_add(1, Ordering::Relaxed);
        // The multiplexer's internal per-request timeout is deliberately set
        // above `attempt_timeout` so `send_once`'s own timeout always fires
        // first — a slow answer must classify as "upstream slow" (fall back,
        // keep the connection), never as "connection dead" (reconnect).
        let mux_timeout = attempt_timeout * 2;
        let connecting = async {
            match &self.target {
                ConnectTarget::Dot { addr, server_name } => tls_exchange(
                    *addr,
                    server_name.clone(),
                    (*self.tls).clone(),
                    mux_timeout,
                    None,
                    self.provider.clone(),
                )
                .await
                .map_err(io::Error::other),
                ConnectTarget::Doh {
                    host,
                    port,
                    server_name,
                    path,
                } => {
                    let addr = resolve(host, *port).await?;
                    HttpsClientStream::builder(Arc::clone(&self.tls), self.provider.clone())
                        .exchange(addr, Arc::clone(server_name), Arc::clone(path))
                        .await
                        .map_err(io::Error::other)
                }
            }
        };
        timeout(attempt_timeout, connecting)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream connect timed out"))?
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
        .map_err(io::Error::other)?;
    Ok(response.into_message())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;

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

    /// A minimal DoT server on an ephemeral port: rcgen self-signed cert for
    /// "localhost", counting accepted connections. `queries_per_connection`
    /// simulates an upstream's idle-close policy: `None` serves a connection
    /// forever, `Some(n)` closes it after `n` answers.
    async fn dot_server(
        queries_per_connection: Option<usize>,
    ) -> (SocketAddr, Arc<AtomicU64>, CertificateDer<'static>) {
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
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                accepts_counter.fetch_add(1, Ordering::Relaxed);
                let acceptor = acceptor.clone();
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
        (addr, accepts, cert)
    }

    fn dot_pool(addr: SocketAddr, root: &CertificateDer<'static>) -> UpstreamPool {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(root.clone()).unwrap();
        let tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        UpstreamPool::with_tls_config(
            &DnsUpstreamsConfig {
                strategy: UpstreamStrategy::Fallback,
                timeout_ms: 2000,
                servers: vec![UpstreamServerConfig {
                    address: addr.to_string(),
                    protocol: UpstreamProtocol::Dot,
                    hostname: Some("localhost".to_string()),
                }],
            },
            tls,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn dot_answers_and_reuses_one_connection_across_queries() {
        let (addr, accepts, cert) = dot_server(None).await;
        let pool = dot_pool(addr, &cert);

        for _ in 0..3 {
            let response = pool.forward(&a_query()).await.unwrap();
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
            assert_eq!(response.answers.len(), 1);
        }

        // The acceptance criterion, asserted from both ends: one TCP accept
        // server-side, one handshake in our own counter.
        assert_eq!(accepts.load(Ordering::Relaxed), 1);
        let status = pool.status();
        assert_eq!(status[0].tls_handshakes, 1);
        assert_eq!(status[0].attempts, 3);
        assert_eq!(status[0].failures, 0);
    }

    #[tokio::test]
    async fn dot_reconnects_after_the_upstream_closes_an_idle_connection() {
        // Upstream closes after every answer — each later query finds a dead
        // pooled connection and must transparently reconnect + retry.
        let (addr, accepts, cert) = dot_server(Some(1)).await;
        let pool = dot_pool(addr, &cert);

        for _ in 0..2 {
            let response = pool.forward(&a_query()).await.unwrap();
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        }

        assert_eq!(accepts.load(Ordering::Relaxed), 2);
        assert_eq!(
            pool.status()[0].failures,
            0,
            "a transparent reconnect must not count as an upstream failure"
        );
    }

    #[tokio::test]
    async fn dot_fails_closed_when_the_certificate_is_untrusted() {
        let (addr, _accepts, _cert) = dot_server(None).await;
        // Trust store deliberately empty: the handshake must fail — an
        // encrypted upstream never silently downgrades.
        let tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        );
        let pool = UpstreamPool::with_tls_config(
            &DnsUpstreamsConfig {
                strategy: UpstreamStrategy::Fallback,
                timeout_ms: 1000,
                servers: vec![UpstreamServerConfig {
                    address: addr.to_string(),
                    protocol: UpstreamProtocol::Dot,
                    hostname: Some("localhost".to_string()),
                }],
            },
            tls,
        )
        .unwrap();
        assert!(pool.forward(&a_query()).await.is_err());
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
        })
        .unwrap();
        let response = pool.forward(&a_query()).await.unwrap();
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
        })
        .unwrap();
        for _ in 0..2 {
            let response = pool.forward(&a_query()).await.unwrap();
            assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        }
        assert_eq!(
            pool.status()[0].tls_handshakes,
            1,
            "second DoH query must reuse the connection"
        );
    }
}
