use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fah_certs::{CaParams, CertStore};
use fah_common::egress::{AllowedNet, DestinationPolicy};
use fah_common::resolve::{HostResolver, Resolving};
use fah_config::{HttpsConfig, HttpsListenConfig, NoSni};
use fah_model::{Event, EventKind, ResourceType, Verdict};
use fah_rules::{Matcher, MatcherBuilder};
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, Incoming};
use hyper::client::conn::{http1, http2};
use hyper::header::{ACCEPT, CONNECTION, CONTENT_TYPE, HOST};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::sign::CertifiedKey;
use rustls::RootCertStore;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::client::TlsStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use fah_http::{ExclusionSet, Interception, ProxyCounters, TlsProxy, TlsServer};

const ORIGIN_NAME: &str = "origin.test";
const PAGE_BYTES: usize = 200 * 1024;
const SEEN_HOST: &str = "x-seen-host";
const HOST_HEADER: &str = "x-host-header";
const BODY_BYTES: &str = "x-body-bytes";

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Proto {
    H1,
    H2,
}

impl Proto {
    fn alpn(self) -> Vec<Vec<u8>> {
        match self {
            Proto::H1 => vec![b"http/1.1".to_vec()],
            Proto::H2 => vec![b"h2".to_vec()],
        }
    }
}

fn page() -> Bytes {
    Bytes::from(
        (0..PAGE_BYTES)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<u8>>(),
    )
}

struct OriginCa {
    root: CertificateDer<'static>,
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}

fn origin_ca() -> OriginCa {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    OriginCa {
        root: cert.der().clone(),
        issuer: rcgen::Issuer::new(params, key),
    }
}

fn leaf_signed_by(ca: &OriginCa) -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec![ORIGIN_NAME.to_string()]).unwrap();
    let cert = params.signed_by(&key, &ca.issuer).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::try_from(key.serialize_der()).unwrap(),
    )
}

fn self_signed() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(vec![ORIGIN_NAME.to_string()]).unwrap();
    let cert = params.self_signed(&key).unwrap();
    (
        cert.der().clone(),
        PrivateKeyDer::try_from(key.serialize_der()).unwrap(),
    )
}

struct Origin {
    addr: SocketAddr,
    connections: Arc<AtomicU64>,
    requests: Arc<AtomicU64>,
    body_seen: tokio::sync::watch::Receiver<u64>,
}

struct OriginSpec {
    proto: Proto,
    keys: Vec<(CertificateDer<'static>, PrivateKeyDer<'static>)>,
    alpn: Option<Vec<Vec<u8>>>,
    close_each_response: bool,
    goaway_after_first: bool,
    cut_after_first: bool,
    count_body: bool,
}

impl OriginSpec {
    fn new(proto: Proto, cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> Self {
        Self {
            proto,
            keys: vec![(cert, key)],
            alpn: None,
            close_each_response: false,
            goaway_after_first: false,
            cut_after_first: false,
            count_body: false,
        }
    }
}

#[derive(Debug)]
struct Rotating {
    keys: Vec<Arc<CertifiedKey>>,
    served: AtomicU64,
}

impl rustls::server::ResolvesServerCert for Rotating {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let index = self.served.fetch_add(1, Ordering::Relaxed) as usize;
        self.keys.get(index.min(self.keys.len() - 1)).cloned()
    }
}

async fn origin(
    proto: Proto,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Origin {
    origin_with(OriginSpec::new(proto, cert, key)).await
}

async fn origin_with(spec: OriginSpec) -> Origin {
    let provider = provider();
    let keys = spec
        .keys
        .into_iter()
        .map(|(cert, key)| Arc::new(CertifiedKey::from_der(vec![cert], key, &provider).unwrap()))
        .collect();
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Rotating {
            keys,
            served: AtomicU64::new(0),
        }));
    config.alpn_protocols = spec.alpn.unwrap_or_else(|| spec.proto.alpn());
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let close_each_response = spec.close_each_response;
    let goaway_after_first = spec.goaway_after_first;
    let cut_after_first = spec.cut_after_first;
    let count_body = spec.count_body;
    let (body_seen_tx, body_seen) = tokio::sync::watch::channel(0u64);
    let body_seen_tx = Arc::new(body_seen_tx);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let requests = Arc::new(AtomicU64::new(0));
    let seen_connections = Arc::clone(&connections);
    let seen_requests = Arc::clone(&requests);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            seen_connections.fetch_add(1, Ordering::Relaxed);
            let acceptor = acceptor.clone();
            let requests = Arc::clone(&seen_requests);
            let body_seen_tx = Arc::clone(&body_seen_tx);
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let answered = Arc::new(tokio::sync::Notify::new());
                let signal = Arc::clone(&answered);
                let service = service_fn(move |request: Request<Incoming>| {
                    requests.fetch_add(1, Ordering::Relaxed);
                    let host_header = request.headers().contains_key(HOST);
                    let host = request
                        .headers()
                        .get(HOST)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string)
                        .or_else(|| request.uri().authority().map(|a| a.to_string()))
                        .unwrap_or_default();
                    let path = request.uri().path().to_string();
                    let signal = Arc::clone(&signal);
                    let body_seen_tx = Arc::clone(&body_seen_tx);
                    async move {
                        let mut received = 0u64;
                        if count_body {
                            let mut incoming = request.into_body();
                            while let Some(frame) = incoming.frame().await {
                                if let Ok(data) = frame.unwrap().into_data() {
                                    received += data.len() as u64;
                                    body_seen_tx.send_modify(|total| *total += data.len() as u64);
                                }
                            }
                        }
                        let body = match path.as_str() {
                            "/page" => page(),
                            path => Bytes::from(format!("origin:{path}")),
                        };
                        let mut response = Response::builder()
                            .header(CONTENT_TYPE, "application/octet-stream")
                            .header(SEEN_HOST, host)
                            .header(HOST_HEADER, if host_header { "present" } else { "absent" })
                            .header(BODY_BYTES, received.to_string());
                        if close_each_response {
                            response = response.header(CONNECTION, "close");
                        }
                        signal.notify_one();
                        Ok::<_, Infallible>(response.body(Full::new(body)).unwrap())
                    }
                });
                let builder = auto::Builder::new(TokioExecutor::new());
                let connection = builder.serve_connection(TokioIo::new(tls), service);
                tokio::pin!(connection);
                if goaway_after_first || cut_after_first {
                    tokio::select! {
                        _ = &mut connection => return,
                        () = answered.notified() => {}
                    }
                    if cut_after_first {
                        return;
                    }
                    connection.as_mut().graceful_shutdown();
                }
                let _ = connection.await;
            });
        }
    });
    Origin {
        addr,
        connections,
        requests,
        body_seen,
    }
}

fn connector(roots: &[CertificateDer<'static>], proto: Proto) -> TlsConnector {
    let mut store = RootCertStore::empty();
    for root in roots {
        store.add(root.clone()).unwrap();
    }
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(store)
        .with_no_client_auth();
    config.alpn_protocols = proto.alpn();
    TlsConnector::from(Arc::new(config))
}

async fn tls_to(addr: SocketAddr, connector: &TlsConnector) -> io::Result<TlsStream<TcpStream>> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    let name = ServerName::try_from(ORIGIN_NAME).unwrap();
    tokio::time::timeout(Duration::from_secs(5), connector.connect(name, tcp))
        .await
        .expect("a handshake outcome must arrive")
}

enum Client {
    H1(http1::SendRequest<Full<Bytes>>),
    H2(http2::SendRequest<Full<Bytes>>),
}

impl Client {
    async fn over(tls: TlsStream<TcpStream>, proto: Proto) -> Self {
        let io = TokioIo::new(tls);
        match proto {
            Proto::H1 => {
                let (sender, connection) = http1::handshake(io).await.unwrap();
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                Client::H1(sender)
            }
            Proto::H2 => {
                let (sender, connection) =
                    http2::handshake(TokioExecutor::new(), io).await.unwrap();
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                Client::H2(sender)
            }
        }
    }

    async fn get(&mut self, path: &str, accept: &str) -> Response<Incoming> {
        let request = match self {
            Client::H1(_) => Request::builder()
                .uri(path)
                .header(HOST, ORIGIN_NAME)
                .header(ACCEPT, accept),
            Client::H2(_) => Request::builder()
                .uri(format!("https://{ORIGIN_NAME}{path}"))
                .header(ACCEPT, accept),
        }
        .body(Full::new(Bytes::new()))
        .unwrap();
        let sent = match self {
            Client::H1(sender) => sender.send_request(request).await,
            Client::H2(sender) => sender.send_request(request).await,
        };
        tokio::time::timeout(Duration::from_secs(5), async { sent })
            .await
            .expect("a response must arrive")
            .expect("the request must succeed at the transport level")
    }
}

async fn body_of(response: Response<Incoming>) -> Bytes {
    response.into_body().collect().await.unwrap().to_bytes()
}

struct Harness {
    server: TlsServer,
    addr: SocketAddr,
    counters: Arc<ProxyCounters>,
    store: Arc<CertStore>,
    fah_root: Option<CertificateDer<'static>>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn shutdown(&self) {
        self.server.shutdown();
    }

    fn ours(&self, proto: Proto) -> TlsConnector {
        connector(self.fah_root.as_slice(), proto)
    }
}

struct Setup {
    clients: Vec<AllowedNet>,
    exclusions: ExclusionSet,
    upstream_roots: Vec<CertificateDer<'static>>,
    rules: Option<Arc<dyn fah_http::Ruleset>>,
    events: Option<mpsc::Sender<Event>>,
    idle: Duration,
    hello: Duration,
    ca: bool,
    max_connections: usize,
    listen: IpAddr,
}

impl Setup {
    fn intercepting(root: CertificateDer<'static>) -> Self {
        Self {
            clients: listed(),
            exclusions: ExclusionSet::empty(),
            upstream_roots: vec![root],
            rules: None,
            events: None,
            idle: Duration::from_secs(30),
            hello: Duration::from_secs(5),
            ca: true,
            max_connections: HttpsConfig::default().max_connections,
            listen: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }
}

fn listed() -> Vec<AllowedNet> {
    vec![AllowedNet::host(IpAddr::V4(Ipv4Addr::LOCALHOST))]
}

fn someone_else() -> Vec<AllowedNet> {
    vec![AllowedNet::host("192.0.2.1".parse().unwrap())]
}

fn store_with_ca() -> (tempfile::TempDir, Arc<CertStore>) {
    store(true)
}

fn store(with_ca: bool) -> (tempfile::TempDir, Arc<CertStore>) {
    fah_certs::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CertStore::open(dir.path()).unwrap());
    if with_ca {
        store.generate_ca(&CaParams::default()).unwrap();
    }
    (dir, store)
}

async fn harness(origin: SocketAddr, setup: Setup) -> Harness {
    let (dir, store) = self::store(setup.ca);
    let fah_root = store.ca_public_der().map(CertificateDer::from);

    let mut roots = RootCertStore::empty();
    for root in setup.upstream_roots {
        roots.add(root).unwrap();
    }
    let interception = Interception::new(
        fah_http::server_config(Arc::clone(&store)).unwrap(),
        fah_http::client_config_with_roots(roots).unwrap(),
        Arc::clone(&store),
        setup.clients,
        setup.exclusions,
    );

    let policy = DestinationPolicy::new(
        origin.port(),
        vec![AllowedNet::host(IpAddr::V4(Ipv4Addr::LOCALHOST))],
    );
    let mut proxy = TlsProxy::new(
        Arc::new(FixedResolver(vec![origin.ip()])),
        policy,
        origin.port(),
        setup.hello,
        setup.idle,
        NoSni::Pass,
    )
    .with_interception(interception);
    if let Some(rules) = setup.rules {
        proxy = proxy.with_rules(rules);
    }
    if let Some(events) = setup.events {
        proxy = proxy.with_events(events);
    }
    let proxy = Arc::new(proxy);
    let counters = proxy.counters();

    let config = HttpsConfig {
        listen: HttpsListenConfig {
            address: setup.listen.to_string(),
            port: 0,
        },
        max_connections: setup.max_connections,
        ..HttpsConfig::default()
    };
    let mut server = TlsServer::bind(&config).await.unwrap();
    let addr = server.local_addr();
    server.serve(proxy);
    Harness {
        server,
        addr,
        counters,
        store,
        fah_root,
        _dir: dir,
    }
}

async fn next_event(rx: &mut mpsc::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event must arrive")
        .expect("channel open")
}

fn https_event(event: Event) -> fah_model::RequestEvent {
    assert_eq!(event.kind(), EventKind::Https, "{event:?}");
    let Event::Https(event) = event else {
        unreachable!()
    };
    *event
}

async fn filtered_end_to_end(downstream: Proto, upstream: Proto) {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(upstream, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            rules: Some(rules_with(&format!("||{ORIGIN_NAME}/ads/\n"))),
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(downstream))
        .await
        .expect("a listed client trusting the CA must complete our handshake");
    assert_eq!(
        tls.get_ref().1.alpn_protocol(),
        Some(downstream.alpn()[0].as_slice()),
        "the client's ALPN choice must be honoured downstream"
    );
    let mut client = Client::over(tls, downstream).await;

    let blocked = client.get("/ads/pixel.gif", "image/*").await;
    assert_eq!(
        blocked.status(),
        StatusCode::OK,
        "an image block collapses quietly"
    );
    assert_eq!(blocked.headers()[CONTENT_TYPE], "image/gif");
    assert!(body_of(blocked).await.is_empty());
    assert_eq!(
        origin.requests.load(Ordering::Relaxed),
        0,
        "a blocked request must cost the origin nothing"
    );

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.request.path, "/ads/pixel.gif");
    assert_eq!(event.request.method, "GET");
    assert_eq!(event.request.resource_type, ResourceType::Image);
    assert_eq!(event.status, 200);
    assert_eq!(event.bytes, 0);
    match &event.verdict {
        Verdict::Block(rule) => assert!(rule.rule.contains("/ads/"), "{}", rule.rule),
        other => panic!("expected a block verdict, got {other:?}"),
    }

    let allowed = client.get("/page", "text/html").await;
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(
        allowed.headers()[SEEN_HOST],
        ORIGIN_NAME,
        "the origin must see the name the client asked for"
    );
    assert_eq!(
        allowed.headers()[HOST_HEADER],
        match upstream {
            Proto::H1 => "present",
            Proto::H2 => "absent",
        },
        "an h1 origin gets Host, an h2 origin gets :authority alone"
    );
    assert!(
        allowed.headers().contains_key("via"),
        "the relayed response carries our Via"
    );
    let body = body_of(allowed).await;
    assert_eq!(body.len(), PAGE_BYTES);
    assert_eq!(body, page(), "the relayed body must be byte-identical");
    assert_eq!(origin.requests.load(Ordering::Relaxed), 1);
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        1,
        "one verified upstream session serves the whole client connection"
    );

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.path, "/page");
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 200);
    assert_eq!(event.bytes, PAGE_BYTES as u64);

    let stats = harness.store.leaf_cache_stats();
    assert_eq!(stats.minted_total, 1, "one leaf for the one host");
    assert_eq!(stats.unwarmed_misses, 0, "the resolver never has to mint");
    let counters = harness.counters.snapshot();
    assert_eq!(counters.blocked, 1);
    assert_eq!(counters.upstream_cert_failures, 0);
    assert_eq!(counters.upstream_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn a_listed_client_gets_url_level_filtering_over_http1() {
    filtered_end_to_end(Proto::H1, Proto::H1).await;
}

#[tokio::test]
async fn a_listed_client_gets_url_level_filtering_over_h2() {
    filtered_end_to_end(Proto::H2, Proto::H2).await;
}

#[tokio::test]
async fn an_h2_client_is_bridged_to_an_http1_origin() {
    filtered_end_to_end(Proto::H2, Proto::H1).await;
}

#[tokio::test]
async fn an_http1_client_is_bridged_to_an_h2_origin() {
    filtered_end_to_end(Proto::H1, Proto::H2).await;
}

async fn spliced_not_intercepted(clients: Vec<AllowedNet>, exclusions: ExclusionSet) {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            clients,
            exclusions,
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let ours = tls_to(harness.addr, &harness.ours(Proto::H1)).await;
    assert!(
        ours.is_err(),
        "a spliced client trusting only our CA must reject the origin's certificate"
    );

    let tls = tls_to(
        harness.addr,
        &connector(std::slice::from_ref(&ca.root), Proto::H1),
    )
    .await
    .expect("a spliced client sees the origin's own certificate");
    let mut client = Client::over(tls, Proto::H1).await;
    let response = client.get("/page", "text/html").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !response.headers().contains_key("via"),
        "a spliced session is relayed untouched"
    );
    assert_eq!(body_of(response).await, page());
    drop(client);

    let stats = harness.store.leaf_cache_stats();
    assert_eq!(
        stats.minted_total, 0,
        "no leaf is ever minted for a spliced client"
    );
    assert_eq!(harness.counters.snapshot().upstream_cert_failures, 0);

    let first = next_event(&mut rx).await;
    assert_eq!(first.kind(), EventKind::HttpsSni, "{first:?}");
    let second = next_event(&mut rx).await;
    assert_eq!(second.kind(), EventKind::HttpsSni, "{second:?}");
    harness.shutdown();
}

#[tokio::test]
async fn a_client_not_on_the_list_is_never_intercepted() {
    spliced_not_intercepted(someone_else(), ExclusionSet::empty()).await;
}

#[tokio::test]
async fn an_excluded_sni_splices_even_for_a_listed_client() {
    spliced_not_intercepted(listed(), ExclusionSet::new(&[ORIGIN_NAME]).unwrap()).await;
}

#[tokio::test]
async fn an_unverifiable_upstream_never_yields_our_leaf() {
    let trusted_elsewhere = origin_ca();
    let (cert, key) = self_signed();
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(trusted_elsewhere.root.clone())
        },
    )
    .await;

    let outcome = tls_to(harness.addr, &harness.ours(Proto::H1)).await;
    assert!(
        outcome.is_err(),
        "the client must see a failed handshake, never a locally-signed success"
    );
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        1,
        "verification needs one upstream attempt"
    );

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.request.path, "");
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 526);
    assert_eq!(event.bytes, 0);

    let stats = harness.store.leaf_cache_stats();
    assert_eq!(
        stats.minted_total, 0,
        "no leaf is minted for an unverified upstream"
    );
    assert_eq!(stats.size, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_cert_failures, 1);
    assert_eq!(counters.upstream_failures, 0);
    assert_eq!(counters.blocked, 0);
    harness.shutdown();
}

#[tokio::test]
async fn a_host_that_is_not_the_verified_sni_is_refused_as_misdirected() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let mut sender = match Client::over(tls, Proto::H1).await {
        Client::H1(sender) => sender,
        Client::H2(_) => unreachable!(),
    };
    let request = Request::builder()
        .uri("/page")
        .header(HOST, "other.test")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(
        origin.requests.load(Ordering::Relaxed),
        0,
        "a request for another name never rides the verified session"
    );

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, "other.test");
    assert_eq!(event.status, 421);
    assert_eq!(harness.counters.snapshot().refused_claim, 1);
    harness.shutdown();
}

#[tokio::test]
async fn a_host_naming_another_port_is_refused_as_misdirected() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let Client::H1(mut sender) = Client::over(tls, Proto::H1).await else {
        unreachable!()
    };
    let elsewhere = origin.addr.port().wrapping_add(1).max(1);
    let request = Request::builder()
        .uri("/page")
        .header(HOST, format!("{ORIGIN_NAME}:{elsewhere}"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(
        origin.requests.load(Ordering::Relaxed),
        0,
        "the verified socket serves one port; a request for another never rides it"
    );
    let _ = body_of(response).await;

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.status, 421);
    assert_eq!(harness.counters.snapshot().refused_claim, 1);

    let good = Request::builder()
        .uri("/page")
        .header(HOST, ORIGIN_NAME)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(good).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the session itself survives a 421"
    );
    assert_eq!(body_of(response).await, page());
    harness.shutdown();
}

#[tokio::test]
async fn an_idle_intercepted_session_is_closed_and_its_permit_returned() {
    const IDLE: Duration = Duration::from_millis(300);
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H2, cert, key).await;
    let harness = harness(
        origin.addr,
        Setup {
            idle: IDLE,
            max_connections: 1,
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H2))
        .await
        .unwrap();
    let mut client = Client::over(tls, Proto::H2).await;
    let response = client.get("/first", "*/*").await;
    assert_eq!(body_of(response).await, Bytes::from("origin:/first"));

    tokio::time::sleep(IDLE * 3).await;
    let Client::H2(sender) = &mut client else {
        unreachable!()
    };
    let after_idle = tokio::time::timeout(Duration::from_secs(5), sender.ready()).await;
    assert!(
        matches!(after_idle, Ok(Err(_))) || sender.is_closed(),
        "the idle watchdog must close an intercepted session, h2 keep-alive or not"
    );

    let tls = tls_to(harness.addr, &harness.ours(Proto::H2))
        .await
        .expect("with max_connections = 1 a second session needs the idle-cut permit back");
    let mut second = Client::over(tls, Proto::H2).await;
    let response = second.get("/second", "*/*").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_of(response).await, Bytes::from("origin:/second"));
    harness.shutdown();
}

#[tokio::test]
async fn an_origin_that_closes_after_each_response_is_reconnected_with_verification() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin_with(OriginSpec {
        close_each_response: true,
        ..OriginSpec::new(Proto::H1, cert, key)
    })
    .await;
    let harness = harness(origin.addr, Setup::intercepting(ca.root.clone())).await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let mut client = Client::over(tls, Proto::H1).await;
    for round in 1..=3u64 {
        let response = client.get("/page", "text/html").await;
        assert_eq!(response.status(), StatusCode::OK, "round {round}");
        assert_eq!(body_of(response).await, page(), "round {round}");
        assert_eq!(
            origin.connections.load(Ordering::Relaxed),
            round,
            "each origin close costs one fresh, verified upstream session"
        );
    }
    assert_eq!(origin.requests.load(Ordering::Relaxed), 3);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_failures, 0);
    assert_eq!(counters.upstream_cert_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_h2_origin_that_goes_away_is_reconnected_with_verification() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin_with(OriginSpec {
        goaway_after_first: true,
        ..OriginSpec::new(Proto::H2, cert, key)
    })
    .await;
    let harness = harness(origin.addr, Setup::intercepting(ca.root.clone())).await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H2))
        .await
        .unwrap();
    let mut client = Client::over(tls, Proto::H2).await;
    let first = client.get("/first", "*/*").await;
    assert_eq!(body_of(first).await, Bytes::from("origin:/first"));
    tokio::time::sleep(Duration::from_millis(500)).await;

    let second = client.get("/second", "*/*").await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_of(second).await, Bytes::from("origin:/second"));
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        2,
        "GOAWAY must be answered by one fresh, verified upstream session"
    );
    assert_eq!(harness.counters.snapshot().upstream_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn a_reconnect_to_an_upstream_whose_certificate_changed_is_refused_as_526() {
    let ca = origin_ca();
    let (good_cert, good_key) = leaf_signed_by(&ca);
    let (bad_cert, bad_key) = self_signed();
    let origin = origin_with(OriginSpec {
        keys: vec![(good_cert, good_key), (bad_cert, bad_key)],
        close_each_response: true,
        ..OriginSpec::new(Proto::H1, self_signed().0, self_signed().1)
    })
    .await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let mut client = Client::over(tls, Proto::H1).await;
    let first = client.get("/first", "*/*").await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_of(first).await, Bytes::from("origin:/first"));
    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.status, 200);

    let second = client.get("/second", "*/*").await;
    assert_eq!(second.status().as_u16(), 526);
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        2,
        "the reconnect is verified against the new certificate"
    );
    assert_eq!(
        origin.requests.load(Ordering::Relaxed),
        1,
        "nothing is sent over an unverified reconnect"
    );
    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.path, "/second");
    assert_eq!(event.status, 526);
    assert_eq!(event.bytes, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_cert_failures, 1);
    assert_eq!(counters.upstream_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_origin_that_rejects_our_alpn_is_an_upstream_failure_not_a_certificate_failure() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin_with(OriginSpec {
        alpn: Some(vec![b"nothing-we-speak".to_vec()]),
        ..OriginSpec::new(Proto::H1, cert, key)
    })
    .await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let outcome = tls_to(harness.addr, &harness.ours(Proto::H1)).await;
    assert!(
        outcome.is_err(),
        "a failed upstream handshake closes the client"
    );
    assert_eq!(origin.connections.load(Ordering::Relaxed), 1);

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.status, 0, "not a certificate failure, so not 526");
    assert_eq!(event.bytes, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_cert_failures, 0);
    assert_eq!(counters.upstream_failures, 1);
    assert_eq!(harness.store.leaf_cache_stats().minted_total, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_upstream_that_refuses_the_connection_is_an_upstream_failure() {
    let ca = origin_ca();
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nobody = closed.local_addr().unwrap();
    drop(closed);
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        nobody,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let outcome = tls_to(harness.addr, &harness.ours(Proto::H1)).await;
    assert!(outcome.is_err());

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.verdict, Verdict::Pass);
    assert_eq!(event.status, 0);
    assert_eq!(event.bytes, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_failures, 1);
    assert_eq!(counters.upstream_cert_failures, 0);
    assert_eq!(counters.resolve_failures, 0);
    assert_eq!(harness.store.leaf_cache_stats().minted_total, 0);
    harness.shutdown();
}

async fn silent_origin() -> (SocketAddr, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&accepted);
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            seen.fetch_add(1, Ordering::Relaxed);
            held.push(stream);
        }
    });
    (addr, accepted)
}

#[tokio::test]
async fn an_upstream_that_never_answers_is_cut_off_at_the_hello_deadline() {
    const HELLO: Duration = Duration::from_millis(300);
    let ca = origin_ca();
    let (silent, accepted) = silent_origin().await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        silent,
        Setup {
            events: Some(tx),
            hello: HELLO,
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let started = Instant::now();
    let outcome = tls_to(harness.addr, &harness.ours(Proto::H1)).await;
    assert!(outcome.is_err());
    let elapsed = started.elapsed();
    assert!(
        elapsed >= HELLO,
        "the client is held until the upstream deadline, not refused early: {elapsed:?}"
    );
    assert!(
        elapsed < HELLO * 10,
        "the deadline must close the client long before the OS SYN retry window: {elapsed:?}"
    );
    assert_eq!(
        accepted.load(Ordering::Relaxed),
        1,
        "the upstream accepted the TCP connection and then said nothing"
    );

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.status, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_failures, 1);
    assert_eq!(counters.upstream_cert_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn a_listed_client_without_a_ca_is_closed_after_the_upstream_check() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ca: false,
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;
    assert!(harness.fah_root.is_none());

    let outcome = tls_to(
        harness.addr,
        &connector(std::slice::from_ref(&ca.root), Proto::H1),
    )
    .await;
    assert!(
        outcome.is_err(),
        "with no CA there is nothing to mint; the client is closed, not spliced"
    );
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        1,
        "the upstream is verified before the missing CA is discovered"
    );
    assert_eq!(origin.requests.load(Ordering::Relaxed), 0);

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, ORIGIN_NAME);
    assert_eq!(event.status, 0);
    let stats = harness.store.leaf_cache_stats();
    assert_eq!(stats.minted_total, 0);
    assert_eq!(stats.size, 0);
    let counters = harness.counters.snapshot();
    assert_eq!(counters.upstream_cert_failures, 0);
    assert_eq!(counters.upstream_failures, 0);
    harness.shutdown();
}

#[tokio::test]
async fn an_h2_authority_that_is_not_the_verified_sni_is_refused_as_misdirected() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H2, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H2))
        .await
        .unwrap();
    let Client::H2(mut sender) = Client::over(tls, Proto::H2).await else {
        unreachable!()
    };
    let request = Request::builder()
        .uri("https://other.test/page")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(origin.requests.load(Ordering::Relaxed), 0);

    let event = https_event(next_event(&mut rx).await);
    assert_eq!(event.request.host, "other.test");
    assert_eq!(event.request.path, "/page");
    assert_eq!(event.status, 421);
    assert_eq!(event.bytes, 0);
    assert_eq!(harness.counters.snapshot().refused_claim, 1);

    let good = Request::builder()
        .uri(format!("https://{ORIGIN_NAME}/page"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(good).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the session itself survives a 421"
    );
    assert_eq!(body_of(response).await, page());
    harness.shutdown();
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn ip(&mut self) -> IpAddr {
        if self.next() & 1 == 0 {
            IpAddr::V4(Ipv4Addr::from(self.next() as u32))
        } else {
            let high = u128::from(self.next()) << 64;
            IpAddr::V6(Ipv6Addr::from(high | u128::from(self.next())))
        }
    }

    fn domain(&mut self) -> String {
        let labels = 1 + (self.next() % 4) as usize;
        (0..labels)
            .map(|_| format!("l{}", self.next() % 1000))
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn empty_interception(store: &Arc<CertStore>, rng: &mut Rng) -> Interception {
    let exclusions = (0..(rng.next() % 8))
        .map(|_| rng.domain())
        .collect::<Vec<_>>();
    Interception::new(
        fah_http::server_config(Arc::clone(store)).unwrap(),
        fah_http::client_config().unwrap(),
        Arc::clone(store),
        Vec::new(),
        ExclusionSet::new(&exclusions).unwrap(),
    )
}

#[test]
fn an_empty_client_list_intercepts_nobody_whatever_else_the_config_says() {
    let (_dir, store) = store_with_ca();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..64 {
        let interception = empty_interception(&store, &mut rng);
        assert!(interception.is_empty());
        for _ in 0..256 {
            let ip = rng.ip();
            assert!(!interception.intercepts(ip), "{ip}");
        }
        let proxy = TlsProxy::new(
            Arc::new(FixedResolver(Vec::new())),
            DestinationPolicy::new(443, Vec::new()),
            443,
            Duration::from_secs(1),
            Duration::from_secs(1),
            NoSni::Pass,
        )
        .with_interception(interception);
        for _ in 0..256 {
            let ip = rng.ip();
            assert!(!proxy.intercepts(ip), "{ip}");
        }
        for fixed in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(192, 168, 88, 10)),
        ] {
            assert!(!proxy.intercepts(fixed), "{fixed}");
        }
    }
}

#[test]
fn a_listed_network_intercepts_exactly_its_members() {
    let (_dir, store) = store_with_ca();
    let proxy = TlsProxy::new(
        Arc::new(FixedResolver(Vec::new())),
        DestinationPolicy::new(443, Vec::new()),
        443,
        Duration::from_secs(1),
        Duration::from_secs(1),
        NoSni::Pass,
    )
    .with_interception(Interception::new(
        fah_http::server_config(Arc::clone(&store)).unwrap(),
        fah_http::client_config().unwrap(),
        store,
        vec![
            "192.168.88.0/24".parse().unwrap(),
            AllowedNet::host("fd00::10".parse().unwrap()),
        ],
        ExclusionSet::empty(),
    ));
    assert!(proxy.intercepts("192.168.88.1".parse().unwrap()));
    assert!(proxy.intercepts("192.168.88.254".parse().unwrap()));
    assert!(proxy.intercepts("fd00::10".parse().unwrap()));
    assert!(!proxy.intercepts("192.168.89.1".parse().unwrap()));
    assert!(!proxy.intercepts("fd00::11".parse().unwrap()));
    assert!(
        !proxy.intercepts("::ffff:192.168.88.1".parse().unwrap()),
        "the proxy canonicalizes peers before asking; the raw mapped form must not match"
    );
}

#[tokio::test]
async fn eight_parallel_h2_requests_share_one_verified_upstream_session() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H2, cert, key).await;
    let harness = harness(origin.addr, Setup::intercepting(ca.root.clone())).await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H2))
        .await
        .unwrap();
    let Client::H2(sender) = Client::over(tls, Proto::H2).await else {
        unreachable!()
    };
    let mut tasks = Vec::with_capacity(8);
    for index in 0..8 {
        let mut sender = sender.clone();
        tasks.push(tokio::spawn(async move {
            let request = Request::builder()
                .uri(format!("https://{ORIGIN_NAME}/page?n={index}"))
                .header(ACCEPT, "text/html")
                .body(Full::new(Bytes::new()))
                .unwrap();
            let response =
                tokio::time::timeout(Duration::from_secs(10), sender.send_request(request))
                    .await
                    .expect("eight concurrent streams must all be answered")
                    .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            body_of(response).await
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), page());
    }
    assert_eq!(origin.requests.load(Ordering::Relaxed), 8);
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        1,
        "eight parallel streams ride one verified upstream session"
    );
    harness.shutdown();
}

struct Trickle {
    chunks_left: usize,
    chunk: Bytes,
    sent: u64,
    window: u64,
    seen: tokio::sync::watch::Receiver<u64>,
    waiting: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl Trickle {
    fn new(
        chunks: usize,
        chunk: Bytes,
        window: u64,
        seen: tokio::sync::watch::Receiver<u64>,
    ) -> Self {
        Self {
            chunks_left: chunks,
            chunk,
            sent: 0,
            window,
            seen,
            waiting: None,
        }
    }
}

impl Body for Trickle {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        loop {
            if self.chunks_left == 0 {
                return Poll::Ready(None);
            }
            let needed = self.sent.saturating_sub(self.window);
            if *self.seen.borrow() >= needed {
                self.waiting = None;
                self.chunks_left -= 1;
                self.sent += self.chunk.len() as u64;
                return Poll::Ready(Some(Ok(Frame::data(self.chunk.clone()))));
            }
            if self.waiting.is_none() {
                let mut seen = self.seen.clone();
                self.waiting = Some(Box::pin(async move {
                    let _ = seen.changed().await;
                }));
            }
            match self.waiting.as_mut().unwrap().as_mut().poll(cx) {
                Poll::Ready(()) => self.waiting = None,
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[tokio::test]
async fn a_streamed_request_body_reaches_the_origin_before_the_client_finishes_sending() {
    const CHUNK: usize = 256 * 1024;
    const CHUNKS: usize = 16;
    const WINDOW: u64 = 1024 * 1024;
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin_with(OriginSpec {
        count_body: true,
        ..OriginSpec::new(Proto::H1, cert, key)
    })
    .await;
    let harness = harness(origin.addr, Setup::intercepting(ca.root.clone())).await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let (mut sender, connection) = http1::handshake::<_, Trickle>(TokioIo::new(tls))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let body = Trickle::new(
        CHUNKS,
        Bytes::from(vec![7u8; CHUNK]),
        WINDOW,
        origin.body_seen.clone(),
    );
    let request = Request::builder()
        .method("POST")
        .uri("/upload")
        .header(HOST, ORIGIN_NAME)
        .body(body)
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(20), sender.send_request(request))
        .await
        .expect(
            "a 4 MiB body must stream through: the client releases each chunk only after the \
             origin has seen the bytes a 1 MiB window behind it, so a proxy that held the body \
             would never let the client finish",
        )
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[BODY_BYTES],
        (CHUNK * CHUNKS).to_string(),
        "the origin received the whole body"
    );
    assert_eq!(*origin.body_seen.borrow(), (CHUNK * CHUNKS) as u64);
    harness.shutdown();
}

async fn second_session_gets_the_only_permit(harness: &Harness) {
    let tls = tokio::time::timeout(
        Duration::from_secs(5),
        tls_to(harness.addr, &harness.ours(Proto::H1)),
    )
    .await
    .expect("with max_connections = 1 the next session must get the permit back within 5 s")
    .expect("handshake");
    let mut client = Client::over(tls, Proto::H1).await;
    let response = client.get("/after", "*/*").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_of(response).await, Bytes::from("origin:/after"));
}

#[tokio::test]
async fn a_client_that_disconnects_mid_response_returns_its_permit() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let harness = harness(
        origin.addr,
        Setup {
            max_connections: 1,
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let (mut sender, connection) = http1::handshake::<_, Full<Bytes>>(TokioIo::new(tls))
        .await
        .unwrap();
    let connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri("/page")
        .header(HOST, ORIGIN_NAME)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    drop(sender);
    connection.abort();

    second_session_gets_the_only_permit(&harness).await;
    assert_eq!(origin.requests.load(Ordering::Relaxed), 2);
    harness.shutdown();
}

#[tokio::test]
async fn an_upstream_that_disconnects_mid_response_ends_the_session_and_returns_its_permit() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin_with(OriginSpec {
        cut_after_first: true,
        ..OriginSpec::new(Proto::H1, cert, key)
    })
    .await;
    let harness = harness(
        origin.addr,
        Setup {
            max_connections: 1,
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let (mut sender, connection) = http1::handshake::<_, Full<Bytes>>(TokioIo::new(tls))
        .await
        .unwrap();
    let connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri("/page")
        .header(HOST, ORIGIN_NAME)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        let response = sender.send_request(request).await?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map(|body| body.to_bytes());
        Ok::<_, hyper::Error>((status, body))
    })
    .await
    .expect("the cut origin must surface within 10 s");
    match &outcome {
        Ok((status, Ok(body))) if *status == StatusCode::OK && body.len() == PAGE_BYTES => {
            eprintln!("note: the origin's cut landed after the whole page was flushed")
        }
        Ok((status, body)) => eprintln!(
            "upstream cut surfaced to the client as status {status}, body {:?}",
            body.as_ref().map(|body| body.len())
        ),
        Err(err) => eprintln!("upstream cut surfaced to the client as {err}"),
    }
    drop(sender);
    connection.abort();

    second_session_gets_the_only_permit(&harness).await;
    assert_eq!(
        origin.connections.load(Ordering::Relaxed),
        2,
        "the second session verified a fresh upstream after the cut one"
    );
    harness.shutdown();
}

#[tokio::test]
async fn shutdown_stops_accepting_while_a_live_intercepted_session_keeps_serving() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let harness = harness(origin.addr, Setup::intercepting(ca.root.clone())).await;

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .unwrap();
    let mut client = Client::over(tls, Proto::H1).await;
    let response = client.get("/before", "*/*").await;
    assert_eq!(body_of(response).await, Bytes::from("origin:/before"));

    harness.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let response = client.get("/after-shutdown", "*/*").await;
    assert_eq!(
        body_of(response).await,
        Bytes::from("origin:/after-shutdown"),
        "shutdown aborts the accept loop only; a live session ends with the runtime (p3-04 L4)"
    );
    let refused = tokio::time::timeout(
        Duration::from_secs(2),
        tls_to(harness.addr, &harness.ours(Proto::H1)),
    )
    .await;
    assert!(
        !matches!(refused, Ok(Ok(_))),
        "no new session is accepted after shutdown"
    );
    assert_eq!(origin.connections.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn an_ipv6_listed_client_is_intercepted_end_to_end() {
    let ca = origin_ca();
    let (cert, key) = leaf_signed_by(&ca);
    let origin = origin(Proto::H1, cert, key).await;
    let (tx, mut rx) = mpsc::channel(16);
    let harness = harness(
        origin.addr,
        Setup {
            clients: vec![AllowedNet::host(IpAddr::V6(Ipv6Addr::LOCALHOST))],
            listen: IpAddr::V6(Ipv6Addr::LOCALHOST),
            rules: Some(rules_with(&format!("||{ORIGIN_NAME}/ads/\n"))),
            events: Some(tx),
            ..Setup::intercepting(ca.root.clone())
        },
    )
    .await;
    assert!(harness.addr.is_ipv6());

    let tls = tls_to(harness.addr, &harness.ours(Proto::H1))
        .await
        .expect("a listed v6 client trusting the CA completes our handshake over [::1]");
    let mut client = Client::over(tls, Proto::H1).await;
    let blocked = client.get("/ads/pixel.gif", "image/*").await;
    assert_eq!(blocked.status(), StatusCode::OK);
    assert!(body_of(blocked).await.is_empty());
    assert_eq!(origin.requests.load(Ordering::Relaxed), 0);

    let event = next_event(&mut rx).await;
    assert_eq!(event.client_ip(), IpAddr::V6(Ipv6Addr::LOCALHOST));
    let event = https_event(event);
    assert_eq!(event.request.path, "/ads/pixel.gif");
    assert!(matches!(event.verdict, Verdict::Block(_)));

    let allowed = client.get("/page", "text/html").await;
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(body_of(allowed).await, page());
    assert_eq!(harness.store.leaf_cache_stats().minted_total, 1);
    harness.shutdown();
}

#[tokio::test]
async fn the_baseline_exclusions_ship_without_any_configuration() {
    let set = ExclusionSet::new::<&str>(&[]).unwrap();
    assert!(set.contains("push.apple.com"));
    assert!(set.contains("play.googleapis.com"));
    assert!(set.contains("login.microsoftonline.com"));
    assert!(!set.contains(ORIGIN_NAME));
    let started = Instant::now();
    for _ in 0..10_000 {
        assert!(!set.contains("ads.example.com"));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
}
