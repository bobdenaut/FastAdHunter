//! Hot-path instrumentation-cost bench (p1-08 acceptance: single-digit ns per
//! event — `Metrics::record` runs on every query alongside the DNS pipeline).

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime};

use criterion::{criterion_group, criterion_main, Criterion};
use fah_metrics::Metrics;
use fah_model::{DecisiveRule, Query, QueryEvent, QueryType, Verdict};

fn pass_event() -> QueryEvent {
    QueryEvent::new(
        Query::new(
            "example.com",
            QueryType::A,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            SystemTime::now(),
        ),
        Verdict::Pass,
        Duration::from_micros(100),
        true,
        false,
        false,
    )
}

fn block_event() -> QueryEvent {
    QueryEvent::new(
        Query::new(
            "ads.example.com",
            QueryType::A,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            SystemTime::now(),
        ),
        Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^")),
        Duration::from_micros(20),
        false,
        false,
        false,
    )
}

fn forward_event() -> QueryEvent {
    QueryEvent::new(
        Query::new(
            "cdn.example.com",
            QueryType::A,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            SystemTime::now(),
        ),
        Verdict::Pass,
        Duration::from_millis(12),
        false,
        true,
        false,
    )
}

fn stale_event() -> QueryEvent {
    QueryEvent::new(
        Query::new(
            "news.example.com",
            QueryType::A,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            SystemTime::now(),
        ),
        Verdict::Pass,
        Duration::from_secs(2),
        true,
        true,
        true,
    )
}

fn bench_record(c: &mut Criterion) {
    let metrics = Metrics::new();
    let pass = pass_event();
    c.bench_function("record cache-hit pass event", |b| {
        b.iter(|| metrics.record(black_box(&pass)));
    });

    let block = block_event();
    c.bench_function("record blocked event", |b| {
        b.iter(|| metrics.record(black_box(&block)));
    });

    // Cycles all four event shapes so the verdict/stage branches don't
    // stay perfectly predicted — closer to real mixed traffic than the
    // single-event benches above.
    let mixed = [pass_event(), block_event(), forward_event(), stale_event()];
    let mut i = 0usize;
    c.bench_function("record mixed events", |b| {
        b.iter(|| {
            i = (i + 1) % mixed.len();
            metrics.record(black_box(&mixed[i]));
        });
    });
}

criterion_group!(benches, bench_record);
criterion_main!(benches);
