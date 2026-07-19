//! Cache-hit in-engine latency vs PERFORMANCE.md budget (p1-05): "Verdict +
//! cache hit, in-engine p99" < 1 ms. `DnsCache` is crate-private, so this
//! benches the whole [`Pipeline::handle`] path (decode -> verdict -> cache
//! hit -> encode) through the same public API the listeners use — the
//! number that matters is what a real query actually pays, not an isolated
//! hashmap lookup.
//!
//! `cargo bench -p fah-dns` runs this.

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_config::{DnsCacheConfig, RulesConfig};
use fah_dns::{Forwarder, Pipeline, Transport};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};

/// Answers exactly once so the bench can assert the cache — not the
/// forwarder — served every timed iteration.
#[derive(Clone)]
struct OnceForwarder {
    calls: Arc<AtomicU64>,
}

impl Forwarder for OnceForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<Message> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        response.add_answer(Record::from_rdata(
            Name::from_str("example.com.").unwrap(),
            300,
            RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
        ));
        Ok(response)
    }
}

fn encode_query() -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str("example.com.").unwrap(),
        RecordType::A,
    ));
    message.to_vec().unwrap()
}

fn bench_cache_hit(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicU64::new(0));

    let pipeline = rt.block_on(async {
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
        let forwarder = OnceForwarder {
            calls: calls.clone(),
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        Pipeline::new(manager, forwarder, 10, &DnsCacheConfig::default(), tx)
    });

    let raw = encode_query();
    let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

    // Warm the cache with exactly one forwarded query; everything the timed
    // loop below does must be answered from the cache instead.
    rt.block_on(pipeline.handle(&raw, client_ip, Transport::Udp));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "expected exactly one forward while warming the cache"
    );

    println!(
        "\n[p1-05] cache-hit bench: forwarder called {} time(s) before the timed loop \
         (PERFORMANCE.md budget: cache hit adds < 1ms in-engine)\n",
        calls.load(Ordering::Relaxed)
    );

    let mut group = c.benchmark_group("dns_cache");
    group.bench_function("cache_hit_in_engine_latency", |b| {
        b.iter(|| rt.block_on(pipeline.handle(black_box(&raw), client_ip, Transport::Udp)));
    });
    group.finish();

    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "the forwarder was called again during the timed loop — the cache stopped \
         short-circuiting, which would invalidate this bench's latency number"
    );
}

criterion_group!(benches, bench_cache_hit);
criterion_main!(benches);
