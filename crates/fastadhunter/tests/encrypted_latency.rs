mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;

use common::{a_query, boot, insecure_client_config, put_user_rules, run_mock_upstream, Ports};

const QUERIES: usize = 2_000;
const ROUNDS: usize = 3;
const DOMAIN: &str = "ads.example.com";
const DOT_HOSTNAME: &str = "dns.fah.test";

fn config_toml(ports: &Ports, upstream: SocketAddr) -> String {
    let dns_port = ports.dns;
    let dot_port = ports.dot;
    let api_port = ports.api;
    format!(
        r#"
[engine]
mode = "dns"

[dns.listen]
address = "127.0.0.1"
port = {dns_port}
dot_port = {dot_port}

[dns.upstreams]
strategy = "fallback"
timeout_ms = 2000

[[dns.upstreams.servers]]
address = "{upstream}"
protocol = "udp"

[rules]
refresh_hours_default = 24
lists = []

[api]
address = "127.0.0.1"
port = {api_port}
tls = true

[log]
level = "warn"
format = "text"
"#
    )
}

struct Sample {
    label: &'static str,
    sorted_micros: Vec<u128>,
}

impl Sample {
    fn new(label: &'static str, mut micros: Vec<u128>) -> Self {
        micros.sort_unstable();
        Self {
            label,
            sorted_micros: micros,
        }
    }

    fn at(&self, quantile: f64) -> u128 {
        let index = ((self.sorted_micros.len() - 1) as f64 * quantile).round() as usize;
        self.sorted_micros[index]
    }

    fn report(&self) {
        println!(
            "{:<10} n={} min={}us p50={}us p90={}us p99={}us max={}us",
            self.label,
            self.sorted_micros.len(),
            self.sorted_micros[0],
            self.at(0.50),
            self.at(0.90),
            self.at(0.99),
            self.sorted_micros[self.sorted_micros.len() - 1],
        );
    }
}

fn p50(micros: &[u128]) -> u128 {
    let mut sorted = micros.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() - 1) / 2]
}

async fn time_udp(socket: &UdpSocket, request: &[u8]) -> Vec<u128> {
    let mut buffer = vec![0u8; 4096];
    let mut micros = Vec::with_capacity(QUERIES);
    for _ in 0..QUERIES {
        let started = Instant::now();
        socket.send(request).await.expect("send");
        socket.recv(&mut buffer).await.expect("recv");
        micros.push(started.elapsed().as_micros());
    }
    micros
}

async fn time_dot(
    port: u16,
    request: &[u8],
    tls: Arc<rustls::ClientConfig>,
) -> (Duration, Vec<u128>) {
    let tcp = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .expect("connect DoT");
    let name = rustls::pki_types::ServerName::try_from(DOT_HOSTNAME).expect("sni");
    let handshake_started = Instant::now();
    let mut stream = tokio_rustls::TlsConnector::from(tls)
        .connect(name, tcp)
        .await
        .expect("DoT handshake");
    let handshake = handshake_started.elapsed();
    let len = u16::try_from(request.len()).expect("short").to_be_bytes();
    let mut micros = Vec::with_capacity(QUERIES);
    for _ in 0..QUERIES {
        let started = Instant::now();
        stream.write_all(&len).await.expect("len");
        stream.write_all(request).await.expect("query");
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).await.expect("reply len");
        let mut reply = vec![0u8; u16::from_be_bytes(len_buf) as usize];
        stream.read_exact(&mut reply).await.expect("reply");
        micros.push(started.elapsed().as_micros());
    }
    (handshake, micros)
}

async fn time_doh(
    http: &reqwest::Client,
    url: &str,
    request: &[u8],
) -> (Option<reqwest::Version>, Vec<u128>) {
    let mut micros = Vec::with_capacity(QUERIES);
    let mut version = None;
    for _ in 0..QUERIES {
        let started = Instant::now();
        let response = http
            .post(url)
            .header("content-type", "application/dns-message")
            .body(request.to_vec())
            .send()
            .await
            .expect("POST /dns-query");
        version.get_or_insert(response.version());
        let _ = response.bytes().await.expect("body");
        micros.push(started.elapsed().as_micros());
    }
    (version, micros)
}

#[ignore = "diagnostic: dev-box per-query latency for the p3-06 budget rows"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_query_latency_udp_vs_dot_vs_doh() {
    let config_dir = tempfile::tempdir().expect("config volume");
    let data_dir = tempfile::tempdir().expect("data volume");
    let upstream = UdpSocket::bind("127.0.0.1:0").await.expect("mock upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    tokio::spawn(run_mock_upstream(upstream));
    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("http client");
    let (_child, ports, base) = boot(config_dir.path(), data_dir.path(), &http, |ports| {
        config_toml(ports, upstream_addr)
    })
    .await;
    let key = std::fs::read_to_string(config_dir.path().join("apikey"))
        .expect("api key")
        .trim()
        .to_string();
    put_user_rules(&http, &base, &key, &[&format!("||{DOMAIN}^")]).await;
    let request = a_query(DOMAIN);

    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client socket");
    socket
        .connect(SocketAddr::from((Ipv4Addr::LOCALHOST, ports.dns)))
        .await
        .expect("connect");
    let tls = Arc::new(insecure_client_config());
    let url = format!("{base}/dns-query");

    let mut udp = Vec::with_capacity(QUERIES * ROUNDS);
    let mut dot = Vec::with_capacity(QUERIES * ROUNDS);
    let mut doh = Vec::with_capacity(QUERIES * ROUNDS);
    let mut handshakes = Vec::with_capacity(ROUNDS);
    let mut doh_version = None;
    println!(
        "blocked-domain round trips from a loopback client, {ROUNDS} rounds x {QUERIES} \
         sequential queries per transport, transports interleaved within each round"
    );
    for round in 1..=ROUNDS {
        let udp_round = time_udp(&socket, &request).await;
        let (handshake, dot_round) = time_dot(ports.dot, &request, Arc::clone(&tls)).await;
        let (version, doh_round) = time_doh(&http, &url, &request).await;
        println!(
            "round {round}: p50 udp={}us dot={}us doh={}us; DoT handshake {:?}",
            p50(&udp_round),
            p50(&dot_round),
            p50(&doh_round),
            handshake
        );
        udp.extend(udp_round);
        dot.extend(dot_round);
        doh.extend(doh_round);
        handshakes.push(handshake);
        doh_version = doh_version.or(version);
    }

    println!(
        "pooled over {ROUNDS} rounds; DoT handshakes (excluded from per-query figures) {:?}; \
         DoH over {:?}",
        handshakes, doh_version
    );
    for sample in [
        Sample::new("udp", udp),
        Sample::new("dot", dot),
        Sample::new("doh-post", doh),
    ] {
        sample.report();
    }
}
