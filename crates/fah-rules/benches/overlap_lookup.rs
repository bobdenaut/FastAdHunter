//! Does deduplication make *lookup* faster, or only smaller? (p1.5-05)
//!
//! Dedup's headline is memory, but collapsing duplicates also shrinks the
//! open-addressing slot table and shortens its probe chains: before dedup, a
//! domain carried by two lists occupied two slots that hash to the same place,
//! so every query for it walked both. This bench isolates that.
//!
//! It is written against the pre-dedup `MatcherBuilder` API on purpose
//! (`new`/`add_list`/`add_rule`/`build` only), so the identical file compiles
//! in a checkout from before dedup existed and the two runs are a true A/B on
//! the same corpus and the same query set.
//!
//! `cargo bench -p fah-rules --bench overlap_lookup` runs it.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_model::QueryType;
use fah_rules::{DomainRule, MatcherBuilder, RuleAction};

/// Rules per list. Two lists, so 1M rules of input.
const PER_LIST: u64 = 500_000;
/// How much of the second list repeats the first — the AdGuard/HaGeZi shape.
const SHARED: u64 = PER_LIST / 2;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

fn synthetic_domain(n: u64) -> String {
    const TLDS: [&str; 5] = ["com", "net", "org", "io", "co"];
    let tld = TLDS[(n % 5) as usize];
    match n % 4 {
        0 => format!("ads{n}.{tld}"),
        1 => format!("track{n}.{tld}"),
        2 => format!("cdn{n}.metrics.{tld}"),
        _ => format!("node{n}.ad.{tld}"),
    }
}

fn block(domain: String) -> DomainRule {
    DomainRule {
        domain: domain.into(),
        action: RuleAction::Block,
        include_subdomains: true,
        dns_types: None,
        dns_rewrite: None,
    }
}

/// Two overlapping lists, compiled together exactly as `ListManager::compile`
/// merges them.
fn build_overlapping() -> fah_rules::Matcher {
    let mut b = MatcherBuilder::new();
    let a = b.add_list("list-a");
    for n in 0..PER_LIST {
        b.add_rule(a, &block(synthetic_domain(n)));
    }
    let second = b.add_list("list-b");
    for i in 0..PER_LIST {
        // The first half repeats list A; the rest are names only B carries.
        let n = if i < SHARED { i } else { i + PER_LIST };
        b.add_rule(second, &block(synthetic_domain(n)));
    }
    b.build()
}

fn bench_overlapping_lookup(c: &mut Criterion) {
    let matcher = build_overlapping();
    let bytes = matcher.heap_bytes();
    println!(
        "\n[p1.5-05] two overlapping lists ({PER_LIST} rules each, {SHARED} shared): \
         {} compiled rules, {bytes} bytes ({:.1} MiB)\n",
        matcher.len(),
        bytes as f64 / (1024.0 * 1024.0),
    );

    // Queries for domains carried by *both* lists — the ones that used to
    // occupy two colliding slots and now occupy one.
    let mut rng = Lcg(0x9e37_79b9_7f4a_7c15);
    let shared: Vec<String> = (0..1024)
        .map(|_| synthetic_domain(rng.next() % SHARED))
        .collect();
    // Domains only one list carries — unaffected by dedup except through the
    // table being smaller overall.
    let unique: Vec<String> = (0..1024)
        .map(|_| synthetic_domain(PER_LIST + rng.next() % (PER_LIST - SHARED)))
        .collect();
    let misses: Vec<String> = (0..1024)
        .map(|i| format!("nomatch{i}.example.org"))
        .collect();

    let mut group = c.benchmark_group("overlap_lookup");
    for (name, set) in [
        ("hit_shared_by_both_lists", &shared),
        ("hit_single_list", &unique),
        ("miss", &misses),
    ] {
        group.bench_function(name, |b| {
            let mut i = 0usize;
            b.iter(|| {
                let d = &set[i % set.len()];
                i += 1;
                black_box(matcher.lookup(black_box(d), &QueryType::A))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_overlapping_lookup);
criterion_main!(benches);
