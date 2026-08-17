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
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;
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

/// The ports a booted instance was given.
pub struct Ports {
    pub dns: u16,
    pub api: u16,
    http: Cell<Option<u16>>,
}

impl Ports {
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
    for attempt in 1..=BOOT_ATTEMPTS {
        let ports = Ports {
            dns: free_udp_port(),
            api: free_tcp_port(),
            http: Cell::new(None),
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
            Command::new(env!("CARGO_BIN_EXE_fastadhunter"))
                .arg("--config")
                .arg(&config_path)
                .arg("--data")
                .arg(data_dir)
                .stdout(Stdio::from(log.try_clone().expect("clone log handle")))
                .stderr(Stdio::from(log))
                .spawn()
                .expect("spawn fastadhunter"),
        );

        let base = format!("https://127.0.0.1:{}", ports.api);
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

    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(&format!("{domain}.")).expect("valid name"),
        RecordType::A,
    ));
    let request = message.to_vec().expect("encodable query");

    socket.send(&request).await.expect("send query");

    let mut buffer = vec![0u8; 4096];
    let len = tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buffer))
        .await
        .unwrap_or_else(|_| panic!("no DNS response for {domain} within 5s"))
        .expect("receive response");

    let response = Message::from_vec(&buffer[..len]).expect("decodable response");
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
