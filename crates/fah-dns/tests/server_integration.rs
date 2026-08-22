//! End-to-end acceptance tests: real wire-format DNS packets sent over real
//! UDP/TCP sockets to a running [`Server`] on an ephemeral port
//! (`hickory-client` isn't in the offline registry cache, so these use
//! `hickory-proto` directly for encode/decode — the same wire format, just
//! without the client's connection-management convenience layer).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fah_config::{
    DnsCacheConfig, DnsListenConfig, DnsUpstreamsConfig, RulesConfig, UpstreamProtocol,
    UpstreamServerConfig, UpstreamStrategy,
};
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Server, UpstreamPool};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, RecordType};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

#[derive(Clone)]
struct SpyForwarder {
    calls: Arc<AtomicU64>,
}

impl Forwarder for SpyForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        Ok(ForwardOutcome::new(response, 0))
    }
}

/// The returned [`tempfile::TempDir`] guard must outlive the server — hold it
/// in the test so the directory is removed on drop, not leaked into the OS
/// temp dir.
async fn start_server(rules_text: &str) -> (Server, Arc<AtomicU64>, tempfile::TempDir) {
    start_server_on("127.0.0.1", rules_text).await
}

async fn start_server_on(
    address: &str,
    rules_text: &str,
) -> (Server, Arc<AtomicU64>, tempfile::TempDir) {
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

    let calls = Arc::new(AtomicU64::new(0));
    let forwarder = SpyForwarder {
        calls: calls.clone(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} }); // drain, don't fill the channel

    let pipeline = Arc::new(Pipeline::new(
        manager,
        forwarder,
        10,
        &DnsCacheConfig::default(),
        fah_dns::DEFAULT_REFRESH_CLAIM_LEASE,
        tx,
    ));
    let listen = DnsListenConfig {
        address: address.to_string(),
        port: 0,
    };
    let mut server = Server::bind(&listen).await.unwrap();
    server.serve(pipeline);
    (server, calls, data_dir)
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
        strategy: UpstreamStrategy::Fallback,
        timeout_ms: 2000,
        servers: vec![UpstreamServerConfig {
            address: upstream_addr.to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        }],
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
    let listen = DnsListenConfig {
        address: "127.0.0.1".to_string(),
        port: 0,
    };
    let mut server = Server::bind(&listen).await.unwrap();
    server.serve(pipeline);

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
