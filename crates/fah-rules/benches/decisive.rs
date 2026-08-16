//! What a block costs *after* the walk — the p3-03 profile's third bucket.
//!
//! Three arms, because the bucket the RB5009 measured held all of them: the
//! lookup that produces a `RuleRef`, the `decisive_rule` that materializes its
//! text, and the `id_of` that names the deciding policy. Only the second
//! allocates, and only the second is priced per option shape.
//!
//! The corpus carries `$dnstype`, `$dnsrewrite` and `$client` rules, as the
//! public lists do. Without them the three side maps are empty and
//! `HashMap::get` answers from a length check without hashing, which prices the
//! option probes at zero.

use std::hint::black_box;
use std::net::IpAddr;

use criterion::{criterion_group, criterion_main, Criterion};
use fah_config::{AssignmentConfig, PolicyConfig};
use fah_model::QueryType;
use fah_rules::{
    parse_rule_list, DomainRule, MatchDecision, Matcher, MatcherBuilder, PolicySet, PolicyState,
    RuleAction, RuleRef,
};

const N: u64 = 1_000_000;
const CLIENT: &str = "192.168.1.50";

/// The option shapes, one apiece — the deployed lists' own proportion.
const OPTIONED: &str = "||opt-dnstype.example.com^$dnstype=A\n\
                        ||opt-rewrite.example.com^$dnsrewrite=1.2.3.4\n\
                        ||opt-client.example.com^$client=192.168.1.50";

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

/// Local copy of `benches/matcher.rs`'s generator, so that fixture stays
/// untouched while both benches walk the same domain shapes.
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

fn corpus(optioned: bool) -> Matcher {
    let mut builder = MatcherBuilder::with_capacity(N as usize);
    let list = builder.add_list("synthetic-1m");
    for n in 0..N {
        builder.add_rule(
            list,
            &DomainRule::plain(synthetic_domain(n).into(), RuleAction::Block, true),
        );
    }
    if optioned {
        builder.add_parsed_list("optioned", &parse_rule_list(OPTIONED));
    }
    builder.build()
}

/// A policy assigned to the benched client, so `id_of` answers `Some` and pays
/// the `Arc` clone the default never does.
fn assigned_to_client() -> PolicySet {
    let configs = vec![PolicyConfig {
        id: "kids".to_string(),
        name: None,
        lists: None,
        blocking_mode: None,
        assignments: vec![AssignmentConfig {
            client: CLIENT.to_string(),
            days: None,
            start: None,
            end: None,
        }],
    }];
    PolicySet::from_config("UTC", &configs).expect("policy set compiles")
}

fn bench_materialization(c: &mut Criterion) {
    let matcher = corpus(true);
    let bare = corpus(false);
    let client: IpAddr = CLIENT.parse().unwrap();

    let zero_config = PolicyState::default();
    let default_active = zero_config.current();
    let default_ctx = matcher.context_for(client, &default_active);

    let named = PolicyState::default();
    named.refresh(&assigned_to_client(), &[]);
    let named_active = named.current();
    let named_ctx = matcher.context_for(client, &named_active);

    let rule_for = |domain: &str| match matcher.lookup_in(domain, &QueryType::A, &default_ctx) {
        MatchDecision::Block(r) => r,
        other => panic!("expected {domain} to block, got {other:?}"),
    };

    let mut rng = Lcg(0x9e37_79b9_7f4a_7c15);
    let hits: Vec<String> = (0..1024)
        .map(|_| synthetic_domain(rng.next() % N))
        .collect();
    let plain: Vec<RuleRef> = hits.iter().map(|d| rule_for(d)).collect();
    let dnstype = rule_for("opt-dnstype.example.com");
    let rewrite = rule_for("opt-rewrite.example.com");
    let scoped = rule_for("opt-client.example.com");

    let mut group = c.benchmark_group("verdict_materialization");

    group.bench_function("lookup_only", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let d = &hits[i % hits.len()];
            i += 1;
            black_box(matcher.lookup_in(black_box(d), &QueryType::A, &default_ctx))
        });
    });

    group.bench_function("decisive_rule/plain", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let r = plain[i % plain.len()];
            i += 1;
            black_box(matcher.decisive_rule(black_box(r)))
        });
    });

    // Same work over empty side maps, where `HashMap::get` returns on a length
    // check: the delta against `plain` is what the three real probes cost.
    let bare_ctx = matcher.context_for(client, &default_active);
    let bare_plain: Vec<RuleRef> = hits
        .iter()
        .map(|d| match bare.lookup_in(d, &QueryType::A, &bare_ctx) {
            MatchDecision::Block(r) => r,
            other => panic!("expected {d} to block, got {other:?}"),
        })
        .collect();

    group.bench_function("decisive_rule/plain_empty_maps", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let r = bare_plain[i % bare_plain.len()];
            i += 1;
            black_box(bare.decisive_rule(black_box(r)))
        });
    });

    for (label, rule) in [
        ("decisive_rule/dnstype", dnstype),
        ("decisive_rule/rewrite", rewrite),
        ("decisive_rule/client", scoped),
    ] {
        group.bench_function(label, |b| {
            b.iter(|| black_box(matcher.decisive_rule(black_box(rule))));
        });
    }

    group.bench_function("id_of/default", |b| {
        b.iter(|| black_box(default_active.id_of(black_box(default_ctx.policy))));
    });

    group.bench_function("id_of/named", |b| {
        b.iter(|| black_box(named_active.id_of(black_box(named_ctx.policy))));
    });

    group.finish();
}

criterion_group!(benches, bench_materialization);
criterion_main!(benches);
