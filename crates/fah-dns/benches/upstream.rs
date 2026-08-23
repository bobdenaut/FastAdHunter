use std::hint::black_box;
use std::io;
use std::net::SocketAddr;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_config::{DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy};
use fah_dns::{Forwarder, UpstreamPool};
use hickory_proto::op::{Message, OpCode, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use tokio::net::UdpSocket;

fn pool_for(addr: SocketAddr) -> UpstreamPool {
    UpstreamPool::from_config(&DnsUpstreamsConfig {
        strategy: UpstreamStrategy::Fallback,
        timeout_ms: 2_000,
        servers: vec![UpstreamServerConfig {
            address: addr.to_string(),
            protocol: UpstreamProtocol::Udp,
            hostname: None,
        }],
        ..Default::default()
    })
    .unwrap()
}

fn a_query() -> Message {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
    ));
    message
}

async fn answering_server() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            let (len, client) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            let mut response = Message::response(request.metadata.id, OpCode::Query);
            response.metadata.response_code = ResponseCode::NoError;
            response.queries = request.queries.clone();
            socket
                .send_to(&response.to_vec().unwrap(), client)
                .await
                .unwrap();
        }
    });
    addr
}

async fn refusing_addr() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.local_addr().unwrap()
}

fn bench_answered(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async { pool_for(answering_server().await) });
    let query = a_query();
    rt.block_on(pool.forward(&query)).unwrap();

    let mut group = c.benchmark_group("upstream");
    group.bench_function("forward_udp_answered", |b| {
        b.iter(|| rt.block_on(pool.forward(black_box(&query))).unwrap());
    });
    group.finish();
}

fn bench_refused(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let pool = rt.block_on(async { pool_for(refusing_addr().await) });
    let query = a_query();
    let probe = rt.block_on(pool.forward(&query)).unwrap_err();
    if probe.kind() == io::ErrorKind::TimedOut {
        eprintln!(
            "skipping forward_udp_refused: this host does not surface ICMP port-unreachable on connected UDP sockets"
        );
        return;
    }

    let mut group = c.benchmark_group("upstream");
    group.bench_function("forward_udp_refused", |b| {
        b.iter(|| rt.block_on(pool.forward(black_box(&query))).unwrap_err());
    });
    group.finish();
}

criterion_group!(benches, bench_answered, bench_refused);
criterion_main!(benches);
