use std::hint::black_box;
use std::io;
use std::net::SocketAddr;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_config::{DnsUpstreamsConfig, UpstreamProtocol, UpstreamServerConfig, UpstreamStrategy};
use fah_dns::{Forwarder, UpstreamPool};
use hickory_proto::op::{Message, OpCode, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use tokio::net::UdpSocket;

const STRATEGIES: [(UpstreamStrategy, &str); 2] = [
    (UpstreamStrategy::Fallback, ""),
    (UpstreamStrategy::Adaptive, "_adaptive"),
];

fn pool_for(addr: SocketAddr, strategy: UpstreamStrategy) -> UpstreamPool {
    UpstreamPool::from_config(&DnsUpstreamsConfig {
        strategy,
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
    let addr = rt.block_on(answering_server());
    let query = a_query();

    let mut group = c.benchmark_group("upstream");
    for (strategy, suffix) in STRATEGIES {
        let pool = pool_for(addr, strategy);
        rt.block_on(pool.forward(&query)).unwrap();
        group.bench_function(format!("forward_udp_answered{suffix}"), |b| {
            b.iter(|| rt.block_on(pool.forward(black_box(&query))).unwrap());
        });
    }
    group.finish();
}

const REFUSED_ARMS: [(UpstreamStrategy, &str); 2] = [
    (UpstreamStrategy::Fallback, "forward_udp_refused"),
    (
        UpstreamStrategy::Adaptive,
        "forward_udp_refused_adaptive_penalized_forced",
    ),
];

const REFUSED_WARMUPS: usize = 4;

fn bench_refused(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(refusing_addr());
    let query = a_query();

    let mut group = c.benchmark_group("upstream");
    for (strategy, name) in REFUSED_ARMS {
        let pool = pool_for(addr, strategy);
        let probe = rt.block_on(pool.forward(&query)).unwrap_err();
        if probe.kind() == io::ErrorKind::TimedOut {
            eprintln!(
                "skipping {name}: this host does not surface ICMP port-unreachable on connected UDP sockets"
            );
            continue;
        }
        for _ in 1..REFUSED_WARMUPS {
            rt.block_on(pool.forward(&query)).unwrap_err();
        }
        group.bench_function(name, |b| {
            b.iter(|| rt.block_on(pool.forward(black_box(&query))).unwrap_err());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_answered, bench_refused);
criterion_main!(benches);
