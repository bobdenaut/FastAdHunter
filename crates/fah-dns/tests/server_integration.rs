//! End-to-end acceptance tests: real wire-format DNS packets sent over real
//! UDP/TCP sockets to a running [`Server`] on an ephemeral port
//! (`hickory-client` isn't in the offline registry cache, so these use
//! `hickory-proto` directly for encode/decode — the same wire format, just
//! without the client's connection-management convenience layer).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fah_config::{
    DnsCacheConfig, DnsConfig, DnsListenConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol,
    UpstreamServerConfig, UpstreamStrategy,
};
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Server, UpstreamPool};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, RecordType};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Notify;
use tokio::time::timeout;

#[derive(Clone)]
struct SpyForwarder {
    calls: Arc<AtomicU64>,
}

fn empty_answer(request: &Message) -> ForwardOutcome {
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.response_code = ResponseCode::NoError;
    ForwardOutcome::new(response, 0)
}

impl Forwarder for SpyForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(empty_answer(request))
    }
}

#[derive(Clone)]
struct StallForwarder {
    release: Arc<Notify>,
    calls: Arc<AtomicU64>,
}

impl Forwarder for StallForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.release.notified().await;
        Ok(empty_answer(request))
    }
}

/// The returned [`tempfile::TempDir`] guard must outlive the server — hold it
/// in the test so the directory is removed on drop, not leaked into the OS
/// temp dir.
async fn start_server(rules_text: &str) -> (Server, Arc<AtomicU64>, tempfile::TempDir) {
    start_server_on("127.0.0.1", rules_text).await
}

async fn build_pipeline<F: Forwarder>(
    forwarder: F,
    rules_text: &str,
) -> (Arc<Pipeline<F>>, tempfile::TempDir) {
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

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let pipeline = Arc::new(Pipeline::new(
        manager,
        forwarder,
        10,
        &DnsCacheConfig::default(),
        fah_dns::DEFAULT_REFRESH_CLAIM_LEASE,
        tx,
    ));
    (pipeline, data_dir)
}

async fn start_server_on(
    address: &str,
    rules_text: &str,
) -> (Server, Arc<AtomicU64>, tempfile::TempDir) {
    let calls = Arc::new(AtomicU64::new(0));
    let forwarder = SpyForwarder {
        calls: calls.clone(),
    };
    let (pipeline, data_dir) = build_pipeline(forwarder, rules_text).await;
    let config = DnsConfig {
        listen: DnsListenConfig {
            address: address.to_string(),
            port: 0,
            dot_enabled: false,
            ..Default::default()
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    server.serve(pipeline, None);
    (server, calls, data_dir)
}

fn config_with(listen: DnsListenConfig) -> DnsConfig {
    DnsConfig {
        listen,
        ..DnsConfig::default()
    }
}

#[tokio::test]
async fn a_disabled_dot_listener_binds_nothing_on_its_port() {
    let held = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let dot_port = held.local_addr().unwrap().port();
    let listen = DnsListenConfig {
        address: "127.0.0.1".to_string(),
        port: 0,
        dot_enabled: true,
        dot_port,
        ..Default::default()
    };
    let refused = Server::bind(&config_with(listen.clone()))
        .await
        .err()
        .expect("dot_enabled = true must fail to bind a port another socket holds");
    assert!(refused.to_string().contains("DoT"), "got: {refused}");

    let listen = DnsListenConfig {
        dot_enabled: false,
        ..listen
    };
    let server = Server::bind(&config_with(listen.clone()))
        .await
        .expect("dot_enabled = false must not touch the DoT port, even while it is held");
    assert!(server.dot_addr().is_none());
    drop(server);
    drop(held);

    let listen = DnsListenConfig {
        dot_enabled: true,
        dot_port: 0,
        ..listen
    };
    let server = Server::bind(&config_with(listen)).await.unwrap();
    let bound = server
        .dot_addr()
        .expect("dot_enabled = true binds the DoT port");
    let taken = tokio::net::TcpListener::bind(bound).await;
    assert!(
        taken.is_err(),
        "the bound DoT port must be held by the server, got {taken:?}"
    );
}

fn encode_a_query(name: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(name).unwrap(),
        RecordType::A,
    ));
    message.to_vec().unwrap()
}

async fn udp_roundtrip(server_addr: SocketAddr, request: &[u8]) -> Vec<u8> {
    // The client socket must match the target's address family.
    let bind = if server_addr.is_ipv6() {
        "[::1]:0"
    } else {
        "127.0.0.1:0"
    };
    let socket = UdpSocket::bind(bind).await.unwrap();
    socket.send_to(request, server_addr).await.unwrap();
    let mut buf = [0u8; 4096];
    let len = timeout(Duration::from_secs(5), socket.recv(&mut buf))
        .await
        .unwrap()
        .unwrap();
    buf[..len].to_vec()
}

async fn tcp_roundtrip(server_addr: SocketAddr, request: &[u8]) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = TcpStream::connect(server_addr).await.unwrap();
    let len = u16::try_from(request.len()).unwrap().to_be_bytes();
    stream.write_all(&len).await.unwrap();
    stream.write_all(request).await.unwrap();

    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await.unwrap();
    let reply_len = u16::from_be_bytes(len_buf) as usize;
    let mut reply = vec![0u8; reply_len];
    stream.read_exact(&mut reply).await.unwrap();
    reply
}

/// `[dns.listen] address = "::"` serves both stacks on one socket
/// (CONFIGURATION.md) — the RB5009 IPv6 cutover binds this way. The same
/// engine, the same verdicts, over IPv6 and IPv4 alike, on UDP and TCP.
#[tokio::test]
async fn dual_stack_listener_answers_the_same_engine_on_both_stacks() {
    let (server, calls, _data_dir) = start_server_on("::", "||ads.example.com^\n").await;
    let query = encode_a_query("ads.example.com.");

    let udp_port = server.udp_addr().port();
    let udp_targets = [
        SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, udp_port)),
        SocketAddr::from((Ipv4Addr::LOCALHOST, udp_port)),
    ];
    for target in udp_targets {
        let reply = udp_roundtrip(target, &query).await;
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
        assert!(
            matches!(decoded.answers[0].data, RData::A(A(ip)) if ip == Ipv4Addr::UNSPECIFIED),
            "expected the synthesized blocked answer over {target}"
        );
    }

    let tcp_port = server.tcp_addr().port();
    for target in [
        SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, tcp_port)),
        SocketAddr::from((Ipv4Addr::LOCALHOST, tcp_port)),
    ] {
        let reply = tcp_roundtrip(target, &query).await;
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    }

    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "blocked on both stacks — the forwarder must never be reached"
    );
    server.shutdown();
}

#[tokio::test]
async fn blocked_domain_answers_null_ip_over_udp_and_never_reaches_the_forwarder() {
    let (server, calls, _data_dir) = start_server("||ads.example.com^\n").await;

    let reply = udp_roundtrip(server.udp_addr(), &encode_a_query("ads.example.com.")).await;
    let decoded = Message::from_vec(&reply).unwrap();

    assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    assert_eq!(decoded.answers.len(), 1);
    assert_eq!(decoded.answers[0].ttl, 10);
    assert!(matches!(decoded.answers[0].data, RData::A(A(addr)) if addr == Ipv4Addr::UNSPECIFIED));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "blocked queries must never touch the network"
    );

    server.shutdown();
}

#[tokio::test]
async fn allowed_domain_is_forwarded_and_answered_over_udp() {
    let (server, calls, _data_dir) = start_server("").await;

    let reply = udp_roundtrip(server.udp_addr(), &encode_a_query("example.com.")).await;
    let decoded = Message::from_vec(&reply).unwrap();

    assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    server.shutdown();
}

#[tokio::test]
async fn tcp_listener_answers_the_same_query_correctly() {
    let (server, _calls, _data_dir) = start_server("||ads.example.com^\n").await;

    let reply = tcp_roundtrip(server.tcp_addr(), &encode_a_query("ads.example.com.")).await;
    let decoded = Message::from_vec(&reply).unwrap();

    assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    assert_eq!(decoded.answers.len(), 1);
    assert!(matches!(decoded.answers[0].data, RData::A(A(addr)) if addr == Ipv4Addr::UNSPECIFIED));

    server.shutdown();
}

#[tokio::test]
async fn malformed_udp_packets_never_crash_the_listener() {
    let (server, _calls, _data_dir) = start_server("||ads.example.com^\n").await;

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    for garbage in [vec![], vec![0u8; 3], vec![0xFFu8; 200]] {
        socket.send_to(&garbage, server.udp_addr()).await.unwrap();
    }
    // The listener must have survived the garbage: a well-formed query sent
    // right after still gets a correct answer.
    let reply = udp_roundtrip(server.udp_addr(), &encode_a_query("ads.example.com.")).await;
    let decoded = Message::from_vec(&reply).unwrap();
    assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);

    server.shutdown();
}

#[tokio::test]
async fn one_tcp_connection_serves_multiple_queries() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (server, _calls, _data_dir) = start_server("||ads.example.com^\n").await;

    let mut stream = TcpStream::connect(server.tcp_addr()).await.unwrap();
    for name in ["ads.example.com.", "other.example.com."] {
        let request = encode_a_query(name);
        let len = u16::try_from(request.len()).unwrap().to_be_bytes();
        stream.write_all(&len).await.unwrap();
        stream.write_all(&request).await.unwrap();

        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await.unwrap();
        let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
        stream.read_exact(&mut reply).await.unwrap();
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    }

    server.shutdown();
}

/// The complete p1-06 pipeline, no test doubles: listener -> Rule Engine ->
/// cache -> [`UpstreamPool`] -> a mock upstream resolver. The second query
/// proves the cache-store wiring: the upstream is hit exactly once.
#[tokio::test]
async fn full_pipeline_forwards_via_upstream_pool_and_caches_the_answer() {
    // Mock upstream: answers every request with one A record, counting hits.
    let upstream_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_socket.local_addr().unwrap();
    let upstream_hits = Arc::new(AtomicU64::new(0));
    let hits_counter = Arc::clone(&upstream_hits);
    tokio::spawn(async move {
        loop {
            let mut buf = [0u8; 4096];
            let (len, client) = upstream_socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            hits_counter.fetch_add(1, Ordering::Relaxed);
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.response_code = ResponseCode::NoError;
            response.queries = request.queries.clone();
            response.add_answer(hickory_proto::rr::Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                300,
                RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
            ));
            let reply = response.to_vec().unwrap();
            upstream_socket.send_to(&reply, client).await.unwrap();
        }
    });

    let pool = UpstreamPool::from_config(&DnsUpstreamsConfig {
        strategy: UpstreamStrategy::Adaptive,
        timeout_ms: 2000,
        servers: vec![UpstreamServerConfig {
            address: upstream_addr.to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        }],
        ..Default::default()
    })
    .unwrap();

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
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let pipeline = Arc::new(Pipeline::new(
        manager,
        pool.clone(),
        10,
        &DnsCacheConfig::default(),
        fah_dns::DEFAULT_REFRESH_CLAIM_LEASE,
        tx,
    ));
    let config = DnsConfig {
        listen: DnsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
            dot_enabled: false,
            ..Default::default()
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    server.serve(pipeline, None);

    for _ in 0..2 {
        let reply = udp_roundtrip(server.udp_addr(), &encode_a_query("example.com.")).await;
        let decoded = Message::from_vec(&reply).unwrap();
        assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
        assert_eq!(decoded.answers.len(), 1);
    }

    assert_eq!(
        upstream_hits.load(Ordering::Relaxed),
        1,
        "second query must come from the cache, not the upstream"
    );
    assert_eq!(pool.status()[0].attempts, 1);
    server.shutdown();
}

#[tokio::test]
async fn a_serving_listener_never_reports_itself_fatal() {
    let (mut server, _calls, _data_dir) = start_server("").await;

    let reply = udp_roundtrip(server.udp_addr(), &encode_a_query("example.com.")).await;
    assert_eq!(
        Message::from_vec(&reply).unwrap().metadata.response_code,
        ResponseCode::NoError
    );
    assert!(
        timeout(Duration::from_millis(200), server.fatal())
            .await
            .is_err(),
        "a healthy listener must not signal a fatal error"
    );

    server.shutdown();
}

#[tokio::test]
async fn a_status_opcode_probe_is_answered_with_notimp_and_the_probe_id() {
    let (server, calls, _data_dir) = start_server("").await;

    let mut probe = Message::query();
    probe.metadata.op_code = hickory_proto::op::OpCode::Status;
    let id = probe.metadata.id;

    let reply = udp_roundtrip(server.udp_addr(), &probe.to_vec().unwrap()).await;
    let decoded = Message::from_vec(&reply).unwrap();

    assert_eq!(decoded.metadata.id, id);
    assert_eq!(decoded.metadata.response_code, ResponseCode::NotImp);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "the healthcheck probe must never reach an upstream"
    );

    server.shutdown();
}

#[tokio::test]
async fn udp_and_tcp_listeners_bind_independently() {
    let (server, _calls, _data_dir) = start_server("").await;
    let udp_ip: IpAddr = server.udp_addr().ip();
    let tcp_ip: IpAddr = server.tcp_addr().ip();
    assert_eq!(udp_ip, tcp_ip);
    assert_ne!(server.udp_addr().port(), 0);
    assert_ne!(server.tcp_addr().port(), 0);
    server.shutdown();
}

async fn await_inflight(
    gauge: &fah_dns::UdpInflightGauge,
    ready: impl Fn(&fah_model::DnsUdpInflight) -> bool,
) -> fah_model::DnsUdpInflight {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = gauge.snapshot();
        if ready(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "the UDP in-flight gauge never reached the expected state: {snapshot:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn the_configured_tcp_and_udp_ceilings_reach_the_listeners() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicU64::new(0));
    let (pipeline, _data_dir) = build_pipeline(
        StallForwarder {
            release: Arc::clone(&release),
            calls: Arc::clone(&calls),
        },
        "||blocked.example^\n",
    )
    .await;
    let config = DnsConfig {
        tcp_max_connections: 1,
        udp_max_inflight: 1,
        listen: DnsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
            dot_enabled: false,
            ..Default::default()
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    server.serve(pipeline, None);

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(&encode_a_query("first.example."), server.udp_addr())
        .await
        .unwrap();
    client
        .send_to(&encode_a_query("second.example."), server.udp_addr())
        .await
        .unwrap();
    let inflight = server.udp_inflight();
    let snapshot = await_inflight(&inflight, |snapshot| {
        snapshot.shed == 1 && calls.load(Ordering::Relaxed) == 1
    })
    .await;
    assert_eq!(
        (snapshot.active, snapshot.peak),
        (1, 1),
        "an explicit udp_max_inflight = 1 admits one datagram and sheds the next"
    );

    release.notify_one();
    let mut buf = [0u8; 4096];
    let len = timeout(Duration::from_secs(5), client.recv(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        Message::from_vec(&buf[..len])
            .unwrap()
            .metadata
            .response_code,
        ResponseCode::NoError
    );
    await_inflight(&inflight, |snapshot| snapshot.active == 0).await;
    assert!(
        timeout(Duration::from_millis(200), client.recv(&mut buf))
            .await
            .is_err(),
        "a shed datagram is never answered"
    );
    assert_eq!(inflight.snapshot().shed, 1);

    let holder = TcpStream::connect(server.tcp_addr()).await.unwrap();
    let mut waiting = TcpStream::connect(server.tcp_addr()).await.unwrap();
    let request = encode_a_query("blocked.example.");
    let len = u16::try_from(request.len()).unwrap().to_be_bytes();
    waiting.write_all(&len).await.unwrap();
    waiting.write_all(&request).await.unwrap();
    let mut len_buf = [0u8; 2];
    assert!(
        timeout(Duration::from_millis(300), waiting.read_exact(&mut len_buf))
            .await
            .is_err(),
        "an explicit tcp_max_connections = 1 holds the second connection while the first is open"
    );
    let connections = server.tcp_connections();
    let held = connections.snapshot();
    assert_eq!((held.active, held.peak), (1, 1));

    drop(holder);
    timeout(Duration::from_secs(5), waiting.read_exact(&mut len_buf))
        .await
        .unwrap()
        .unwrap();
    let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
    waiting.read_exact(&mut reply).await.unwrap();
    let decoded = Message::from_vec(&reply).unwrap();
    assert_eq!(decoded.metadata.response_code, ResponseCode::NoError);
    assert!(matches!(decoded.answers[0].data, RData::A(A(ip)) if ip == Ipv4Addr::UNSPECIFIED));
    assert_eq!(
        connections.snapshot().peak,
        1,
        "never two connections at once under a ceiling of one"
    );
    server.shutdown();
}

async fn await_connections(
    gauge: &fah_dns::TcpConnectionGauge,
    ready: impl Fn(&fah_model::DnsTcpConnections) -> bool,
) -> fah_model::DnsTcpConnections {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = gauge.snapshot();
        if ready(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "the connection gauge never reached the expected state: {snapshot:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn self_signed_fallback() -> Arc<rustls::sign::CertifiedKey> {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    let signed = rcgen::generate_simple_self_signed(vec!["fallback.test".to_string()]).unwrap();
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signed.signing_key.serialize_der()));
    let signing_key = rustls::crypto::aws_lc_rs::sign::any_supported_type(&key).unwrap();
    Arc::new(rustls::sign::CertifiedKey::new(
        vec![signed.cert.der().clone()],
        signing_key,
    ))
}

async fn stream_roundtrip<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: &mut S,
    request: &[u8],
) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let len = u16::try_from(request.len()).unwrap().to_be_bytes();
    stream.write_all(&len).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await.unwrap();
    let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
    stream.read_exact(&mut reply).await.unwrap();
    reply
}

/// DoT and plain TCP share the framing loop and the gauge *type*, so the only
/// thing keeping the two figures apart is which instance each listener was
/// handed. This drives one connection over each transport and checks both
/// gauges after each — a shared instance fails in whichever direction it was
/// shared.
#[tokio::test]
async fn each_dns_stream_listener_counts_into_its_own_gauge() {
    fah_certs::install_crypto_provider();
    let cert_dir = tempfile::tempdir().unwrap();
    let store = Arc::new(fah_certs::CertStore::open(cert_dir.path()).unwrap());
    store.generate_ca(&fah_certs::CaParams::default()).unwrap();
    let ca_der = rustls::pki_types::CertificateDer::from(store.ca_public_der().unwrap());

    let calls = Arc::new(AtomicU64::new(0));
    let (pipeline, _data_dir) = build_pipeline(
        SpyForwarder {
            calls: Arc::clone(&calls),
        },
        "||blocked.example^\n",
    )
    .await;
    let config = DnsConfig {
        tcp_max_connections: 8,
        listen: DnsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
            dot_enabled: true,
            dot_port: 0,
            ..Default::default()
        },
        ..DnsConfig::default()
    };
    let mut server = Server::bind(&config).await.unwrap();
    let tls = fah_dns::DotTls::new(store, self_signed_fallback()).unwrap();
    server.serve(pipeline, Some(tls));

    let dot = server.dot_connections();
    let tcp = server.tcp_connections();
    let request = encode_a_query("blocked.example.");

    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_der).unwrap();
    let client = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let host = "dns.fah.test";
    let plain = TcpStream::connect(server.dot_addr().unwrap())
        .await
        .unwrap();
    let mut encrypted = tokio_rustls::TlsConnector::from(client)
        .connect(
            rustls::pki_types::ServerName::try_from(host.to_string()).unwrap(),
            plain,
        )
        .await
        .unwrap();
    let reply = stream_roundtrip(&mut encrypted, &request).await;
    assert_eq!(
        Message::from_vec(&reply).unwrap().metadata.response_code,
        ResponseCode::NoError
    );
    assert_eq!(dot.snapshot().active, 1, "the DoT connection is counted");
    assert_eq!(
        tcp.snapshot(),
        fah_model::DnsTcpConnections::default(),
        "no DoT connection may reach the TCP gauge"
    );

    drop(encrypted);
    await_connections(&dot, |snapshot| snapshot.active == 0).await;

    let mut plain = TcpStream::connect(server.tcp_addr()).await.unwrap();
    let reply = stream_roundtrip(&mut plain, &request).await;
    assert_eq!(
        Message::from_vec(&reply).unwrap().metadata.response_code,
        ResponseCode::NoError
    );
    assert_eq!(
        tcp.snapshot().active,
        1,
        "the plain TCP connection is counted"
    );
    let after = dot.snapshot();
    assert_eq!(
        (after.active, after.peak),
        (0, 1),
        "the surviving DoT peak proves it was the DoT gauge that moved, not a shared one"
    );
    server.shutdown();
}
