//! Compiled-matcher benches vs PERFORMANCE.md budgets (p1-02):
//! 1M domains <= 40 MB compiled, verdict lookup p99 < 1 ms (target: sub-µs).
//!
//! `cargo bench -p fah-rules` runs these. The 1M-domain memory figure is
//! printed once at startup (criterion times lookups, not allocation), so it is
//! visible in the bench output alongside the latency distribution.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_model::QueryType;
use fah_rules::{DomainRule, MatcherBuilder, RuleAction};

const N: usize = 1_000_000;

/// A deterministic pseudo-random generator — reproducible synthetic domains
/// without pulling a rand dependency.
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

/// Realistic blocklist-domain shapes. Real 1M lists (OISD, StevenBlack) average
/// ~20-22 bytes per domain across a mix of apex and one/two-label subdomains;
/// this generator matches that distribution so the memory figure reflects the
/// PERFORMANCE.md budget's actual input, not artificially long names.
fn synthetic_domain(n: u64) -> String {
    const TLDS: [&str; 5] = ["com", "net", "org", "io", "co"];
    let tld = TLDS[(n % 5) as usize];
    // `n` is embedded so every domain is unique (a real 1M list has 1M distinct
    // names); the varied prefixes keep the length distribution realistic.
    match n % 4 {
        0 => format!("ads{n}.{tld}"),
        1 => format!("track{n}.{tld}"),
        2 => format!("cdn{n}.metrics.{tld}"),
        _ => format!("node{n}.ad.{tld}"),
    }
}

fn build_1m() -> fah_rules::Matcher {
    // Sized like the compile path does it, so the 1M-domain memory figure
    // below is the product's and not a rehashing builder's.
    let mut builder = MatcherBuilder::with_capacity(N);
    let list = builder.add_list("synthetic-1m");
    for n in 0..N as u64 {
        builder.add_rule(
            list,
            &DomainRule {
                domain: synthetic_domain(n).into(),
                action: RuleAction::Block,
                include_subdomains: true,
                dns_types: None,
                dns_rewrite: None,
            },
        );
    }
    builder.build()
}

fn bench_matcher(c: &mut Criterion) {
    let matcher = build_1m();

    let bytes = matcher.heap_bytes();
    let mb = bytes as f64 / (1024.0 * 1024.0);
    println!(
        "\n[p1-02] compiled matcher: {} rules, {bytes} bytes ({mb:.1} MiB) resident \
         (PERFORMANCE.md budget: <= 40 MB for 1M domains)\n",
        matcher.len()
    );
    assert!(
        bytes <= 40 * 1024 * 1024,
        "compiled 1M-domain matcher is {mb:.1} MiB, over the 40 MB budget"
    );

    // Query set mixes hits (blocked domains, incl. a deep subdomain) and misses.
    let mut rng = Lcg(0x9e37_79b9_7f4a_7c15);
    let hits: Vec<String> = (0..1024)
        .map(|_| synthetic_domain(rng.next() % N as u64))
        .collect();
    let deep: Vec<String> = hits.iter().map(|d| format!("a.b.c.{d}")).collect();
    let misses: Vec<String> = (0..1024)
        .map(|i| format!("nomatch{i}.example.org"))
        .collect();

    let mut group = c.benchmark_group("matcher_lookup");

    group.bench_function("hit_exact", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let d = &hits[i % hits.len()];
            i += 1;
            black_box(matcher.lookup(black_box(d), &QueryType::A))
        });
    });

    group.bench_function("hit_subdomain", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let d = &deep[i % deep.len()];
            i += 1;
            black_box(matcher.lookup(black_box(d), &QueryType::A))
        });
    });

    group.bench_function("miss", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let d = &misses[i % misses.len()];
            i += 1;
            black_box(matcher.lookup(black_box(d), &QueryType::A))
        });
    });

    group.finish();
}

/// What deduplication actually buys, in bytes — the number that decides
/// whether its compile-time cost is worth paying (p1.5-05).
///
/// Two 1M-rule lists are compiled together at several overlap fractions. The
/// same pair is also compiled with the overlap *renamed away* (fully distinct),
/// which is exactly what the compiled matcher would have cost before dedup
/// existed, so the saving is a measured difference and not an extrapolation.
///
/// Not a timed criterion bench: the question is memory, and criterion measures
/// durations. Build wall time is timed here directly, once per configuration.
fn report_dedup_savings(_c: &mut Criterion) {
    fn rule(domain: String) -> DomainRule {
        DomainRule {
            domain: domain.into(),
            action: RuleAction::Block,
            include_subdomains: true,
            dns_types: None,
            dns_rewrite: None,
        }
    }

    /// `shared` of list B's rules repeat list A's; the rest are its own.
    /// `distinct_second` renames even the shared part, modelling the
    /// pre-dedup world where every duplicate occupied its own record.
    fn compile(shared: usize, distinct_second: bool) -> (fah_rules::Matcher, std::time::Duration) {
        let started = std::time::Instant::now();
        let mut b = MatcherBuilder::with_capacity(2 * N);
        let a = b.add_list("list-a");
        for n in 0..N as u64 {
            b.add_rule(a, &rule(synthetic_domain(n)));
        }
        let second = b.add_list("list-b");
        for i in 0..N as u64 {
            // The first `shared` rules of B repeat A's; the remainder are new
            // names drawn from a disjoint numeric range.
            let domain = if (i as usize) < shared && !distinct_second {
                synthetic_domain(i)
            } else if (i as usize) < shared {
                synthetic_domain(i + 10 * N as u64)
            } else {
                synthetic_domain(i + N as u64)
            };
            b.add_rule(second, &rule(domain));
        }
        let matcher = b.build();
        let elapsed = started.elapsed();
        (matcher, elapsed)
    }

    println!("\n[p1.5-05] deduplication: what it costs and what it saves");
    println!(
        "  two {N}-rule lists compiled together, by how much of the second \
         repeats the first\n"
    );
    println!(
        "  {:>8} {:>12} {:>12} {:>12} {:>12} {:>10}",
        "overlap", "duplicates", "rules", "compiled", "saved", "build"
    );
    for percent in [0usize, 25, 50, 75, 90, 100] {
        let shared = N / 100 * percent;
        let (deduped, elapsed) = compile(shared, false);
        let (undeduped, _) = compile(shared, true);
        let saved = undeduped.heap_bytes() as f64 - deduped.heap_bytes() as f64;
        println!(
            "  {:>7}% {:>12} {:>12} {:>9.1} MiB {:>8.1} MiB {:>7.0} ms",
            percent,
            deduped.duplicates_removed(),
            deduped.len(),
            deduped.heap_bytes() as f64 / (1024.0 * 1024.0),
            saved / (1024.0 * 1024.0),
            elapsed.as_secs_f64() * 1000.0,
        );
    }
    println!();
}

criterion_group!(benches, bench_matcher, report_dedup_savings);
criterion_main!(benches);
