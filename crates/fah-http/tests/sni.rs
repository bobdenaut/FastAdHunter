use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_config::{HttpConfig, HttpListenConfig, HttpsConfig, HttpsListenConfig, NoSni};
use fah_model::{Event, EventKind, ResourceType, Verdict};
use fah_rules::{Matcher, MatcherBuilder};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use fah_http::{ConnectionGauge, Proxy, ProxyCounters, Server, TlsProxy, TlsServer};

const ORIGIN_NAME: &str = "origin.test";

struct FixedResolver(Vec<IpAddr>);

impl HostResolver for FixedResolver {
    fn resolve(&self, _host: String) -> Resolving {
        let addresses = self.0.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

struct FixedRules(Arc<Matcher>);

impl fah_http::Ruleset for FixedRules {
    fn matcher(&self) -> Arc<Matcher> {
        Arc::clone(&self.0)
    }
}

fn rules_with(lines: &str) -> Arc<dyn fah_http::Ruleset> {
    let parsed = fah_rules::parse_rule_list(lines);
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("test-list", &parsed);
    Arc::new(FixedRules(Arc::new(builder.build())))
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn self_signed() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec![ORIGIN_NAME.to_string()]).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::try_from(key_pair.serialize_der()).unwrap(),
    )
}

async fn origin() -> (SocketAddr, Arc<AtomicU64>) {
    let (cert, key) = self_signed();
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&accepts);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::Relaxed);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(stream).await else {
                    return;
                };
                let (mut reader, mut writer) = tokio::io::split(stream);
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    (addr, accepts)
}

#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn connector() -> TlsConnector {
    let provider = provider();
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify(provider)))
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

struct Harness {
    server: TlsServer,
    addr: SocketAddr,
    counters: Arc<ProxyCounters>,
}

impl Harness {
    fn shutdown(&self) {
        self.server.shutdown();
    }
}

async fn harness(
    origin_port: u16,
    resolves_to: IpAddr,
    rules: Option<Arc<dyn fah_http::Ruleset>>,
    events: Option<mpsc::Sender<Event>>,
    no_sni: NoSni,
) -> Harness {
    harness_with(
        origin_port,
        resolves_to,
        rules,
        events,
        no_sni,
        Limits::default(),
    )
    .await
}

struct Limits {
    hello: Duration,
    idle: Duration,
    max_connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            hello: Duration::from_secs(5),
            idle: Duration::from_secs(30),
            max_connections: 1024,
        }
    }
}

async fn harness_with(
    origin_port: u16,
    resolves_to: IpAddr,
    rules: Option<Arc<dyn fah_http::Ruleset>>,
    events: Option<mpsc::Sender<Event>>,
    no_sni: NoSni,
    limits: Limits,
) -> Harness {
    let policy = DestinationPolicy::new(
        origin_port,
        vec![AllowedNet::host("127.0.0.1".parse().unwrap())],
    );
    let mut proxy = TlsProxy::new(
        Arc::new(FixedResolver(vec![resolves_to])),
        policy,
        origin_port,
        limits.hello,
        limits.idle,
        no_sni,
    );
    let max_connections = limits.max_connections;
    if let Some(rules) = rules {
        proxy = proxy.with_rules(rules);
    }
    if let Some(events) = events {
        proxy = proxy.with_events(events);
    }
    let proxy = Arc::new(proxy);
    let counters = proxy.counters();

    let config = HttpsConfig {
        listen: HttpsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        max_connections,
        ..HttpsConfig::default()
    };
    let mut server = TlsServer::bind(&config).await.unwrap();
    let addr = server.local_addr();
    server.serve(proxy);
    Harness {
        server,
        addr,
        counters,
    }
}

async fn next_event(rx: &mut mpsc::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event must arrive")
        .expect("channel open")
}

#[tokio::test]
async fn a_tls_session_completes_end_to_end_through_the_splice() {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.port(),
        origin.ip(),
        Some(rules_with("||ads.example.com^\n")),
        Some(tx),
        NoSni::Pass,
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let mut tls = connector()
        .connect(name, stream)
        .await
        .expect("the handshake must complete through the splice");

    let payload: Vec<u8> = (0..64 * 1024).map(|index| (index % 251) as u8).collect();
    tls.write_all(&payload).await.unwrap();
    let mut echoed = vec![0u8; payload.len()];
    tls.read_exact(&mut echoed).await.unwrap();
    assert_eq!(
        echoed, payload,
        "the spliced payload must be byte-identical"
    );

    assert_eq!(accepts.load(Ordering::Relaxed), 1);
    drop(tls);

    let event = next_event(&mut rx).await;
    assert_eq!(event.kind(), EventKind::HttpsSni);
    let Event::HttpsSni(event) = event else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.request.path, "");
    assert_eq!(event.request.method, "");
    assert_eq!(event.request.resource_type, ResourceType::Unknown);
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 0);
    assert!(event.bytes >= payload.len() as u64);
    harness.shutdown();
}

#[tokio::test]
async fn a_blocked_sni_costs_the_origin_nothing() {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.port(),
        origin.ip(),
        Some(rules_with(&format!("||{ORIGIN_NAME}^\n"))),
        Some(tx),
        NoSni::Pass,
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    assert!(
        connector().connect(name, stream).await.is_err(),
        "a blocked SNI must close the connection"
    );

    let event = next_event(&mut rx).await;
    assert_eq!(event.kind(), EventKind::HttpsSni);
    let Event::HttpsSni(event) = event else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.bytes, 0);
    match &event.verdict {
        Verdict::Block(rule) => assert!(rule.rule.contains(ORIGIN_NAME), "{}", rule.rule),
        other => panic!("expected a block verdict, got {other:?}"),
    }

    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "a blocked SNI must reach no upstream at all"
    );
    let counters = harness.counters.snapshot();
    assert_eq!(counters.blocked, 1);
    assert_eq!(counters.resolve_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_allowed_sni_is_spliced_and_reported_as_allow() {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.port(),
        origin.ip(),
        Some(rules_with(&format!(
            "||{ORIGIN_NAME}^\n@@||{ORIGIN_NAME}^\n"
        ))),
        Some(tx),
        NoSni::Pass,
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let mut tls = connector()
        .connect(name, stream)
        .await
        .expect("an allow verdict must splice exactly like a pass");
    tls.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tls.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
    drop(tls);

    let Event::HttpsSni(event) = next_event(&mut rx).await else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    match &event.verdict {
        Verdict::Allow(rule) => assert!(rule.rule.contains(ORIGIN_NAME), "{}", rule.rule),
        other => panic!("expected an allow verdict, got {other:?}"),
    }
    assert!(event.bytes >= 4, "got {}", event.bytes);
    assert_eq!(harness.counters.snapshot().blocked, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_sni_resolving_to_a_private_address_is_refused() {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.port(),
        "192.168.77.1".parse().unwrap(),
        Some(rules_with("||ads.example.com^\n")),
        Some(tx),
        NoSni::Pass,
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    assert!(connector().connect(name, stream).await.is_err());

    let Event::HttpsSni(event) = next_event(&mut rx).await else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 0);
    assert_eq!(event.bytes, 0);

    let counters = harness.counters.snapshot();
    assert_eq!(counters.refused_destination, 1);
    assert_eq!(counters.upstream_failures, 0);
    assert_eq!(accepts.load(Ordering::Relaxed), 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_unreachable_upstream_is_reported_within_the_hello_deadline() {
    const HELLO: Duration = Duration::from_millis(500);
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness_with(
        443,
        "192.0.2.1".parse().unwrap(),
        None,
        Some(tx),
        NoSni::Pass,
        Limits {
            hello: HELLO,
            ..Limits::default()
        },
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let outcome = tokio::time::timeout(HELLO * 10, connector().connect(name, stream))
        .await
        .expect("the connect deadline must close the client long before the OS SYN retry window");
    assert!(
        outcome.is_err(),
        "nothing answers on TEST-NET-1; the handshake cannot complete"
    );

    let Event::HttpsSni(event) = next_event(&mut rx).await else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 0);
    assert_eq!(event.bytes, 0);

    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_failures, 1);
    assert_eq!(counters.resolve_failures, 0);
    assert_eq!(counters.refused_destination, 0);
    harness.shutdown();
}

#[tokio::test]
async fn garbage_on_the_https_port_is_closed_and_counted() {
    let (origin, _) = origin().await;
    let harness = harness(origin.port(), origin.ip(), None, None, NoSni::Pass).await;

    let mut stream = TcpStream::connect(harness.addr).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .expect("non-TLS bytes must be closed, not held")
        .unwrap();
    assert!(response.is_empty());
    let counters = harness.counters.snapshot();
    assert_eq!(counters.non_tls, 1);
    assert_eq!(counters.hello_timeouts, 0, "garbage is not silence");

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let mut tls = connector()
        .connect(name, stream)
        .await
        .expect("the listener must still serve the next connection");
    tls.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tls.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
    harness.shutdown();
}

#[tokio::test]
async fn a_preconnect_closed_unused_is_a_hello_timeout_not_garbage() {
    let (origin, _) = origin().await;
    let harness = harness(origin.port(), origin.ip(), None, None, NoSni::Pass).await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    drop(stream);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let counters = loop {
        let counters = harness.counters.snapshot();
        if counters.hello_timeouts == 1 {
            break counters;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "an EOF before any hello must be counted as a hello timeout"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(counters.non_tls, 0, "silence is not garbage");
    assert_eq!(counters.connections, 1);
    assert_eq!(
        counters.requests, 0,
        "a connection that never reached a verdict is not a request"
    );
    harness.shutdown();
}

#[tokio::test]
async fn an_idle_spliced_session_is_closed_and_its_permit_returned() {
    const IDLE: Duration = Duration::from_millis(400);
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness_with(
        origin.port(),
        origin.ip(),
        None,
        Some(tx),
        NoSni::Pass,
        Limits {
            idle: IDLE,
            max_connections: 1,
            ..Limits::default()
        },
    )
    .await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let mut tls = connector().connect(name, stream).await.unwrap();
    tls.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tls.read_exact(&mut echoed).await.unwrap();

    let mut buf = [0u8; 1];
    let closed = tokio::time::timeout(IDLE * 10, tls.read(&mut buf))
        .await
        .expect("the idle deadline must close a silent session");
    assert!(
        matches!(closed, Ok(0) | Err(_)),
        "the client must see the session end, got {closed:?}"
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 1);

    let event = next_event(&mut rx).await;
    let Event::HttpsSni(event) = event else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.verdict, Verdict::Pass);
    assert!(
        event.bytes >= 4,
        "bytes relayed before the idle close must be counted, got {}",
        event.bytes
    );
    assert!(
        event.duration < IDLE,
        "duration is hello-to-connected, not session length, got {:?}",
        event.duration
    );

    let mut next = TcpStream::connect(harness.addr).await.unwrap();
    next.write_all(b"not tls").await.unwrap();
    let read = tokio::time::timeout(Duration::from_secs(5), next.read(&mut buf))
        .await
        .expect("the released permit must let the next connection through")
        .unwrap();
    assert_eq!(read, 0);
    harness.shutdown();
}

async fn no_sni_verdict(no_sni: NoSni) -> Verdict {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(origin.port(), origin.ip(), None, Some(tx), no_sni).await;

    let stream = TcpStream::connect(harness.addr).await.unwrap();
    let name = ServerName::from("127.0.0.1".parse::<IpAddr>().unwrap());
    assert!(
        connector().connect(name, stream).await.is_err(),
        "a hello with no SNI has no destination and must be closed"
    );

    let event = next_event(&mut rx).await;
    assert_eq!(event.kind(), EventKind::HttpsSni);
    let Event::HttpsSni(event) = event else {
        panic!("expected an https-sni event");
    };
    assert_eq!(
        event.request.host, "",
        "a no-SNI observation carries no name"
    );
    assert_eq!(event.bytes, 0);
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "no-SNI is never spliced, whatever the classification says"
    );
    harness.shutdown();
    event.verdict
}

#[tokio::test]
async fn a_hello_without_sni_is_classified_by_config() {
    assert_eq!(no_sni_verdict(NoSni::Pass).await, Verdict::Pass);
    match no_sni_verdict(NoSni::Block).await {
        Verdict::Block(rule) => assert!(rule.rule.contains("no_sni"), "{}", rule.rule),
        other => panic!("expected a block verdict, got {other:?}"),
    }
}

struct RecordingResolver {
    addresses: Vec<IpAddr>,
    thread: Arc<Mutex<Option<String>>>,
}

impl HostResolver for RecordingResolver {
    fn resolve(&self, _host: String) -> Resolving {
        *self.thread.lock().unwrap() = std::thread::current().name().map(str::to_string);
        let addresses = self.addresses.clone();
        Box::pin(async move { Ok(addresses) })
    }
}

struct DomainHarness {
    http: Server,
    tls: TlsServer,
    http_addr: SocketAddr,
    https_addr: SocketAddr,
    counters: Arc<ProxyCounters>,
    https_open: Arc<ConnectionGauge>,
    http_open: Arc<ConnectionGauge>,
    thread: Arc<Mutex<Option<String>>>,
}

impl DomainHarness {
    fn shutdown(&mut self) {
        self.tls.shutdown();
        self.http.shutdown();
    }
}

async fn domain_harness(
    origin: SocketAddr,
    events: Option<mpsc::Sender<Event>>,
    limits: Limits,
) -> DomainHarness {
    let host = "127.0.0.1".parse::<IpAddr>().unwrap();
    let origin_port = origin.port();
    let thread = Arc::new(Mutex::new(None));
    let mut proxy = TlsProxy::new(
        Arc::new(RecordingResolver {
            addresses: vec![origin.ip()],
            thread: Arc::clone(&thread),
        }),
        DestinationPolicy::new(origin_port, vec![AllowedNet::host(host)]),
        origin_port,
        limits.hello,
        limits.idle,
        NoSni::Pass,
    )
    .with_rules(rules_with("||ads.example.com^\n"));
    if let Some(events) = events {
        proxy = proxy.with_events(events);
    }
    let proxy = Arc::new(proxy);
    let counters = proxy.counters();

    let mut http = Server::bind(&HttpConfig {
        listen: HttpListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        max_connections: limits.max_connections,
        ..HttpConfig::default()
    })
    .await
    .unwrap();
    let http_addr = http.local_addr();
    let http_open = http.connections();
    http.serve_domains(NonZeroUsize::MIN, Duration::from_secs(1), move || {
        Proxy::new(
            Arc::new(FixedResolver(vec![host])),
            DestinationPolicy::new(origin_port, vec![AllowedNet::host(host)]),
            origin_port,
            limits.hello,
            limits.idle,
            1,
            false,
        )
    })
    .unwrap();

    let mut tls = TlsServer::bind(&HttpsConfig {
        listen: HttpsListenConfig {
            address: "127.0.0.1".to_string(),
            port: 0,
        },
        max_connections: limits.max_connections,
        ..HttpsConfig::default()
    })
    .await
    .unwrap();
    let https_addr = tls.local_addr();
    let https_open = tls.connections();
    tls.serve_domains(Arc::clone(&proxy), &http).unwrap();

    DomainHarness {
        http,
        tls,
        http_addr,
        https_addr,
        counters,
        https_open,
        http_open,
        thread,
    }
}

#[tokio::test]
async fn an_allowed_sni_is_spliced_on_an_allocation_domain() {
    let (origin, accepts) = origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let mut harness = domain_harness(origin, Some(tx), Limits::default()).await;

    let stream = TcpStream::connect(harness.https_addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    let mut tls = connector()
        .connect(name, stream)
        .await
        .expect("the handshake must complete through the domain lane");

    let payload: Vec<u8> = (0..16 * 1024).map(|index| (index % 251) as u8).collect();
    tls.write_all(&payload).await.unwrap();
    let mut echoed = vec![0u8; payload.len()];
    tls.read_exact(&mut echoed).await.unwrap();
    assert_eq!(
        echoed, payload,
        "the spliced payload must be byte-identical on the domain lane"
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
    drop(tls);

    let event = next_event(&mut rx).await;
    let Event::HttpsSni(event) = event else {
        panic!("expected an https-sni event");
    };
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.verdict, Verdict::Pass);
    assert!(event.bytes >= payload.len() as u64);

    let stats = harness.counters.snapshot();
    assert_eq!(stats.connections, 1);
    assert_eq!(stats.blocked, 0);
    assert_eq!(
        harness.thread.lock().unwrap().as_deref(),
        Some("fah-http-0"),
        "the spliced session must run on the allocation domain's own thread"
    );

    harness.shutdown();
}

#[tokio::test]
async fn a_saturated_https_lane_leaves_the_http_lane_bounded_and_leaks_no_permit() {
    const CEILING: usize = 2;
    const OVERSHOOT: usize = 40;

    let (origin, _accepts) = origin().await;
    let mut harness = domain_harness(
        origin,
        None,
        Limits {
            hello: Duration::from_secs(30),
            idle: Duration::from_secs(30),
            max_connections: CEILING,
        },
    )
    .await;

    let mut held = Vec::new();
    for _ in 0..CEILING {
        let stream = TcpStream::connect(harness.https_addr).await.unwrap();
        let name = ServerName::try_from(ORIGIN_NAME).unwrap();
        held.push(
            connector()
                .connect(name, stream)
                .await
                .expect("the lane must serve up to its ceiling"),
        );
    }

    let mut queued = Vec::new();
    for _ in 0..OVERSHOOT {
        queued.push(TcpStream::connect(harness.https_addr).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        harness.https_open.open() as usize,
        CEILING,
        "{OVERSHOOT} sockets past the ceiling must wait for a permit, not be served"
    );

    let mut http = TcpStream::connect(harness.http_addr).await.unwrap();
    http.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
    let mut answer = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), http.read_to_end(&mut answer))
        .await
        .expect("the HTTP lane must answer while the HTTPS lane sits at its ceiling")
        .unwrap();
    assert!(
        String::from_utf8_lossy(&answer).contains(" 400 "),
        "a request with no Host is refused; got: {answer:?}"
    );
    drop(http);

    drop(queued);
    drop(held);
    let drained = wait_for(Duration::from_secs(10), || {
        harness.https_open.open() == 0 && harness.http_open.open() == 0
    })
    .await;
    assert!(
        drained,
        "every permit must come back: https={} http={}",
        harness.https_open.open(),
        harness.http_open.open()
    );

    let stats = harness.counters.snapshot();
    assert_eq!(
        stats.connections,
        (CEILING + OVERSHOOT) as u64,
        "every accepted HTTPS socket is judged once the ceiling frees up"
    );

    harness.shutdown();
}

async fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    done()
}
