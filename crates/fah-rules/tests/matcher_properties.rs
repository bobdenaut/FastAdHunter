//! Property test (p1-02): a query's verdict *class* (allow/block/pass) is
//! invariant under permutation of rule insertion order — RULE_ENGINE.md says
//! list order and rule order are not significant to the verdict, only the
//! allow > block precedence and label specificity are. Uses a seeded LCG shuffle
//! so it needs no `rand`/`proptest` dependency.

use fah_model::{QueryType, Verdict};
use fah_rules::{DomainRule, MatcherBuilder, RuleAction};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn rule(domain: &str, action: RuleAction, include_subdomains: bool) -> DomainRule {
    DomainRule {
        domain: domain.into(),
        action,
        include_subdomains,
        dns_types: None,
        dns_rewrite: None,
        client: None,
    }
}

/// A pool of overlapping rules (same domains as both block and allow, nested
/// subdomains, exact-only rules) so permutations genuinely exercise precedence.
/// Note an allow on a parent covers every subdomain (allow > block), so the
/// blocks below live under domains with no ancestor allow.
fn rule_pool() -> Vec<DomainRule> {
    use RuleAction::{Allow, Block};
    vec![
        rule("example.com", Block, true),
        rule("ads.example.com", Block, true),
        rule("tracker.example.com", Block, true),
        rule("safe.example.com", Allow, true), // overrides the example.com block
        rule("exact.example.net", Block, false), // exact only, no subdomains
        rule("foo.example.org", Block, true),  // same domain as the allow below
        rule("foo.example.org", Allow, true),  // allow > block on the same domain
        rule("a.b.c.example.io", Block, true), // deeper block ...
        rule("b.c.example.io", Allow, true),   // ... overridden by a parent allow
    ]
}

fn queries() -> Vec<&'static str> {
    vec![
        "example.com",
        "ads.example.com",
        "sub.ads.example.com",
        "tracker.example.com",
        "safe.example.com",
        "x.safe.example.com",
        "exact.example.net",
        "sub.exact.example.net",
        "foo.example.org",
        "a.b.c.example.io",
        "unrelated.test",
    ]
}

fn verdict_class(v: &Verdict) -> u8 {
    match v {
        Verdict::Allow(_) => 0,
        Verdict::Block(_) => 1,
        Verdict::Pass => 2,
    }
}

fn build(order: &[usize], pool: &[DomainRule]) -> fah_rules::Matcher {
    let mut b = MatcherBuilder::new();
    // Spread rules across two lists to prove list identity is irrelevant too.
    let list_a = b.add_list("list-a");
    let list_b = b.add_list("list-b");
    for (pos, &idx) in order.iter().enumerate() {
        let list = if pos % 2 == 0 { list_a } else { list_b };
        b.add_rule(list, &pool[idx]);
    }
    b.build()
}

#[test]
fn verdict_class_is_invariant_under_rule_order_permutation() {
    let pool = rule_pool();
    let identity: Vec<usize> = (0..pool.len()).collect();
    let baseline = build(&identity, &pool);

    // Reference verdict class per query from the identity ordering.
    let expected: Vec<u8> = queries()
        .iter()
        .map(|q| verdict_class(&baseline.verdict(q, &QueryType::A)))
        .collect();

    let mut rng = Lcg(0xdead_beef_cafe_f00d);
    for _ in 0..200 {
        // Fisher–Yates shuffle of the insertion order.
        let mut order = identity.clone();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        let permuted = build(&order, &pool);

        for (q, &want) in queries().iter().zip(&expected) {
            let got = verdict_class(&permuted.verdict(q, &QueryType::A));
            assert_eq!(
                got, want,
                "verdict class for {q} changed under permutation {order:?}"
            );
        }
    }
}

#[test]
fn precedence_and_specificity_are_as_documented() {
    let pool = rule_pool();
    let identity: Vec<usize> = (0..pool.len()).collect();
    let m = build(&identity, &pool);

    // block with no overriding allow.
    assert!(matches!(
        m.verdict("example.com", &QueryType::A),
        Verdict::Block(_)
    ));
    assert!(matches!(
        m.verdict("ads.example.com", &QueryType::A),
        Verdict::Block(_)
    ));
    // subdomain inherits the parent block.
    assert!(matches!(
        m.verdict("sub.ads.example.com", &QueryType::A),
        Verdict::Block(_)
    ));
    // a more specific allow overrides the parent example.com block, and covers
    // its own subdomains.
    assert!(matches!(
        m.verdict("safe.example.com", &QueryType::A),
        Verdict::Allow(_)
    ));
    assert!(matches!(
        m.verdict("x.safe.example.com", &QueryType::A),
        Verdict::Allow(_)
    ));
    // allow > block on the same exact domain.
    assert!(matches!(
        m.verdict("foo.example.org", &QueryType::A),
        Verdict::Allow(_)
    ));
    // a parent allow overrides a deeper block.
    assert!(matches!(
        m.verdict("a.b.c.example.io", &QueryType::A),
        Verdict::Allow(_)
    ));
    // exact-only rule blocks the exact name but not its subdomain.
    assert!(matches!(
        m.verdict("exact.example.net", &QueryType::A),
        Verdict::Block(_)
    ));
    assert!(matches!(
        m.verdict("sub.exact.example.net", &QueryType::A),
        Verdict::Pass
    ));
}
