//! Upstream resolution (ARCHITECTURE.md §Upstreams): the configured
//! `[[dns.upstreams.servers]]` are tried in order — primary first, next on
//! timeout or transport error (`strategy = "fallback"`). Plain UDP retries
//! over TCP against the same server when the answer comes back truncated;
//! DoT/DoH hold one persistent multiplexed connection each, so steady-state
//! traffic performs no TLS handshakes. DNSSEC is pass-through: the client's
//! DO bit and the upstream's RRSIGs travel unmodified (ARCHITECTURE.md
//! §Listeners).

mod alarm;
mod encrypted;
mod plain;

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fah_config::{DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy};
use hickory_proto::op::{Message, Query as WireQuery};
use hickory_proto::rr::{Name, RData, RecordType};
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tracing::{debug, info, warn};

use alarm::FailureAlarm;
use encrypted::{ConnectTarget, ExchangeConn};

/// Resolves an Allow/Pass query. Implementors run off the block path
/// entirely — [`crate::pipeline::Pipeline`] never calls this for a `Block`
/// verdict (ARCHITECTURE.md: "Blocked queries never touch the network").
pub trait Forwarder: Clone + Send + Sync + 'static {
    fn forward(
        &self,
        query: &Message,
    ) -> impl std::future::Future<Output = io::Result<Message>> + Send;
}

/// The real end of the pipeline: ordered fallback over the configured
/// upstream servers, each attempt bounded by `[dns.upstreams] timeout_ms` —
/// a down primary costs at most one extra timeout window before the next
/// server answers.
#[derive(Clone)]
pub struct UpstreamPool {
    servers: Arc<[UpstreamServer]>,
    timeout: Duration,
    /// Shared, not cloned: the pool is handed out by cheap `Arc` clone (see
    /// `fastadhunter`'s adapters), and a per-clone alarm would let every holder
    /// warn once for the same outage.
    alarm: Arc<FailureAlarm>,
}

/// One upstream's counters — feeds p1-08's metrics and p1-09's `/health`
/// degraded state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamStatus {
    /// The `address` exactly as configured — the stable identity for
    /// metrics labels.
    pub address: String,
    pub protocol: fah_model::Protocol,
    pub attempts: u64,
    pub failures: u64,
    /// Failures since the last success. **Not a liveness signal**: `forward`
    /// walks the list in order and never skips, so a secondary is attempted
    /// only when the primary fails and its streak can be hours old.
    pub consecutive_failures: u64,
    /// TLS handshakes attempted (always 0 for plain UDP). Staying flat while
    /// `attempts` grows is the connection-reuse proof (p1-06 acceptance).
    pub tls_handshakes: u64,
}

impl UpstreamPool {
    pub fn from_config(config: &DnsUpstreamsConfig) -> io::Result<Self> {
        let tls = Arc::new(
            hickory_net::tls::client_config()
                .map_err(|err| io::Error::other(format!("building rustls config: {err}")))?,
        );
        Self::with_tls_config(config, tls)
    }

    /// Test seam: DoT/DoH tests hand in a `ClientConfig` trusting their own
    /// throwaway CA instead of the webpki roots `from_config` bakes in.
    fn with_tls_config(config: &DnsUpstreamsConfig, tls: Arc<ClientConfig>) -> io::Result<Self> {
        match config.strategy {
            // Single variant today; a future strategy gets wired here.
            UpstreamStrategy::Fallback => {}
        }
        let servers = config
            .servers
            .iter()
            .map(|server| UpstreamServer::new(server, &tls))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            servers: servers.into(),
            timeout: Duration::from_millis(u64::from(config.timeout_ms)),
            alarm: Arc::new(FailureAlarm::new()),
        })
    }

    /// Resolves a hostname to addresses over the configured upstreams — the
    /// same servers that answer client queries, reached the same way.
    ///
    /// This is *not* the query pipeline: it skips the Rule Engine and the
    /// cache deliberately. Running it through the pipeline would let a
    /// blocklist blacklist the host serving the next copy of itself, so a
    /// single bad rule could stop all future list updates with no way back
    /// short of editing the config by hand.
    ///
    /// `A` and `AAAA` go out concurrently and either one carrying addresses is
    /// enough. That matters on a v4-only or v6-only link, where the other
    /// family's query legitimately comes back empty or fails — treating that
    /// as a total failure is precisely the musl `getaddrinfo` behaviour that
    /// broke list fetches in the first place (p1-11 defect 2).
    pub async fn resolve_host(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        let name = Name::from_utf8(host)
            .map_err(|err| invalid(format!("invalid hostname {host:?}: {err}")))?;

        let (v4, v6) = tokio::join!(
            self.lookup(&name, RecordType::A),
            self.lookup(&name, RecordType::AAAA),
        );

        let mut addrs = Vec::new();
        // v4 first: on a link with no working IPv6 this puts a usable address
        // at the front, and the connector tries them in order.
        for found in [&v4, &v6].into_iter().flatten() {
            addrs.extend(found.iter().copied());
        }
        if !addrs.is_empty() {
            return Ok(addrs);
        }
        // Nothing usable — report why, preferring a transport error over a
        // merely empty answer, since that is the actionable one.
        match (v4, v6) {
            (Err(err), _) | (_, Err(err)) => Err(err),
            _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no addresses for {host}"),
            )),
        }
    }

    async fn lookup(&self, name: &Name, record_type: RecordType) -> io::Result<Vec<IpAddr>> {
        let mut query = Message::query();
        query.add_query(WireQuery::query(name.clone(), record_type));
        query.metadata.recursion_desired = true;

        let response = self.forward(&query).await?;
        Ok(response
            .answers
            .iter()
            .filter_map(|record| match &record.data {
                RData::A(a) => Some(IpAddr::V4(a.0)),
                RData::AAAA(aaaa) => Some(IpAddr::V6(aaaa.0)),
                // CNAMEs in the chain are ignored: the upstream is recursive,
                // so the addresses it resolved to are in this same answer.
                _ => None,
            })
            .collect())
    }

    pub fn status(&self) -> Vec<UpstreamStatus> {
        self.servers
            .iter()
            .map(|server| UpstreamStatus {
                address: server.address.clone(),
                protocol: server.protocol,
                attempts: server.attempts.load(Ordering::Relaxed),
                failures: server.failures.load(Ordering::Relaxed),
                consecutive_failures: server.consecutive_failures.load(Ordering::Relaxed),
                tls_handshakes: match &server.transport {
                    Transport::Udp { .. } => 0,
                    Transport::Encrypted(conn) => conn.handshakes(),
                },
            })
            .collect()
    }
}

impl Forwarder for UpstreamPool {
    async fn forward(&self, query: &Message) -> io::Result<Message> {
        let mut last_err = None;
        for server in self.servers.iter() {
            server.attempts.fetch_add(1, Ordering::Relaxed);
            match server.query(query, self.timeout).await {
                Ok(response) => {
                    server.consecutive_failures.store(0, Ordering::Relaxed);
                    if self.alarm.clear() {
                        info!(
                            upstreams = self.servers.len(),
                            "upstreams recovered — answering from the network again"
                        );
                    }
                    return Ok(response);
                }
                Err(err) => {
                    debug!(upstream = %server.address, error = %err, "upstream attempt failed");
                    server.failures.fetch_add(1, Ordering::Relaxed);
                    server.consecutive_failures.fetch_add(1, Ordering::Relaxed);
                    last_err = Some(err);
                }
            }
        }
        // Every configured upstream failed this query. With encrypted-only
        // upstreams nothing plaintext waits behind them, so clients are now
        // living on whatever the cache can still serve — the operator has to
        // hear about it, at most once per alarm interval.
        let Some(err) = last_err else {
            // Degenerate rather than an outage: `fah_config` validation rejects
            // an empty server list, so there is nothing here to have failed.
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no upstreams configured",
            ));
        };
        if let Some(suppressed) = self.alarm.claim() {
            warn!(
                upstreams = self.servers.len(),
                suppressed,
                error = %err,
                "all upstreams failed — answers now depend on cached entries"
            );
        }
        Err(err)
    }
}

struct UpstreamServer {
    address: String,
    protocol: fah_model::Protocol,
    transport: Transport,
    attempts: AtomicU64,
    failures: AtomicU64,
    consecutive_failures: AtomicU64,
}

enum Transport {
    Udp { addr: SocketAddr },
    Encrypted(ExchangeConn),
}

impl UpstreamServer {
    fn new(config: &UpstreamServerConfig, tls: &Arc<ClientConfig>) -> io::Result<Self> {
        let (protocol, transport) = match config.protocol {
            UpstreamProtocol::Udp => (
                fah_model::Protocol::Udp,
                Transport::Udp {
                    addr: socket_addr(&config.address, 53)?,
                },
            ),
            UpstreamProtocol::Dot => {
                // fah-config validation already demands the hostname; the
                // re-check keeps this constructor safe standalone.
                let hostname = config
                    .hostname
                    .clone()
                    .filter(|hostname| !hostname.is_empty())
                    .ok_or_else(|| {
                        invalid(format!(
                            "dot upstream {} requires a hostname",
                            config.address
                        ))
                    })?;
                let server_name = ServerName::try_from(hostname).map_err(|err| {
                    invalid(format!(
                        "dot upstream {}: invalid hostname: {err}",
                        config.address
                    ))
                })?;
                (
                    fah_model::Protocol::Dot,
                    Transport::Encrypted(ExchangeConn::new(
                        ConnectTarget::Dot {
                            addr: socket_addr(&config.address, 853)?,
                            server_name,
                        },
                        Arc::clone(tls),
                    )),
                )
            }
            UpstreamProtocol::Doh => {
                let url = url::Url::parse(&config.address)
                    .map_err(|err| invalid(format!("doh upstream {}: {err}", config.address)))?;
                if url.scheme() != "https" {
                    return Err(invalid(format!(
                        "doh upstream {} must be an https:// URL",
                        config.address
                    )));
                }
                // Not `host_str()`: that brackets IPv6 literals ("[::1]"),
                // which neither the resolver nor rustls's `ServerName`
                // accepts — take the typed host and render IPs bare.
                let host: Arc<str> = match url.host() {
                    Some(url::Host::Domain(domain)) => domain.into(),
                    Some(url::Host::Ipv4(ip)) => ip.to_string().into(),
                    Some(url::Host::Ipv6(ip)) => ip.to_string().into(),
                    None => {
                        return Err(invalid(format!(
                            "doh upstream {} has no host",
                            config.address
                        )))
                    }
                };
                // The URL host names the certificate; `hostname` overrides it
                // (the IP-literal-URL case, where the cert still carries the
                // resolver's DNS name).
                let server_name: Arc<str> = config
                    .hostname
                    .as_deref()
                    .filter(|hostname| !hostname.is_empty())
                    .map_or_else(|| Arc::clone(&host), Into::into);
                (
                    fah_model::Protocol::Doh,
                    Transport::Encrypted(ExchangeConn::new(
                        ConnectTarget::Doh {
                            host,
                            port: url.port().unwrap_or(443),
                            server_name,
                            path: url.path().into(),
                        },
                        Arc::clone(tls),
                    )),
                )
            }
        };
        Ok(Self {
            address: config.address.clone(),
            protocol,
            transport,
            attempts: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            consecutive_failures: AtomicU64::new(0),
        })
    }

    async fn query(&self, request: &Message, attempt_timeout: Duration) -> io::Result<Message> {
        match &self.transport {
            Transport::Udp { addr } => plain::query(*addr, request, attempt_timeout).await,
            Transport::Encrypted(conn) => conn.query(request, attempt_timeout).await,
        }
    }
}

/// `[[dns.upstreams.servers]] address` for udp/dot: a bare IP (default port
/// applied) or an explicit `IP:port`.
fn socket_addr(address: &str, default_port: u16) -> io::Result<SocketAddr> {
    if let Ok(addr) = address.parse::<SocketAddr>() {
        return Ok(addr);
    }
    address
        .parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, default_port))
        .map_err(|_| {
            invalid(format!(
                "invalid upstream address {address:?} (expected IP or IP:port)"
            ))
        })
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::time::Instant;

    use hickory_proto::op::{Edns, OpCode, Query, ResponseCode};
    use hickory_proto::rr::rdata::{A, NULL};
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use tokio::net::{TcpListener, UdpSocket};

    use super::*;

    fn udp_server_config(addr: SocketAddr) -> UpstreamServerConfig {
        UpstreamServerConfig {
            address: addr.to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        }
    }

    fn pool_of(servers: Vec<UpstreamServerConfig>, timeout_ms: u32) -> UpstreamPool {
        UpstreamPool::from_config(&DnsUpstreamsConfig {
            strategy: UpstreamStrategy::Fallback,
            timeout_ms,
            servers,
        })
        .unwrap()
    }

    fn a_query() -> Message {
        let mut message = Message::query();
        message.add_query(Query::query(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
        ));
        message
    }

    /// A response echoing the request's ID and question, answering with one
    /// A record — the shape every mock upstream in here replies with.
    fn answer_for(request: &Message) -> Message {
        let mut response = Message::response(request.metadata.id, OpCode::Query);
        response.metadata.response_code = ResponseCode::NoError;
        response.queries = request.queries.clone();
        response.add_answer(Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            300,
            RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
        ));
        response
    }

    fn rcode_response(request: &Message, code: ResponseCode) -> Message {
        let mut response = Message::response(request.metadata.id, OpCode::Query);
        response.metadata.response_code = code;
        response.queries = request.queries.clone();
        response
    }

    async fn udp_server_with(
        count: usize,
        reply: impl Fn(&Message) -> Message + Send + 'static,
    ) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..count {
                let mut buf = [0u8; 4096];
                let (len, client) = socket.recv_from(&mut buf).await.unwrap();
                let request = Message::from_vec(&buf[..len]).unwrap();
                let bytes = reply(&request).to_vec().unwrap();
                socket.send_to(&bytes, client).await.unwrap();
            }
        });
        addr
    }

    async fn answering_udp_server(count: usize) -> SocketAddr {
        udp_server_with(count, answer_for).await
    }

    async fn rcode_udp_server(count: usize, code: ResponseCode) -> SocketAddr {
        udp_server_with(count, move |request| rcode_response(request, code)).await
    }

    #[tokio::test]
    async fn a_decoded_rcode_is_not_an_upstream_failure() {
        for code in [
            ResponseCode::ServFail,
            ResponseCode::NXDomain,
            ResponseCode::Refused,
        ] {
            let primary = rcode_udp_server(1, code).await;
            let secondary = answering_udp_server(1).await;
            let pool = pool_of(
                vec![udp_server_config(primary), udp_server_config(secondary)],
                2000,
            );
            pool.servers[0]
                .consecutive_failures
                .store(7, Ordering::Relaxed);

            let response = pool.forward(&a_query()).await.unwrap();

            assert_eq!(response.metadata.response_code, code);
            assert_eq!(pool.servers[0].attempts.load(Ordering::Relaxed), 1);
            assert_eq!(
                pool.servers[0].failures.load(Ordering::Relaxed),
                0,
                "{code} must not move the failure counter"
            );
            assert_eq!(
                pool.servers[0].consecutive_failures.load(Ordering::Relaxed),
                0,
                "{code} is a successful exchange and resets the streak"
            );
            assert_eq!(
                pool.servers[1].attempts.load(Ordering::Relaxed),
                0,
                "{code} must not continue the upstream walk"
            );
        }
    }

    async fn dead_addr() -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        drop(socket);
        addr
    }

    /// Serves `count` UDP requests, answering `A` with 93.184.216.34 and
    /// leaving `AAAA` unanswered when `answer_aaaa` is false — the shape of a
    /// host with no IPv6, and of the link where musl's all-or-nothing
    /// `getaddrinfo` broke list fetches (p1-11 defect 2).
    async fn family_aware_udp_server(count: usize, answer_aaaa: bool) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..count {
                let mut buf = [0u8; 4096];
                let (len, client) = socket.recv_from(&mut buf).await.unwrap();
                let request = Message::from_vec(&buf[..len]).unwrap();
                let is_aaaa = request.queries[0].query_type() == RecordType::AAAA;
                if is_aaaa && !answer_aaaa {
                    continue; // silence, so the AAAA lookup times out
                }
                let mut response = Message::response(request.metadata.id, OpCode::Query);
                response.metadata.response_code = ResponseCode::NoError;
                response.queries = request.queries.clone();
                if !is_aaaa {
                    response.add_answer(Record::from_rdata(
                        Name::from_ascii("lists.example.com.").unwrap(),
                        300,
                        RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
                    ));
                }
                let reply = response.to_vec().unwrap();
                socket.send_to(&reply, client).await.unwrap();
            }
        });
        addr
    }

    #[tokio::test]
    async fn resolve_host_uses_the_configured_upstreams() {
        let addr = family_aware_udp_server(2, true).await;
        let pool = pool_of(vec![udp_server_config(addr)], 2000);

        let addrs = pool.resolve_host("lists.example.com").await.unwrap();
        assert_eq!(addrs, vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]);
    }

    #[tokio::test]
    async fn resolve_host_succeeds_when_only_one_address_family_answers() {
        // AAAA gets no reply at all; the A answer alone must still be enough.
        // Treating a dead family as total failure is exactly the bug this
        // whole port exists to avoid.
        let addr = family_aware_udp_server(2, false).await;
        let pool = pool_of(vec![udp_server_config(addr)], 300);

        let addrs = pool.resolve_host("lists.example.com").await.unwrap();
        assert_eq!(addrs, vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]);
    }

    #[tokio::test]
    async fn resolve_host_errors_when_no_upstream_answers() {
        let pool = pool_of(vec![udp_server_config(dead_addr().await)], 200);

        let err = pool.resolve_host("lists.example.com").await.unwrap_err();
        assert!(
            !err.to_string().is_empty(),
            "the caller logs this chain; it must say something"
        );
    }

    #[tokio::test]
    async fn resolve_host_rejects_a_hostname_it_cannot_parse() {
        let pool = pool_of(vec![udp_server_config(dead_addr().await)], 200);

        // No network round trip should be attempted for this.
        let err = pool.resolve_host("not a hostname").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn first_upstream_answers() {
        let addr = answering_udp_server(1).await;
        let pool = pool_of(vec![udp_server_config(addr)], 2000);
        let response = pool.forward(&a_query()).await.unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(response.answers.len(), 1);
    }

    #[tokio::test]
    async fn falls_back_past_a_timing_out_upstream_within_one_extra_window() {
        // Bound but silent: the realistic down-primary shape — packets
        // vanish, only the timeout advances the fallback.
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let silent_addr = silent.local_addr().unwrap();
        let alive = answering_udp_server(1).await;
        let pool = pool_of(
            vec![udp_server_config(silent_addr), udp_server_config(alive)],
            500,
        );

        let started = Instant::now();
        let response = pool.forward(&a_query()).await.unwrap();
        let elapsed = started.elapsed();

        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert!(
            elapsed < Duration::from_millis(1500),
            "answer must arrive within one extra timeout window, took {elapsed:?}"
        );

        let status = pool.status();
        assert_eq!(status[0].failures, 1);
        assert_eq!(status[0].consecutive_failures, 1);
        assert_eq!(status[1].attempts, 1);
        assert_eq!(status[1].failures, 0);
        drop(silent);
    }

    #[tokio::test]
    async fn success_resets_consecutive_failures() {
        let dead = dead_addr().await;
        let pool = pool_of(vec![udp_server_config(dead)], 200);
        assert!(pool.forward(&a_query()).await.is_err());
        assert_eq!(pool.status()[0].consecutive_failures, 1);

        // Something starts answering on the same address: recovery.
        let socket = UdpSocket::bind(dead).await.unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            let reply = answer_for(&request).to_vec().unwrap();
            socket.send_to(&reply, client).await.unwrap();
        });
        pool.forward(&a_query()).await.unwrap();

        let status = pool.status();
        assert_eq!(status[0].consecutive_failures, 0);
        assert_eq!(status[0].failures, 1, "historical count stays");
    }

    #[tokio::test]
    async fn all_upstreams_dead_errors_and_counts_failures() {
        let pool = pool_of(
            vec![
                udp_server_config(dead_addr().await),
                udp_server_config(dead_addr().await),
            ],
            200,
        );
        assert!(pool.forward(&a_query()).await.is_err());
        for status in pool.status() {
            assert_eq!(status.attempts, 1);
            assert_eq!(status.failures, 1);
        }
    }

    #[tokio::test]
    async fn truncated_udp_reply_is_retried_over_tcp_to_the_same_server() {
        // UDP and TCP mocks must share one port ("TCP retry to the *same*
        // server"): bind TCP on an ephemeral port, then claim the matching
        // UDP port — retrying because another process may hold it.
        let (tcp_listener, udp_socket) = loop {
            let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
            if let Ok(udp) = UdpSocket::bind(tcp.local_addr().unwrap()).await {
                break (tcp, udp);
            }
        };
        let addr = udp_socket.local_addr().unwrap();

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let (len, client) = udp_socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            let mut truncated = Message::response(request.metadata.id, OpCode::Query);
            truncated.metadata.response_code = ResponseCode::NoError;
            truncated.metadata.truncation = true;
            truncated.queries = request.queries.clone();
            let reply = truncated.to_vec().unwrap();
            udp_socket.send_to(&reply, client).await.unwrap();
        });
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = tcp_listener.accept().await.unwrap();
            let mut len_buf = [0u8; 2];
            stream.read_exact(&mut len_buf).await.unwrap();
            let mut request_buf = vec![0u8; u16::from_be_bytes(len_buf) as usize];
            stream.read_exact(&mut request_buf).await.unwrap();
            let request = Message::from_vec(&request_buf).unwrap();
            let reply = answer_for(&request).to_vec().unwrap();
            let reply_len = u16::try_from(reply.len()).unwrap().to_be_bytes();
            stream.write_all(&reply_len).await.unwrap();
            stream.write_all(&reply).await.unwrap();
        });

        let pool = pool_of(vec![udp_server_config(addr)], 2000);
        let response = pool.forward(&a_query()).await.unwrap();
        assert!(!response.metadata.truncation);
        assert_eq!(
            response.answers.len(),
            1,
            "the full TCP answer, not the truncated stub"
        );
        assert_eq!(pool.status()[0].failures, 0, "a TC retry is not a failure");
    }

    #[tokio::test]
    async fn do_bit_is_forwarded_and_rrsig_comes_back_untouched() {
        let rrsig_bytes = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x42];
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let served_rdata = rrsig_bytes.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            // The DNSSEC pass-through contract's request half: the DO bit
            // set by the client must still be set on the upstream's copy.
            assert!(
                request
                    .edns
                    .as_ref()
                    .is_some_and(|edns| edns.flags().dnssec_ok),
                "DO bit must be forwarded upstream"
            );
            let mut response = answer_for(&request);
            // No dnssec feature compiled in (Phase 1 is pass-through), so an
            // RRSIG parses as an Unknown record carrying raw rdata — exactly
            // what must survive the round trip unmodified.
            response.add_answer(Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                300,
                RData::Unknown {
                    code: RecordType::RRSIG,
                    rdata: NULL::with(served_rdata),
                },
            ));
            let reply = response.to_vec().unwrap();
            socket.send_to(&reply, client).await.unwrap();
        });

        let mut query = a_query();
        query
            .edns
            .get_or_insert_with(Edns::new)
            .set_max_payload(4096)
            .set_dnssec_ok(true);

        let pool = pool_of(vec![udp_server_config(addr)], 2000);
        let response = pool.forward(&query).await.unwrap();
        let rrsig = response
            .answers
            .iter()
            .find(|record| record.record_type() == RecordType::RRSIG)
            .expect("RRSIG must be passed through");
        assert!(
            matches!(&rrsig.data, RData::Unknown { rdata, .. } if rdata.anything == rrsig_bytes),
            "RRSIG rdata must come back byte-identical"
        );
    }

    #[tokio::test]
    async fn upstream_query_id_is_randomized_not_the_clients() {
        // Collect the IDs three upstream-side requests arrive with. The
        // client's ID is fixed; all three matching it has probability
        // (1/65536)^3 — effectively impossible unless the ID is passed
        // through unrandomized.
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let (id_tx, mut id_rx) = tokio::sync::mpsc::channel(3);
        tokio::spawn(async move {
            for _ in 0..3 {
                let mut buf = [0u8; 4096];
                let (len, client) = socket.recv_from(&mut buf).await.unwrap();
                let request = Message::from_vec(&buf[..len]).unwrap();
                id_tx.send(request.metadata.id).await.unwrap();
                let reply = answer_for(&request).to_vec().unwrap();
                socket.send_to(&reply, client).await.unwrap();
            }
        });

        let pool = pool_of(vec![udp_server_config(addr)], 2000);
        let mut query = a_query();
        query.metadata.id = 0x1234;
        let mut seen = Vec::new();
        for _ in 0..3 {
            pool.forward(&query).await.unwrap();
            seen.push(id_rx.recv().await.unwrap());
        }
        assert!(
            seen.iter().any(|id| *id != 0x1234),
            "upstream query IDs must not be the client's own"
        );
    }

    #[tokio::test]
    async fn mismatched_id_reply_is_discarded_and_the_matching_one_accepted() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            // A spoofed-looking reply with the wrong ID, then garbage, then
            // the genuine answer.
            let mut wrong = Message::response(request.metadata.id.wrapping_add(1), OpCode::Query);
            wrong.metadata.response_code = ResponseCode::NXDomain;
            socket
                .send_to(&wrong.to_vec().unwrap(), client)
                .await
                .unwrap();
            socket.send_to(&[0xFF; 5], client).await.unwrap();
            let reply = answer_for(&request).to_vec().unwrap();
            socket.send_to(&reply, client).await.unwrap();
        });

        let pool = pool_of(vec![udp_server_config(addr)], 2000);
        let response = pool.forward(&a_query()).await.unwrap();
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
    }

    #[tokio::test]
    async fn empty_pool_errors_instead_of_hanging() {
        let pool = pool_of(vec![], 200);
        assert!(pool.forward(&a_query()).await.is_err());
    }

    #[test]
    fn bare_ip_gets_the_default_port_and_explicit_port_wins() {
        assert_eq!(
            socket_addr("1.1.1.1", 53).unwrap(),
            "1.1.1.1:53".parse().unwrap()
        );
        assert_eq!(
            socket_addr("1.1.1.1:5353", 53).unwrap(),
            "1.1.1.1:5353".parse().unwrap()
        );
        assert_eq!(
            socket_addr("::1", 853).unwrap(),
            "[::1]:853".parse().unwrap()
        );
        assert!(socket_addr("not-an-ip", 53).is_err());
    }

    #[test]
    fn doh_url_must_be_https_with_a_host() {
        let tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        );
        let bad = UpstreamServerConfig {
            address: "http://cloudflare-dns.com/dns-query".to_string(),
            protocol: UpstreamProtocol::Doh,
            hostname: None,
        };
        assert!(UpstreamServer::new(&bad, &tls).is_err());
        let good = UpstreamServerConfig {
            address: "https://cloudflare-dns.com/dns-query".to_string(),
            protocol: UpstreamProtocol::Doh,
            hostname: None,
        };
        assert!(UpstreamServer::new(&good, &tls).is_ok());
    }

    #[test]
    fn doh_ip_literal_urls_are_accepted_including_ipv6() {
        let tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        );
        // IPv6 literals arrive bracketed in the URL; the constructor must
        // store them bare or the resolver and rustls both choke on them.
        for address in [
            "https://1.1.1.1/dns-query",
            "https://[2606:4700:4700::1111]/dns-query",
            "https://[2606:4700:4700::1111]:8443/dns-query",
        ] {
            let config = UpstreamServerConfig {
                address: address.to_string(),
                protocol: UpstreamProtocol::Doh,
                hostname: Some("cloudflare-dns.com".to_string()),
            };
            let server =
                UpstreamServer::new(&config, &tls).unwrap_or_else(|err| panic!("{address}: {err}"));
            let Transport::Encrypted(conn) = &server.transport else {
                panic!("doh must build an encrypted transport");
            };
            let encrypted::ConnectTarget::Doh { host, .. } = conn.target() else {
                panic!("doh must build a Doh target");
            };
            assert!(
                host.parse::<IpAddr>().is_ok(),
                "{address}: stored host {host:?} must parse as a bare IP"
            );
        }
    }
}
