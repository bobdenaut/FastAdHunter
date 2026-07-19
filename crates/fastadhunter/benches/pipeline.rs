//! Whole-product benches vs PERFORMANCE.md §Budgets (p1-10).
//!
//! The per-crate benches cover the pieces in isolation — `fah-rules` proves
//! the compiled matcher's size and verdict latency at 1M domains, `fah-dns`
//! proves the cache-hit path, `fah-metrics` proves event recording. What is
//! left are the budget rows no single crate can answer, because they need the
//! whole assembly the binary builds: a blocked query end to end, the overhead
//! the engine adds to a forwarded query, sustained throughput, startup from
//! `/data` cached lists, and steady-state resident memory.
//!
//! This lives in the binary's package because it is the only crate allowed to
//! see every layer (ARCHITECTURE.md §Dependency Layering). `cargo bench -p
//! fastadhunter` runs it.
//!
//! Budget rows covered here:
//!
//! | PERFORMANCE.md row | bench |
//! | ------------------ | ----- |
//! | Blocked query, in-engine p99 < 1 ms | `blocked_query` |
//! | Forwarded query overhead, p99 < 1 ms | `forwarded_query_overhead` |
//! | Sustained throughput >= 10 000 QPS | `sustained_throughput` |
//! | Startup to serving, 1M-domain parse, < 3 s | `startup_from_cached_lists` |
//! | RAM steady-state, 1M loaded, <= 128 MB | `steady_state_memory` |

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use fah_config::{DnsCacheConfig, RuleListConfig, RulesConfig};
use fah_dns::{Forwarder, Pipeline, Transport};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};

/// Blocked-domain count for the startup and memory benches — PERFORMANCE.md
/// states both budgets against "1M blocked domains loaded".
const BLOCKLIST_SIZE: usize = 1_000_000;

/// Distinct domains cycled through the forward-path benches. Larger than the
/// cache so the timed loop keeps missing and actually walks the forward path
/// instead of settling into the cache-hit path `fah-dns`'s bench already
/// measures.
const FORWARD_DOMAINS: usize = 4096;

/// Concurrent in-flight queries in the throughput bench. The RB5009 has four
/// cores; a queue several times deeper than that is what a sustained-load
/// measurement needs to keep every core busy without measuring queueing.
const CONCURRENCY: usize = 64;

const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

/// Deterministic pseudo-random source — reproducible synthetic domains with no
/// `rand` dependency, matching `fah-rules`' matcher bench.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG constants.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// Blocklist-domain shapes matching real 1M lists (OISD, StevenBlack): a mix
/// of apex and one/two-label subdomains averaging ~20-22 bytes, so the memory
/// and parse figures reflect the budget's actual input.
fn blocked_domain(n: u64) -> String {
    const TLDS: [&str; 5] = ["com", "net", "org", "io", "co"];
    let tld = TLDS[(n % 5) as usize];
    match n % 4 {
        0 => format!("ads{n}.{tld}"),
        1 => format!("track{n}.{tld}"),
        2 => format!("cdn{n}.metrics.{tld}"),
        _ => format!("node{n}.ad.{tld}"),
    }
}

/// A hosts-format blocklist of `count` domains — the format most real 1M lists
/// ship in, so the startup bench parses what the device will actually parse.
fn hosts_blocklist(count: usize) -> String {
    let mut text = String::with_capacity(count * 32);
    for n in 0..count as u64 {
        text.push_str("0.0.0.0 ");
        text.push_str(&blocked_domain(n));
        text.push('\n');
    }
    text
}

/// Answers instantly with a fixed A record. Upstream RTT is deliberately
/// excluded — PERFORMANCE.md: "In-engine latency excludes upstream RTT — we
/// measure what we add."
#[derive(Clone)]
struct InstantForwarder {
    calls: Arc<AtomicU64>,
}

impl InstantForwarder {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Forwarder for InstantForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<Message> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        let name = request
            .queries
            .first()
            .map(|query| query.name().clone())
            .unwrap_or_else(|| Name::from_str("example.com.").unwrap());
        response.add_answer(Record::from_rdata(
            name,
            300,
            RData::A(A(Ipv4Addr::new(93, 184, 216, 34))),
        ));
        Ok(response)
    }
}

fn encode_query(domain: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(&format!("{domain}.")).expect("valid domain"),
        RecordType::A,
    ));
    message.to_vec().expect("encodable query")
}

/// A `ListManager` holding `rules` as the user-rules block — no network, no
/// `/data` list files, so the bench setup stays offline and fast.
async fn manager_with_user_rules(data_dir: &std::path::Path, rules: String) -> Arc<ListManager> {
    let manager = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.to_path_buf(),
        )
        .expect("list manager"),
    );
    manager.set_user_rules(rules).await;
    manager
}

/// The pipeline the listeners call into, with the event channel drained the
/// way the binary's fan-out task drains it — a full channel would otherwise
/// turn the bench into a measurement of the drop path.
fn build_pipeline(
    manager: Arc<ListManager>,
    forwarder: InstantForwarder,
) -> Pipeline<InstantForwarder> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    Pipeline::new(manager, forwarder, 10, &DnsCacheConfig::default(), tx)
}

/// PERFORMANCE.md: "Blocked query, in-engine p99 < 1 ms". The whole path —
/// decode, verdict, synthesize 0.0.0.0, encode — never touching cache or
/// upstream (ADR-0001: the Rule Engine runs first and blocked queries stop
/// there).
fn bench_blocked_query(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let forwarder = InstantForwarder::new();

    let pipeline = rt.block_on(async {
        let manager =
            manager_with_user_rules(data_dir.path(), "||ads.example.com^\n".to_string()).await;
        build_pipeline(manager, forwarder.clone())
    });

    let raw = encode_query("tracker.ads.example.com");

    // Prove the query really is blocked before timing it — a bench of an
    // accidentally-forwarded query would report a meaningless number.
    let response = rt
        .block_on(pipeline.handle(&raw, CLIENT, Transport::Udp))
        .expect("blocked query answers");
    let decoded = Message::from_vec(&response).expect("decodable response");
    assert_eq!(
        decoded.answers.len(),
        1,
        "expected a synthesized blocked answer"
    );
    assert_eq!(
        forwarder.calls.load(Ordering::Relaxed),
        0,
        "a blocked query must never reach an upstream"
    );

    let mut group = c.benchmark_group("full_pipeline");
    group.bench_function("blocked_query", |b| {
        b.iter(|| rt.block_on(pipeline.handle(black_box(&raw), CLIENT, Transport::Udp)));
    });
    group.finish();

    assert_eq!(
        forwarder.calls.load(Ordering::Relaxed),
        0,
        "the forwarder was reached during the timed loop — the verdict path changed"
    );
}

/// PERFORMANCE.md: "Forwarded query overhead added by engine, p99 < 1 ms".
/// The forwarder answers instantly, so what criterion times is exactly the
/// engine's share: decode, verdict, cache miss, cache insert, encode.
fn bench_forwarded_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let forwarder = InstantForwarder::new();

    let pipeline = rt.block_on(async {
        let manager =
            manager_with_user_rules(data_dir.path(), "||ads.example.com^\n".to_string()).await;
        build_pipeline(manager, forwarder.clone())
    });

    // More distinct domains than the cache holds, so the timed loop keeps
    // taking the forward path rather than decaying into cache hits.
    let queries: Vec<Vec<u8>> = (0..FORWARD_DOMAINS)
        .map(|i| encode_query(&format!("host{i}.forwarded.example.net")))
        .collect();

    let before = forwarder.calls.load(Ordering::Relaxed);
    let mut group = c.benchmark_group("full_pipeline");
    group.bench_function("forwarded_query_overhead", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let raw = &queries[i % queries.len()];
            i += 1;
            rt.block_on(pipeline.handle(black_box(raw), CLIENT, Transport::Udp))
        });
    });
    group.finish();

    assert!(
        forwarder.calls.load(Ordering::Relaxed) > before,
        "no query reached the forwarder — this measured the cache, not the forward path"
    );
}

/// PERFORMANCE.md: "Sustained throughput >= 10 000 QPS". A realistic mix —
/// blocked, cache-hit and forwarded queries — driven concurrently, because
/// the budget is about the assembled system under load, not one query at a
/// time. Reported as elements/second by criterion's throughput support.
fn bench_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let forwarder = InstantForwarder::new();

    let pipeline = rt.block_on(async {
        let manager =
            manager_with_user_rules(data_dir.path(), "||ads.example.com^\n".to_string()).await;
        Arc::new(build_pipeline(manager, forwarder.clone()))
    });

    // Roughly a real resolver's shape: a third blocked, a third repeat
    // (cache-hit) traffic, a third fresh names that must be forwarded.
    let mut rng = Lcg(0x2545_f491_4f6c_dd1d);
    let workload: Vec<Vec<u8>> = (0..CONCURRENCY * 3)
        .map(|i| match i % 3 {
            0 => encode_query(&format!("t{}.ads.example.com", rng.next() % 1024)),
            1 => encode_query("cached.example.org"),
            _ => encode_query(&format!("fresh{}.example.net", rng.next())),
        })
        .collect();
    let workload = Arc::new(workload);

    // Warm the cache-hit third so the steady-state mix is what gets measured.
    rt.block_on(pipeline.handle(&encode_query("cached.example.org"), CLIENT, Transport::Udp));

    let mut group = c.benchmark_group("full_pipeline");
    group.throughput(Throughput::Elements(workload.len() as u64));
    group.bench_function("sustained_throughput", |b| {
        b.iter_custom(|iters| {
            let pipeline = Arc::clone(&pipeline);
            let workload = Arc::clone(&workload);
            rt.block_on(async move {
                let started = Instant::now();
                for _ in 0..iters {
                    let mut handles = Vec::with_capacity(workload.len());
                    for index in 0..workload.len() {
                        let pipeline = Arc::clone(&pipeline);
                        let workload = Arc::clone(&workload);
                        handles.push(tokio::spawn(async move {
                            pipeline
                                .handle(&workload[index], CLIENT, Transport::Udp)
                                .await
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await.expect("query task"));
                    }
                }
                started.elapsed()
            })
        });
    });
    group.finish();
}

/// PERFORMANCE.md: "Startup to serving (cached lists, 1M-domain parse) 1-3 s".
/// Measures exactly what the binary does before the listeners bind:
/// `ListManager::new` then `boot()` — read `/data`, parse, compile, swap in.
/// No network (RULE_ENGINE.md: boot must not wait on it).
fn bench_startup(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    // Lay down the `/data` cache copy the way a previous run's refresh would
    // have: `<data>/lists/<id>.raw`.
    let lists_dir = data_dir.path().join("lists");
    std::fs::create_dir_all(&lists_dir).unwrap();
    std::fs::write(
        lists_dir.join("blocklist-1m.raw"),
        hosts_blocklist(BLOCKLIST_SIZE),
    )
    .unwrap();

    let config = RulesConfig {
        refresh_hours_default: 24,
        lists: vec![RuleListConfig {
            id: "blocklist-1m".to_string(),
            url: "https://example.invalid/blocklist.txt".to_string(),
            enabled: true,
            refresh_hours: None,
        }],
    };

    let mut group = c.benchmark_group("startup");
    // A 1M-domain parse takes on the order of a second; criterion's default
    // 100 samples would run for minutes to say the same thing.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));
    group.bench_function("startup_from_cached_lists", |b| {
        b.iter(|| {
            rt.block_on(async {
                let manager =
                    ListManager::new(&config, data_dir.path().to_path_buf()).expect("list manager");
                manager.boot().await;
                black_box(manager.matcher().len())
            })
        });
    });
    group.finish();
}

/// PERFORMANCE.md: "RAM steady-state, 1M blocked domains loaded <= 128 MB" and
/// "Compiled ruleset for 1M domains <= 40 MB".
///
/// Not a timed bench — a one-shot measurement printed alongside the others,
/// because criterion measures durations and this budget is about bytes. The
/// compiled-ruleset figure is hardware-independent and asserted; the RSS
/// figure comes from `/proc` and is therefore Linux-only, so on other dev
/// machines it is reported as unavailable rather than faked. The number that
/// counts against the 128 MB budget is the RB5009's, measured in p1-11.
fn report_memory(_c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();

    let lists_dir = data_dir.path().join("lists");
    std::fs::create_dir_all(&lists_dir).unwrap();
    std::fs::write(
        lists_dir.join("blocklist-1m.raw"),
        hosts_blocklist(BLOCKLIST_SIZE),
    )
    .unwrap();

    let config = RulesConfig {
        refresh_hours_default: 24,
        lists: vec![RuleListConfig {
            id: "blocklist-1m".to_string(),
            url: "https://example.invalid/blocklist.txt".to_string(),
            enabled: true,
            refresh_hours: None,
        }],
    };

    let forwarder = InstantForwarder::new();
    let (manager, pipeline) = rt.block_on(async {
        let manager =
            Arc::new(ListManager::new(&config, data_dir.path().to_path_buf()).expect("manager"));
        manager.boot().await;
        let pipeline = build_pipeline(Arc::clone(&manager), forwarder);
        (manager, pipeline)
    });

    // Drive real traffic through the assembled pipeline so the figure is
    // steady-state under load, not just-after-boot.
    let queries: Vec<Vec<u8>> = (0..FORWARD_DOMAINS)
        .map(|i| encode_query(&format!("host{i}.steady.example.net")))
        .collect();
    rt.block_on(async {
        for raw in &queries {
            pipeline.handle(raw, CLIENT, Transport::Udp).await;
        }
    });

    let matcher = manager.matcher();
    let ruleset_bytes = matcher.heap_bytes();
    let ruleset_mb = ruleset_bytes as f64 / (1024.0 * 1024.0);

    let rss = fah_metrics::resident_memory_bytes();
    let rss_report = if rss == 0 {
        "unavailable on this platform (/proc/self/status is Linux-only; \
         the RB5009 figure is measured in p1-11)"
            .to_string()
    } else {
        format!(
            "{:.1} MiB (budget: <= 128 MB)",
            rss as f64 / (1024.0 * 1024.0)
        )
    };

    println!(
        "\n[p1-10] steady-state memory, {} rules loaded and {} queries served:\n  \
         compiled ruleset: {ruleset_bytes} bytes ({ruleset_mb:.1} MiB) (budget: <= 40 MB)\n  \
         process RSS: {rss_report}\n",
        matcher.len(),
        queries.len()
    );

    assert!(
        ruleset_bytes <= 40 * 1024 * 1024,
        "compiled 1M-domain ruleset is {ruleset_mb:.1} MiB, over the 40 MB budget"
    );
}

criterion_group!(
    benches,
    bench_blocked_query,
    bench_forwarded_overhead,
    bench_throughput,
    bench_startup,
    report_memory
);
criterion_main!(benches);
