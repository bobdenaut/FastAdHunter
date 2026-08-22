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
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Transport};
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};

/// Blocked-domain count for the startup and memory benches — PERFORMANCE.md
/// states both budgets against "1M blocked domains loaded".
const BLOCKLIST_SIZE: usize = 1_000_000;

/// Distinct domains cycled through the forward-path benches — more than the
/// default cache's 10 000 entries, so with oldest-first eviction the entry a
/// query would hit is always gone again before the cycle returns to it. The
/// timed loop therefore misses every time and actually walks the forward path
/// — including the eviction a full cache pays per insert, which is the steady
/// state a long-running resolver serves from — instead of settling into the
/// cache-hit path `fah-dns`'s bench already measures.
const FORWARD_DOMAINS: usize = 16_384;

/// Queries in flight per wave of the throughput bench: a third blocked, a
/// third cache-hit, a third forwarded. Several times the RB5009's four cores
/// is what a sustained-load measurement needs to keep every core busy without
/// turning into a measurement of queue depth.
const WAVE: usize = 192;

/// Never-yet-seen names for the throughput bench's forwarded third, consumed
/// through a cursor shared across waves. Bigger than the cache, so by the
/// time the pool wraps a reused name has long been evicted and still misses.
const FRESH_POOL: usize = 16_384;

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

/// Writes the 1M-domain hosts blocklist where a previous run's refresh would
/// have cached it (`<data>/lists/<id>.raw`) and returns the config declaring
/// that list — the setup the startup and memory benches share.
fn cached_1m_blocklist(data_dir: &std::path::Path) -> RulesConfig {
    let lists_dir = data_dir.join("lists");
    std::fs::create_dir_all(&lists_dir).unwrap();
    std::fs::write(
        lists_dir.join("blocklist-1m.raw"),
        hosts_blocklist(BLOCKLIST_SIZE),
    )
    .unwrap();
    RulesConfig {
        refresh_hours_default: 24,
        lists: vec![RuleListConfig {
            id: "blocklist-1m".to_string(),
            url: "https://example.invalid/blocklist.txt".to_string(),
            enabled: true,
            refresh_hours: None,
        }],
    }
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
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
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
        Ok(ForwardOutcome::new(response, 0))
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
/// engine's share: decode, verdict, cache miss, cache insert — with the
/// eviction every insert into a full cache performs — and encode.
fn bench_forwarded_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let forwarder = InstantForwarder::new();

    let pipeline = rt.block_on(async {
        let manager =
            manager_with_user_rules(data_dir.path(), "||ads.example.com^\n".to_string()).await;
        build_pipeline(manager, forwarder.clone())
    });

    // FORWARD_DOMAINS distinct names, visited strictly in order: combined
    // with oldest-first eviction, cycling a set larger than the cache
    // guarantees every lookup misses (the entry was evicted before the cycle
    // came back around), so the timed loop cannot decay into cache hits.
    let queries: Vec<Vec<u8>> = (0..FORWARD_DOMAINS)
        .map(|i| encode_query(&format!("host{i}.forwarded.example.net")))
        .collect();

    let before = forwarder.calls.load(Ordering::Relaxed);
    // One cursor, two jobs: workload index and premise accounting. It must
    // live outside the closure and never rewind — criterion re-invokes the
    // closure per measurement phase, and a cursor that restarts at zero walks
    // straight back into the entries the previous phase just cached, turning
    // a slice of every sample into cache hits.
    let cursor = AtomicU64::new(0);
    let mut group = c.benchmark_group("full_pipeline");
    group.bench_function("forwarded_query_overhead", |b| {
        b.iter(|| {
            // One relaxed add per ~5 µs iteration: cost in the noise.
            let n = cursor.fetch_add(1, Ordering::Relaxed);
            let raw = &queries[n as usize % queries.len()];
            rt.block_on(pipeline.handle(black_box(raw), CLIENT, Transport::Udp))
        });
    });
    group.finish();

    // Criterion never invokes the routine when a `--bench <filter>` argument
    // excludes it, so only assert when the loop ran. And when it ran, demand
    // more than "the forwarder was reached": warmup alone satisfies that even
    // if every measured sample was a cache hit. The premise worth asserting
    // is that essentially every iteration forwarded.
    let iterations = cursor.load(Ordering::Relaxed);
    let forwards = forwarder.calls.load(Ordering::Relaxed) - before;
    if iterations > 0 {
        assert!(
            forwards >= iterations * 9 / 10,
            "only {forwards} of {iterations} timed queries reached the forwarder — \
             the loop decayed into cache hits and measured the wrong path"
        );
    }
}

/// PERFORMANCE.md: "Sustained throughput >= 10 000 QPS". A realistic mix —
/// a third blocked, a third repeat (cache-hit) traffic, a third fresh names
/// that must be forwarded — driven `WAVE` queries at a time, because the
/// budget is about the assembled system under load, not one query at a time.
/// Reported as elements/second by criterion's throughput support.
fn bench_throughput(c: &mut Criterion) {
    // Four workers to match the four RB5009 cores the budget is stated for.
    // PERFORMANCE.md §Measuring reliably restricts this bench to four cores
    // externally; shaping the runtime the same way keeps even an unpinned
    // run comparable instead of dev-box-shaped.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
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

    let mut rng = Lcg(0x2545_f491_4f6c_dd1d);
    let blocked: Arc<Vec<Vec<u8>>> = Arc::new(
        (0..WAVE / 3)
            .map(|_| encode_query(&format!("t{}.ads.example.com", rng.next() % 1024)))
            .collect(),
    );
    let cached: Arc<Vec<u8>> = Arc::new(encode_query("cached.example.org"));
    // The forwarded third rotates through this pool via a cursor shared
    // across waves. It must not be a fixed per-wave set: anything fixed is in
    // the cache from the second wave on and stops forwarding, quietly turning
    // the mix into two-thirds cache hits.
    let fresh: Arc<Vec<Vec<u8>>> = Arc::new(
        (0..FRESH_POOL)
            .map(|i| encode_query(&format!("fresh{i}.example.net")))
            .collect(),
    );
    let fresh_cursor = Arc::new(AtomicU64::new(0));

    // Warm the cache-hit third so the steady-state mix is what gets measured.
    rt.block_on(pipeline.handle(&cached, CLIENT, Transport::Udp));

    let before = forwarder.calls.load(Ordering::Relaxed);
    let total = AtomicU64::new(0);

    let mut group = c.benchmark_group("full_pipeline");
    group.throughput(Throughput::Elements(WAVE as u64));
    group.bench_function("sustained_throughput", |b| {
        b.iter_custom(|iters| {
            total.fetch_add(iters * WAVE as u64, Ordering::Relaxed);
            let pipeline = Arc::clone(&pipeline);
            let blocked = Arc::clone(&blocked);
            let cached = Arc::clone(&cached);
            let fresh = Arc::clone(&fresh);
            let fresh_cursor = Arc::clone(&fresh_cursor);
            rt.block_on(async move {
                let started = Instant::now();
                for _ in 0..iters {
                    let mut handles = Vec::with_capacity(WAVE);
                    for index in 0..WAVE {
                        let pipeline = Arc::clone(&pipeline);
                        let blocked = Arc::clone(&blocked);
                        let cached = Arc::clone(&cached);
                        let fresh = Arc::clone(&fresh);
                        let fresh_cursor = Arc::clone(&fresh_cursor);
                        handles.push(tokio::spawn(async move {
                            let raw: &[u8] = match index % 3 {
                                0 => &blocked[index / 3],
                                1 => &cached,
                                _ => {
                                    let n = fresh_cursor.fetch_add(1, Ordering::Relaxed);
                                    &fresh[n as usize % FRESH_POOL]
                                }
                            };
                            pipeline.handle(raw, CLIENT, Transport::Udp).await
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

    // The premise, asserted (a filtered-out run leaves `total` at zero): the
    // fresh third must actually forward, and not much more than the fresh
    // third may — blocked queries stop at the verdict and the hot name stays
    // hot, except for the rare eviction under churn.
    let total = total.load(Ordering::Relaxed);
    let forwards = forwarder.calls.load(Ordering::Relaxed) - before;
    if total > 0 {
        assert!(
            forwards >= total / 3 * 9 / 10,
            "only {forwards} of {total} queries forwarded — the fresh third \
             decayed into cache hits and the measured mix is not what this \
             bench claims"
        );
        assert!(
            forwards <= total / 3 + total / 10,
            "{forwards} of {total} queries forwarded — more than the fresh \
             third; the cache-hit or blocked thirds are leaking upstream"
        );
    }
}

/// PERFORMANCE.md: "Startup to serving (cached lists, 1M-domain parse) 1-3 s".
/// Measures exactly what the binary does before the listeners bind:
/// `ListManager::new` then `boot()` — read `/data`, parse, compile, swap in.
/// No network (RULE_ENGINE.md: boot must not wait on it).
fn bench_startup(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config = cached_1m_blocklist(data_dir.path());

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

/// Splits `startup_from_cached_lists` into its three phases, because "startup
/// is 3 s" is not actionable — optimising the wrong phase is wasted work.
///
/// Boot does: read each `/data/lists/<id>.raw` off disk, parse it into a
/// `ParsedRuleList`, then feed those into a `MatcherBuilder` and `build()`.
/// The three benches below time exactly those steps on the same 1M-domain
/// input the startup bench uses, so their sum should account for the whole.
///
/// Disk read is measured against the OS page cache, warm — which is what a
/// restart on a running router actually sees.
fn bench_startup_phases(c: &mut Criterion) {
    let data_dir = tempfile::tempdir().unwrap();
    let path = data_dir.path().join("blocklist-1m.raw");
    std::fs::write(&path, hosts_blocklist(BLOCKLIST_SIZE)).unwrap();

    // Warm the page cache so the read bench measures the read, not the SSD.
    let text = std::fs::read_to_string(&path).unwrap();
    let parsed = fah_rules::parse_rule_list(&text);
    println!(
        "\n[phases] input: {} bytes, {} active rules, {} inactive\n",
        text.len(),
        parsed.active_count(),
        parsed.inactive_count()
    );

    let mut group = c.benchmark_group("startup_phases");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    group.bench_function("1_read_from_data", |b| {
        b.iter(|| black_box(std::fs::read_to_string(&path).unwrap().len()));
    });

    group.bench_function("2_parse_rule_list", |b| {
        b.iter(|| black_box(fah_rules::parse_rule_list(black_box(&text)).active_count()));
    });

    // `with_capacity` is what the compile path uses (lifecycle sizes the
    // builder's transient dedup index from a one-pass ceiling on the rule
    // count), so the bench must use it too or it measures a rehash storm the
    // product never pays.
    group.bench_function("3_build_matcher", |b| {
        b.iter(|| {
            let mut builder = fah_rules::MatcherBuilder::with_capacity(parsed.rules.len());
            builder.add_parsed_list(std::sync::Arc::from("blocklist-1m"), black_box(&parsed));
            black_box(builder.build().len())
        });
    });

    // The same corpus loaded twice — the AdGuard/HaGeZi overlap in its
    // extreme form. Dedup must make the second copy cost build *time* and no
    // resident bytes at all (p1.5-05).
    group.bench_function("4_build_matcher_two_overlapping_lists", |b| {
        b.iter(|| {
            let mut builder = fah_rules::MatcherBuilder::with_capacity(parsed.rules.len() * 2);
            builder.add_parsed_list(std::sync::Arc::from("list-a"), black_box(&parsed));
            builder.add_parsed_list(std::sync::Arc::from("list-b"), black_box(&parsed));
            black_box(builder.build().len())
        });
    });

    let mut single = fah_rules::MatcherBuilder::with_capacity(parsed.rules.len());
    single.add_parsed_list(std::sync::Arc::from("list-a"), &parsed);
    let single = single.build();
    let mut doubled = fah_rules::MatcherBuilder::with_capacity(parsed.rules.len() * 2);
    doubled.add_parsed_list(std::sync::Arc::from("list-a"), &parsed);
    doubled.add_parsed_list(std::sync::Arc::from("list-b"), &parsed);
    let doubled = doubled.build();
    println!(
        "\n[p1.5-05] dedup on fully overlapping lists: one list {} rules / {:.1} MiB, \
         the same list twice {} rules / {:.1} MiB, {} duplicates removed\n",
        single.len(),
        single.heap_bytes() as f64 / (1024.0 * 1024.0),
        doubled.len(),
        doubled.heap_bytes() as f64 / (1024.0 * 1024.0),
        doubled.duplicates_removed(),
    );

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
    let config = cached_1m_blocklist(data_dir.path());

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

    let rss_report = match fah_common::process::resident_bytes() {
        Some(rss) => format!(
            "{:.1} MiB (budget: <= 128 MB)",
            rss as f64 / (1024.0 * 1024.0)
        ),
        None => "unavailable on this platform (/proc/self/status is Linux-only; \
                 the RB5009 figure is measured in p1-11)"
            .to_string(),
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
    bench_startup_phases,
    report_memory
);
criterion_main!(benches);
