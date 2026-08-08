//! Per-event cost of the Statistics fan-out (`Stats::record` / `record_http`).
//!
//! This runs in the binary's **single** event consumer, after the answer is
//! already on the wire — so it adds no query latency, but it is the one task
//! that must keep up with every completed query. At the 10 000 QPS budget in
//! PERFORMANCE.md the whole consumer has 100 µs per event; anything approaching
//! that shows up as `counters.events_dropped`, which silently under-counts both
//! Statistics and Metrics.
//!
//! Realistic shape matters more here than in the `fah-metrics` twin: both
//! aggregates and the client registry are keyed maps whose cost depends on how
//! many distinct domains and clients they already hold, so the fixtures
//! pre-populate them rather than hammering one key.

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use criterion::{criterion_group, criterion_main, Criterion};
use fah_config::{HistoryConfig, StatsConfig};
use fah_model::{
    DecisiveRule, Query, QueryEvent, QueryType, Request, RequestEvent, ResourceType, Verdict,
};
use fah_stats::Stats;

/// Distinct clients the registry holds. A household LAN with phones, laptops
/// and IoT sits here; one key would measure a hash hit and nothing else.
const CLIENTS: usize = 24;

/// Distinct domains cycled through, so the top-N tracking sees churn instead of
/// one already-hot entry.
const DOMAINS: usize = 512;

fn stats(dir: &tempfile::TempDir) -> Stats {
    Stats::new(
        &StatsConfig::default(),
        &HistoryConfig::default(),
        PathBuf::from(dir.path()),
    )
}

fn client(index: usize) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 10, (index % CLIENTS) as u8 + 2))
}

fn dns_event(index: usize, blocked: bool, cache_hit: bool) -> QueryEvent {
    let domain = format!("host{}.example{}.com", index % DOMAINS, index % 17);
    let verdict = if blocked {
        Verdict::Block(DecisiveRule::new("oisd-basic", "||ads.example.com^"))
    } else {
        Verdict::Pass
    };
    QueryEvent::new(
        Query::new(&domain, QueryType::A, client(index), SystemTime::now()),
        verdict,
        Duration::from_micros(if blocked { 20 } else { 100 }),
        cache_hit,
        false,
        None,
    )
}

fn http_event(index: usize) -> RequestEvent {
    RequestEvent::new(
        Request {
            host: format!("cdn{}.example.com", index % DOMAINS),
            path: "/assets/app.js".to_string(),
            method: "GET".to_string(),
            resource_type: ResourceType::Script,
            client_ip: client(index),
            timestamp: SystemTime::now(),
        },
        Verdict::Pass,
        Duration::from_micros(400),
        200,
        4096,
    )
}

/// Warms the registry and aggregates so the measured calls hit populated maps.
fn warmed(stats: &Stats) {
    for index in 0..(CLIENTS * 8) {
        stats.record(dns_event(
            index,
            index.is_multiple_of(3),
            index.is_multiple_of(2),
        ));
    }
}

fn bench_record(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut group = c.benchmark_group("stats_record");

    let blocked = stats(&dir);
    warmed(&blocked);
    let mut index = 0usize;
    group.bench_function("blocked", |b| {
        b.iter(|| {
            index = index.wrapping_add(1);
            blocked.record(black_box(dns_event(index, true, false)));
        })
    });

    let hit = stats(&dir);
    warmed(&hit);
    let mut index = 0usize;
    group.bench_function("pass_cache_hit", |b| {
        b.iter(|| {
            index = index.wrapping_add(1);
            hit.record(black_box(dns_event(index, false, true)));
        })
    });

    // The deployed mix: mostly pass, a third blocked, half served from cache.
    let mixed = stats(&dir);
    warmed(&mixed);
    let mut index = 0usize;
    group.bench_function("mixed", |b| {
        b.iter(|| {
            index = index.wrapping_add(1);
            mixed.record(black_box(dns_event(
                index,
                index.is_multiple_of(3),
                index.is_multiple_of(2),
            )));
        })
    });

    let http = stats(&dir);
    warmed(&http);
    let mut index = 0usize;
    group.bench_function("http", |b| {
        b.iter(|| {
            index = index.wrapping_add(1);
            http.record_http(black_box(http_event(index)));
        })
    });

    group.finish();
}

/// The event construction the arms above include, so it can be subtracted.
/// PERFORMANCE.md §Measuring reliably: a per-stage profile once attributed
/// 1.34 µs to header stripping when 1.28 µs of it was the harness.
fn bench_harness_cost(c: &mut Criterion) {
    let mut index = 0usize;
    c.bench_function("stats_record/harness_event_build", |b| {
        b.iter(|| {
            index = index.wrapping_add(1);
            black_box(dns_event(
                index,
                index.is_multiple_of(3),
                index.is_multiple_of(2),
            ));
        })
    });
}

criterion_group!(benches, bench_record, bench_harness_cost);
criterion_main!(benches);
