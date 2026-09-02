//! Shared machinery for the tests that spawn the real `fastadhunter` binary.
//!
//! Extracted from `e2e.rs` when `http_e2e.rs` needed the same boot sequence.
//! What lives here is the part that is expensive to get right rather than
//! merely long: the ephemeral-port retry loop and the platform-specific
//! `is_port_conflict` needles. Each test owns its own config and assertions.

// Each test binary uses a subset; `mod common` compiles the whole file into
// every one of them, so the unused remainder is expected rather than dead.
#![allow(dead_code)]

use std::cell::Cell;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, RecordType};
use serde_json::Value;
use tokio::net::UdpSocket;

/// What the mock upstream answers for anything it is asked.
pub const UPSTREAM_IP: Ipv4Addr = Ipv4Addr::new(93, 184, 216, 34);

/// How many times to re-pick ports when another process claims the one we
/// just released. Three losses in a row is a machine problem, not a race.
const BOOT_ATTEMPTS: u32 = 3;

const DNS_PORT_DRAWS: u32 = 512;

/// Kills the spawned binary when the test ends, however it ends.
pub struct Guard(pub Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn binary_under_test() -> PathBuf {
    static ANNOUNCED: Once = Once::new();
    let Some(raw) = std::env::var_os("FAH_E2E_BINARY") else {
        return PathBuf::from(env!("CARGO_BIN_EXE_fastadhunter"));
    };
    let path = std::path::absolute(&raw).unwrap_or_else(|err| {
        panic!("FAH_E2E_BINARY={raw:?} cannot be resolved to an absolute path: {err}")
    });
    assert!(
        path.is_file(),
        "FAH_E2E_BINARY={raw:?} resolves to {}, which is not a file; a relative value resolves \
         against the test process's working directory, {}",
        path.display(),
        std::env::current_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default()
    );
    ANNOUNCED.call_once(|| eprintln!("FAH_E2E_BINARY override active: {}", path.display()));
    path
}

/// The ports a booted instance was given.
pub struct Ports {
    pub dns: u16,
    pub api: u16,
    pub dot: u16,
    http: Cell<Option<u16>>,
    https: Cell<Option<u16>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApiScheme {
    Https,
    Http,
}

impl ApiScheme {
    fn base(self, port: u16) -> String {
        match self {
            ApiScheme::Https => format!("https://127.0.0.1:{port}"),
            ApiScheme::Http => format!("http://127.0.0.1:{port}"),
        }
    }
}

impl Ports {
    pub fn https(&self) -> u16 {
        match self.https.get() {
            Some(port) => port,
            None => {
                let port = free_tcp_port();
                self.https.set(Some(port));
                port
            }
        }
    }

    /// The HTTP listener's port, drawn on first use.
    ///
    /// Lazy on purpose: every ephemeral port drawn is another chance to land in
    /// a WinNAT-reserved block (see [`is_port_conflict`]), and three draws per
    /// attempt made the DNS-only test lose the race noticeably more often than
    /// two. A config that never mentions HTTP now never pays for it.
    pub fn http(&self) -> u16 {
        match self.http.get() {
            Some(port) => port,
            None => {
                let port = free_tcp_port();
                self.http.set(Some(port));
                port
            }
        }
    }
}

/// Boots the binary on freshly-picked ports and waits for it to serve.
///
/// `config` is called once per attempt, because a retry must render the new
/// ports rather than the ones that lost the race.
///
/// Ports are chosen by binding an ephemeral socket and releasing it, which is
/// inherently racy: between the release and the engine's own `bind`, anything
/// else on the machine can claim the port — and under `cargo test --workspace`
/// there are dozens of other tests binding ephemeral sockets in parallel. That
/// race made this flaky, so a lost race is retried with fresh ports instead of
/// being reported as a product failure. Every *other* startup failure still
/// fails immediately, with the engine's log attached — masking those is exactly
/// what this must not do.
pub async fn boot(
    config_dir: &Path,
    data_dir: &Path,
    client: &reqwest::Client,
    config: impl Fn(&Ports) -> String,
) -> (Guard, Ports, String) {
    boot_with(config_dir, data_dir, client, ApiScheme::Https, config).await
}

pub async fn boot_with(
    config_dir: &Path,
    data_dir: &Path,
    client: &reqwest::Client,
    scheme: ApiScheme,
    config: impl Fn(&Ports) -> String,
) -> (Guard, Ports, String) {
    for attempt in 1..=BOOT_ATTEMPTS {
        let ports = Ports {
            dns: free_udp_port(),
            api: free_tcp_port(),
            dot: free_tcp_port(),
            http: Cell::new(None),
            https: Cell::new(None),
        };

        let config_path = config_dir.join("fastadhunter.toml");
        std::fs::write(&config_path, config(&ports)).expect("write config");

        // The engine's output goes to a file rather than the terminal: a
        // failing assertion should be readable, not buried in the log — but
        // when the binary dies during startup, that log is the only thing that
        // can say why, so it has to be kept somewhere retrievable.
        let log_path = config_dir.join("engine.log");
        let log = std::fs::File::create(&log_path).expect("engine log");
        let child = Guard(
            Command::new(binary_under_test())
                .arg("--config")
                .arg(&config_path)
                .arg("--data")
                .arg(data_dir)
                .stdout(Stdio::from(log.try_clone().expect("clone log handle")))
                .stderr(Stdio::from(log))
                .spawn()
                .expect("spawn fastadhunter"),
        );

        let base = scheme.base(ports.api);
        match await_api_ready(client, &base, child, &log_path).await {
            Ok(child) => return (child, ports, base),
            // Dropping the guard on the way out of the match killed the child.
            Err(log) if is_port_conflict(&log) && attempt < BOOT_ATTEMPTS => continue,
            Err(log) => panic!(
                "fastadhunter failed to start (attempt {attempt}/{BOOT_ATTEMPTS})\
                 \n--- engine log ---\n{log}"
            ),
        }
    }
    unreachable!("the loop either returns or panics on the last attempt")
}

/// Polls the public health endpoint until the API answers. Returns the child
/// on success, or the engine's log on failure — a dead child would otherwise
/// show up as an opaque connection-refused timeout.
async fn await_api_ready(
    client: &reqwest::Client,
    base: &str,
    mut child: Guard,
    log_path: &Path,
) -> Result<Guard, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.0.try_wait() {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!("exited with {status}\n{log}"));
        }
        if let Ok(response) = client.get(format!("{base}/health")).send().await {
            if response.status().is_success() {
                let body: Value = response.json().await.expect("health json");
                assert_eq!(body["status"], "ok");
                return Ok(child);
            }
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            return Err(format!("never became ready at {base}\n{log}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Whether a startup failure was the port we picked being unusable, rather
/// than a defect worth failing the test over.
fn is_port_conflict(log: &str) -> bool {
    let log = log.to_ascii_lowercase();
    [
        // Taken: Windows WSAEADDRINUSE, Linux EADDRINUSE, and the prose forms.
        "10048",
        "os error 98",
        "address already in use",
        "socket address",
        // Reserved: Windows WSAEACCES. Hyper-V/WinNAT reserves whole blocks of
        // ephemeral ports (`netsh interface ipv4 show excludedportrange
        // protocol=tcp`), and binding inside one fails with "an attempt was
        // made to access a socket in a way forbidden by its access
        // permissions" — not "in use". Without this the harness treats a
        // reserved port as a real defect and fails the whole gate; it made the
        // suite flake roughly one run in four on this box.
        "10013",
        "forbidden by its access permissions",
    ]
    .iter()
    .any(|needle| log.contains(needle))
}

// ─── DNS ────────────────────────────────────────────────────────────────

/// Answers every query with a single A record. Stands in for a real resolver
/// so the test never leaves the machine (PERFORMANCE.md's "in-engine latency
/// excludes upstream RTT" applies here too: this upstream is instant).
pub async fn run_mock_upstream(socket: UdpSocket) {
    run_mock_upstream_answering(socket, UPSTREAM_IP).await
}

/// As [`run_mock_upstream`], but names the address every question resolves to.
/// The HTTP test points every host at its own loopback origin this way.
pub async fn run_mock_upstream_answering(socket: UdpSocket, answer: Ipv4Addr) {
    let mut buffer = vec![0u8; 4096];
    loop {
        let Ok((len, from)) = socket.recv_from(&mut buffer).await else {
            return;
        };
        let Ok(request) = Message::from_vec(&buffer[..len]) else {
            continue;
        };
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        if let Some(query) = request.queries.first() {
            response.add_query(query.clone());
            response.add_answer(hickory_proto::rr::Record::from_rdata(
                query.name().clone(),
                300,
                RData::A(A(answer)),
            ));
        }
        let Ok(bytes) = response.to_vec() else {
            continue;
        };
        let _ = socket.send_to(&bytes, from).await;
    }
}

pub struct Answer {
    pub rcode: ResponseCode,
    pub a_records: Vec<Ipv4Addr>,
    pub ttl: Option<u32>,
}

/// One real UDP query against the binary's listener, the way any client on the
/// LAN would ask.
pub async fn resolve(port: u16, domain: &str) -> Answer {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client socket");
    socket
        .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .expect("connect to the DNS listener");

    let request = a_query(domain);
    socket.send(&request).await.expect("send query");

    let mut buffer = vec![0u8; 4096];
    let len = tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buffer))
        .await
        .unwrap_or_else(|_| panic!("no DNS response for {domain} within 5s"))
        .expect("receive response");

    decode_answer(&buffer[..len])
}

pub fn a_query(domain: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(&format!("{domain}.")).expect("valid name"),
        RecordType::A,
    ));
    message.to_vec().expect("encodable query")
}

pub fn decode_answer(bytes: &[u8]) -> Answer {
    let response = Message::from_vec(bytes).expect("decodable response");
    Answer {
        rcode: response.metadata.response_code,
        a_records: response
            .answers
            .iter()
            .filter_map(|record| match record.data {
                RData::A(a) => Some(a.0),
                _ => None,
            })
            .collect(),
        ttl: response.answers.first().map(|record| record.ttl),
    }
}

pub async fn resolve_dot(
    port: u16,
    domain: &str,
    tls: Arc<rustls::ClientConfig>,
    sni: &str,
) -> Answer {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let tcp = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .expect("connect to the DoT listener");
    let name = rustls::pki_types::ServerName::try_from(sni.to_string()).expect("a valid SNI");
    let mut stream = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_rustls::TlsConnector::from(tls).connect(name, tcp),
    )
    .await
    .expect("DoT handshake within 10s")
    .expect("DoT handshake");

    let request = a_query(domain);
    let len = u16::try_from(request.len())
        .expect("a short query")
        .to_be_bytes();
    stream.write_all(&len).await.expect("send length");
    stream.write_all(&request).await.expect("send query");

    let mut len_buf = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len_buf))
        .await
        .unwrap_or_else(|_| panic!("no DoT response for {domain} within 5s"))
        .expect("response length");
    let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
    stream.read_exact(&mut reply).await.expect("response body");
    decode_answer(&reply)
}

pub async fn resolve_doh_post(client: &reqwest::Client, base: &str, domain: &str) -> Answer {
    let response = client
        .post(format!("{base}/dns-query"))
        .header("content-type", "application/dns-message")
        .body(a_query(domain))
        .send()
        .await
        .expect("POST /dns-query");
    doh_answer(response).await
}

pub async fn resolve_doh_get(client: &reqwest::Client, base: &str, domain: &str) -> Answer {
    let response = client
        .get(format!(
            "{base}/dns-query?dns={}",
            base64url(&a_query(domain))
        ))
        .send()
        .await
        .expect("GET /dns-query");
    doh_answer(response).await
}

async fn doh_answer(response: reqwest::Response) -> Answer {
    assert_eq!(response.status(), 200, "DoH must answer 200");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/dns-message")
    );
    let body = response.bytes().await.expect("DoH body");
    decode_answer(&body)
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut word = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            word |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..=chunk.len() {
            let sextet = (word >> (18 - 6 * index)) & 0x3F;
            out.push(ALPHABET[sextet as usize] as char);
        }
    }
    out
}

pub fn client_config_trusting(ca_der: Vec<u8>) -> Arc<rustls::ClientConfig> {
    fah_api::install_crypto_provider();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(ca_der))
        .expect("the exported CA parses as a root");
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

// ─── API ────────────────────────────────────────────────────────────────

pub async fn get_json(client: &reqwest::Client, base: &str, key: &str, path: &str) -> Value {
    let response = client
        .get(format!("{base}{path}"))
        .bearer_auth(key)
        .send()
        .await
        .unwrap_or_else(|err| panic!("GET {path}: {err}"));
    assert!(
        response.status().is_success(),
        "GET {path} returned {}",
        response.status()
    );
    response.json().await.expect("json body")
}

pub async fn post_json(client: &reqwest::Client, base: &str, key: &str, path: &str) -> Value {
    let response = client
        .post(format!("{base}{path}"))
        .bearer_auth(key)
        .send()
        .await
        .unwrap_or_else(|err| panic!("POST {path}: {err}"));
    assert!(
        response.status().is_success(),
        "POST {path} returned {}",
        response.status()
    );
    response.json().await.expect("json body")
}

/// Replaces the user-rules block and waits for the API to confirm — the
/// handler recompiles the ruleset before responding, so a 200 here means the
/// next query already sees the new verdict.
pub async fn put_user_rules(client: &reqwest::Client, base: &str, key: &str, rules: &[&str]) {
    let response = client
        .put(format!("{base}/api/v1/rules/user"))
        .bearer_auth(key)
        .json(&serde_json::json!({ "rules": rules }))
        .send()
        .await
        .expect("PUT /api/v1/rules/user");
    assert!(
        response.status().is_success(),
        "PUT /api/v1/rules/user returned {}",
        response.status()
    );
}

/// Statistics are fed by the binary's event fan-out task, so they land a beat
/// after the query is answered — poll rather than sleep on a guessed delay.
pub async fn await_stats<F>(client: &reqwest::Client, base: &str, key: &str, ready: F) -> Value
where
    F: Fn(&Value) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stats = get_json(client, base, key, "/api/v1/stats").await;
        if ready(&stats) {
            return stats;
        }
        assert!(
            Instant::now() < deadline,
            "statistics never caught up with the queries: {stats}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ─── WebSocket ──────────────────────────────────────────────────────────

pub async fn connect_events(
    base: &str,
    key: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!(
        "{}/api/v1/events?token={key}",
        base.replace("https://", "wss://")
    );
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(insecure_client_config()));
    let (socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
            .await
            .expect("the events socket must accept a ?token= upgrade");
    socket
}

/// A rustls client that accepts the self-signed appliance certificate —
/// tungstenite has no `danger_accept_invalid_certs` switch, so the
/// verification bypass is spelled out here. Test-only.
pub fn insecure_client_config() -> rustls::ClientConfig {
    fah_api::install_crypto_provider();

    #[derive(Debug)]
    struct AcceptAny;

    impl rustls::client::danger::ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny))
        .with_no_client_auth()
}

// ─── ports ──────────────────────────────────────────────────────────────

/// Draws a port free on both UDP and TCP — what the DNS listener binds.
/// Windows keeps separate excluded-port ranges per protocol, so a port the OS
/// hands out for UDP can still be TCP-reserved; failed draws are held so the
/// allocator advances past a whole reserved block. Hard-coding instead would
/// collide with whatever the developer already runs (an AdGuard Home on 53).
pub fn free_udp_port() -> u16 {
    let mut rejected = Vec::new();
    for _ in 0..DNS_PORT_DRAWS {
        let udp = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind an ephemeral UDP port");
        let port = udp.local_addr().expect("local addr").port();
        if std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return port;
        }
        rejected.push(udp);
    }
    panic!("no ephemeral port was bindable on both UDP and TCP in {DNS_PORT_DRAWS} draws")
}

pub fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral TCP port")
        .local_addr()
        .expect("local addr")
        .port()
}

pub const ORIGIN_PORT: u16 = 443;
pub const PAGE_HOST: &str = "shop.example.com";
pub const AD_HOST: &str = "ads.example.com";
pub const DOT_HOSTNAME: &str = "dns.fah.test";

pub struct FullMode {
    pub origin_ip: Ipv4Addr,
    pub clients: Vec<String>,
    pub api_tls: bool,
}

pub fn full_mode_config(ports: &Ports, upstream: SocketAddr, mode: &FullMode) -> String {
    let dns_port = ports.dns;
    let dot_port = ports.dot;
    let api_port = ports.api;
    let http_port = ports.http();
    let https_port = ports.https();
    let origin_ip = mode.origin_ip;
    let api_tls = mode.api_tls;
    let clients = mode
        .clients
        .iter()
        .map(|client| format!("{client:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
[engine]
mode = "dns+http+https"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}
dot_port = {dot_port}

[dns.blocking]
mode = "null_ip"
ttl_seconds = 10

[dns.upstreams]
strategy = "fallback"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "{upstream}"
protocol = "udp"

[http.listen]
address = "127.0.0.1"
port = {http_port}

[https]
hello_timeout_ms = 5000
idle_timeout_ms = 10000

[https.listen]
address = "127.0.0.1"
port = {https_port}

[https.interception]
clients = [{clients}]

[egress]
allow_destinations = ["{origin_ip}"]

[rules]
refresh_hours_default = 24
lists = []

[stats]
snapshot_interval_seconds = 300

[api]
address = "127.0.0.1"
port = {api_port}
tls = {api_tls}

[log]
level = "info"
format = "text"
"#
    )
}

pub struct Instance {
    pub child: Guard,
    pub ports: Ports,
    pub base: String,
    pub key: String,
    pub http: reqwest::Client,
    pub config_dir: tempfile::TempDir,
    pub data_dir: tempfile::TempDir,
}

pub async fn boot_full(mode: FullMode) -> Instance {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    boot_full_in(config_dir, data_dir, mode).await
}

pub async fn boot_full_in(
    config_dir: tempfile::TempDir,
    data_dir: tempfile::TempDir,
    mode: FullMode,
) -> Instance {
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream_answering(upstream, mode.origin_ip));
    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");
    let scheme = if mode.api_tls {
        ApiScheme::Https
    } else {
        ApiScheme::Http
    };
    let (child, ports, base) =
        boot_with(config_dir.path(), data_dir.path(), &http, scheme, |ports| {
            full_mode_config(ports, upstream_addr, &mode)
        })
        .await;
    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("first boot must persist an API key")
        .trim()
        .to_string();
    Instance {
        child,
        ports,
        base,
        key,
        http,
        config_dir,
        data_dir,
    }
}

impl Instance {
    pub async fn generate_ca(&self) -> Vec<u8> {
        let generated = self
            .http
            .post(format!("{}/api/v1/certificates/ca/generate", self.base))
            .bearer_auth(&self.key)
            .json(&serde_json::json!({ "confirm": true }))
            .send()
            .await
            .expect("generate a CA");
        assert_eq!(generated.status(), 200, "CA generation must succeed");
        self.export_ca("der").await
    }

    pub async fn export_ca(&self, format: &str) -> Vec<u8> {
        let response = self
            .http
            .get(format!(
                "{}/api/v1/certificates/ca/export?format={format}",
                self.base
            ))
            .bearer_auth(&self.key)
            .send()
            .await
            .expect("export the CA");
        assert_eq!(response.status(), 200, "CA export ({format}) must succeed");
        response.bytes().await.expect("CA export body").to_vec()
    }

    pub async fn certificates(&self) -> Value {
        get_json(&self.http, &self.base, &self.key, "/api/v1/certificates").await
    }

    pub fn engine_log(&self) -> String {
        std::fs::read_to_string(self.config_dir.path().join("engine.log")).unwrap_or_default()
    }
}

pub fn self_signed_origin(
    hosts: &[&str],
) -> (
    rustls::pki_types::CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let key = rcgen::KeyPair::generate().expect("origin key pair");
    let params = rcgen::CertificateParams::new(
        hosts
            .iter()
            .map(|host| host.to_string())
            .collect::<Vec<_>>(),
    )
    .expect("origin params");
    let cert = params
        .self_signed(&key)
        .expect("self-signed origin certificate");
    (
        cert.der().clone(),
        rustls::pki_types::PrivateKeyDer::try_from(key.serialize_der()).expect("origin key der"),
    )
}

pub async fn bind_origin(ip: Ipv4Addr) -> Option<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind((ip, ORIGIN_PORT)).await {
        Ok(listener) => Some(listener),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            None
        }
        Err(err) => panic!("binding the origin on {ip}:{ORIGIN_PORT}: {err}"),
    }
}

pub fn skip_origin_message(ip: Ipv4Addr) -> String {
    format!(
        "SKIPPED: {ip}:{ORIGIN_PORT} is unavailable, and the binary originates HTTPS to port \
         {ORIGIN_PORT} only. Free the port to run this test."
    )
}

pub struct OriginCounters {
    pub accepts: Arc<std::sync::atomic::AtomicU64>,
    pub handshakes: Arc<std::sync::atomic::AtomicU64>,
}

pub fn run_tls_origin(
    listener: tokio::net::TcpListener,
    cert: rustls::pki_types::CertificateDer<'static>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
    payload: Arc<Vec<u8>>,
) -> OriginCounters {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::AsyncWriteExt;

    fah_api::install_crypto_provider();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("origin server config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let counters = OriginCounters {
        accepts: Arc::new(AtomicU64::new(0)),
        handshakes: Arc::new(AtomicU64::new(0)),
    };
    let accepts = Arc::clone(&counters.accepts);
    let handshakes = Arc::clone(&counters.handshakes);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            accepts.fetch_add(1, Ordering::Relaxed);
            let acceptor = acceptor.clone();
            let payload = Arc::clone(&payload);
            let handshakes = Arc::clone(&handshakes);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                handshakes.fetch_add(1, Ordering::Relaxed);
                let _ = tls.write_all(&payload).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    counters
}

pub fn run_raw_origin(
    listener: tokio::net::TcpListener,
    payload: Arc<Vec<u8>>,
) -> Arc<std::sync::atomic::AtomicU64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::io::AsyncWriteExt;

    let accepts = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&accepts);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::Relaxed);
            let payload = Arc::clone(&payload);
            tokio::spawn(async move {
                let (mut reader, mut writer) = stream.into_split();
                tokio::spawn(async move {
                    let mut sink = tokio::io::sink();
                    let _ = tokio::io::copy(&mut reader, &mut sink).await;
                });
                let _ = writer.write_all(&payload).await;
                let _ = writer.shutdown().await;
            });
        }
    });
    accepts
}

pub fn pseudo_random_payload(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

pub fn client_hello(host: &str) -> Vec<u8> {
    let mut entry = vec![0u8];
    entry.extend_from_slice(&(host.len() as u16).to_be_bytes());
    entry.extend_from_slice(host.as_bytes());
    let mut sni = Vec::new();
    sni.extend_from_slice(&(entry.len() as u16).to_be_bytes());
    sni.extend_from_slice(&entry);
    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0u16.to_be_bytes());
    extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni);
    hello_record(&extensions)
}

pub fn client_hello_without_sni() -> Vec<u8> {
    hello_record(&[])
}

fn hello_record(extensions: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&[0x42; 32]);
    body.push(0);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&[0x13, 0x01]);
    body.push(1);
    body.push(0);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(extensions);
    let mut handshake = vec![0x01u8];
    handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    handshake.extend_from_slice(&body);
    let mut record = vec![0x16u8, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

pub async fn tls_connect_from(
    from: Option<Ipv4Addr>,
    port: u16,
    sni: &str,
    tls: Arc<rustls::ClientConfig>,
) -> std::io::Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
    let socket = tokio::net::TcpSocket::new_v4()?;
    if let Some(address) = from {
        socket.bind(SocketAddr::from((address, 0)))?;
    }
    let tcp = socket
        .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await?;
    let name = rustls::pki_types::ServerName::try_from(sni.to_string())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio_rustls::TlsConnector::from(tls).connect(name, tcp),
    )
    .await
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
}

pub async fn raw_exchange(addr: SocketAddr, hello: &[u8]) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    stream.set_nodelay(true).expect("nodelay");
    stream.write_all(hello).await.expect("send the hello");
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut received))
        .await
        .expect("the peer must close within 10 s")
        .expect("read to end");
    received
}

pub async fn await_event<S, F>(socket: &mut S, what: &str, ready: F) -> Value
where
    S: futures_util::StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
    F: Fn(&Value) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "no `query` event matching {what} arrived on the events socket"
        );
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .unwrap_or_else(|_| panic!("no `query` event matching {what} arrived"))
            .expect("the socket closed early")
            .expect("socket error");
        let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(&text).expect("event json");
        if json["type"] == "query" && ready(&json["data"]) {
            return json["data"].clone();
        }
    }
}

pub fn password_from_log(log: &str) -> Option<String> {
    let marker = "dashboard_password=";
    let start = log.find(marker)? + marker.len();
    let rest = &log[start..];
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let password = rest[..end].trim_matches('"');
    (!password.is_empty()).then(|| password.to_string())
}

pub async fn login_cookie(instance: &Instance, password: &str) -> String {
    let response = instance
        .http
        .post(format!("{}/api/v1/auth/login", instance.base))
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .expect("POST /api/v1/auth/login");
    assert!(
        response.status().is_success(),
        "login with the generated password answered {}",
        response.status()
    );
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .expect("a session cookie");
    cookie.split(';').next().expect("cookie pair").to_string()
}

pub fn decode_base64(text: &[u8]) -> Vec<u8> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in text {
        let Some(v) = value(c) else {
            continue;
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

pub struct Needles {
    pub label: String,
    base64: Vec<u8>,
    der: Vec<u8>,
}

impl Needles {
    pub fn from_pem(label: &str, pem: &str) -> Self {
        let base64: String = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .map(str::trim)
            .collect();
        assert!(
            base64.len() >= 64,
            "{label}: the PEM body is too short to be a key"
        );
        let der = decode_base64(base64.as_bytes());
        Self {
            label: label.to_string(),
            base64: base64.into_bytes(),
            der,
        }
    }

    pub fn found_in(&self, body: &[u8]) -> Option<&'static str> {
        let compact: Vec<u8> = body
            .iter()
            .copied()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        let mut unescaped = Vec::with_capacity(compact.len());
        let mut index = 0;
        while index < compact.len() {
            if compact[index] == b'\\' && compact.get(index + 1) == Some(&b'n') {
                index += 2;
                continue;
            }
            unescaped.push(compact[index]);
            index += 1;
        }
        if contains(&unescaped, &self.base64) {
            return Some("whole base64 payload");
        }
        if contains(&unescaped, &self.base64[..48]) {
            return Some("leading base64 chunk");
        }
        if contains(body, &self.der) {
            return Some("DER bytes");
        }
        for header in [
            "BEGIN PRIVATE KEY",
            "BEGIN EC PRIVATE KEY",
            "BEGIN RSA PRIVATE KEY",
            "BEGIN ENCRYPTED PRIVATE KEY",
        ] {
            if contains(body, header.as_bytes()) {
                return Some("PEM private-key header");
            }
        }
        None
    }
}

#[test]
fn free_udp_port_is_bindable_on_both_protocols() {
    for _ in 0..32 {
        let port = free_udp_port();
        let udp = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, port));
        let tcp = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port));
        assert!(
            udp.is_ok(),
            "port {port} must be bindable on UDP: {:?}",
            udp.err()
        );
        assert!(
            tcp.is_ok(),
            "port {port} must be bindable on TCP: {:?}",
            tcp.err()
        );
    }
}
