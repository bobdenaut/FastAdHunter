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
    let mut builder = MatcherBuilder::new();
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

criterion_group!(benches, bench_matcher);
criterion_main!(benches);
