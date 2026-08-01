//! p2-05's memory acceptance criterion, as a test rather than a claim.
//!
//! "Two policies sharing one list: the list is compiled once, and the absolute
//! heap stays within a few MiB of the single-ruleset figure rather than a
//! multiple of it."
//!
//! The risk this guards is specific. A policy is a subset of the configured
//! rule lists, so the obvious implementation is one compiled `Matcher` per
//! policy — and the deployed ruleset is 21.9 MiB against roughly 24 MiB of
//! headroom under PERFORMANCE.md's 128 MB budget, so the *second* policy would
//! overrun it on its own. These assertions fail if anyone ever reintroduces
//! that shape.

use fah_config::{RuleListConfig, RulesConfig};
use fah_model::{PolicyId, QueryType};
use fah_rules::{parse_rule_list, ClientContext, Matcher, MatcherBuilder, PolicySet};

/// Two lists with a deliberate overlap: `shared` appears in both, so a
/// per-policy compile would pay for it twice.
const SHARED: &str = "\
||ads.example.com^
||tracker.example.net^
||analytics.example.org^
";
const ADULT: &str = "\
||adult-one.example.com^
||adult-two.example.com^
";

fn policy_config(id: &str, lists: &[&str]) -> fah_config::PolicyConfig {
    fah_config::PolicyConfig {
        id: id.to_string(),
        name: None,
        lists: Some(lists.iter().map(|list| list.to_string()).collect()),
        blocking_mode: None,
        assignments: Vec::new(),
    }
}

fn rules_config(lists: &[&str]) -> RulesConfig {
    RulesConfig {
        refresh_hours_default: 24,
        lists: lists
            .iter()
            .map(|id| RuleListConfig {
                id: id.to_string(),
                url: format!("https://example.invalid/{id}"),
                enabled: true,
                refresh_hours: None,
            })
            .collect(),
    }
}

/// Compiles the union of `texts` under `policies`, exactly as the list
/// lifecycle does.
fn compile(texts: &[(&str, &str)], policies: &PolicySet) -> Matcher {
    let mut builder = MatcherBuilder::new();
    for (id, text) in texts {
        let parsed = parse_rule_list(text);
        builder.add_parsed_list_masked(*id, &parsed, policies.mask_for_list(id));
    }
    builder.build()
}

fn blocks(matcher: &Matcher, domain: &str, policy: PolicyId) -> bool {
    let ctx = ClientContext {
        policy,
        ..ClientContext::default()
    };
    matches!(
        matcher.lookup_in(domain, &QueryType::A, &ctx),
        fah_rules::MatchDecision::Block(_)
    )
}

#[test]
fn two_policies_over_overlapping_lists_compile_the_shared_list_once() {
    let config = rules_config(&["shared", "adult"]);
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy_config("kids", &["shared", "adult"]),
            policy_config("guest", &["shared"]),
        ],
    )
    .unwrap();
    // The config the policies name must be the config that exists, or the
    // masks below would be silently empty.
    for policy in ["kids", "guest"] {
        assert!(policies.id_of(policy).is_some(), "{policy} is defined");
    }
    assert_eq!(config.lists.len(), 2);

    let texts = [("shared", SHARED), ("adult", ADULT)];
    let matcher = compile(&texts, &policies);

    // One compile, one record per distinct rule — not one per (rule, policy).
    assert_eq!(
        matcher.len(),
        5,
        "the shared list must contribute its rules once, not once per policy"
    );

    let kids = policies.id_of("kids").unwrap();
    let guest = policies.id_of("guest").unwrap();

    // Both policies see the shared list.
    assert!(blocks(&matcher, "ads.example.com", kids));
    assert!(blocks(&matcher, "ads.example.com", guest));
    // Only the policy that enabled it sees the adult list.
    assert!(blocks(&matcher, "adult-one.example.com", kids));
    assert!(!blocks(&matcher, "adult-one.example.com", guest));
    // The default policy sees every enabled list, as it always has.
    assert!(blocks(&matcher, "adult-one.example.com", PolicyId::DEFAULT));
}

/// The heap half of the criterion: N policies must cost the union plus a mask
/// array, not N copies of the corpus.
#[test]
fn policies_cost_a_mask_array_and_not_a_second_ruleset() {
    let texts = [("shared", SHARED), ("adult", ADULT)];

    let single = compile(&texts, &PolicySet::single_default());

    // Every policy slot the model allows. They enable only `shared`, so the
    // masks genuinely differ from list to list — policies that all enabled
    // everything would filter nothing and the array would be dropped, which is
    // a different property (and the one the zero-config test covers).
    let ids: Vec<String> = (1..PolicyId::MAX)
        .map(|index| format!("p{index}"))
        .collect();
    let configs: Vec<fah_config::PolicyConfig> = ids
        .iter()
        .map(|id| policy_config(id, &["shared"]))
        .collect();
    let policies = PolicySet::from_config("UTC", &configs).unwrap();
    assert_eq!(policies.len(), PolicyId::MAX);

    let many = compile(&texts, &policies);

    assert_eq!(
        many.len(),
        single.len(),
        "policies must not multiply the compiled rule count"
    );

    // The mask array is two bytes per rule and nothing else, whether there are
    // two policies or sixteen.
    let overhead = many.heap_bytes() - single.heap_bytes();
    let masks = (many.len() + many.url_len()) * std::mem::size_of::<u16>();
    assert_eq!(
        overhead,
        masks,
        "{} policies added {overhead} bytes; only the {masks}-byte mask array is expected",
        policies.len()
    );
}

/// The zero-config case pays nothing at all: with no policies configured the
/// mask array is not allocated. Neither is it when policies exist but all
/// enable every list — the masks are then equal and filter nothing.
#[test]
fn a_ruleset_no_policy_narrows_carries_no_masks() {
    let texts = [("shared", SHARED), ("adult", ADULT)];
    let single = compile(&texts, &PolicySet::single_default());

    let mut plain = MatcherBuilder::new();
    for (id, text) in &texts {
        plain.add_parsed_list(*id, &parse_rule_list(text));
    }
    assert_eq!(single.heap_bytes(), plain.build().heap_bytes());

    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy_config("kids", &["shared", "adult"]),
            policy_config("guest", &["shared", "adult"]),
        ],
    )
    .unwrap();
    assert_eq!(
        compile(&texts, &policies).heap_bytes(),
        single.heap_bytes(),
        "policies that narrow nothing should cost nothing"
    );
}

/// Deduplication keeps the *first* list's attribution but must union policy
/// visibility — otherwise a rule present in two lists would vanish for every
/// policy that enabled only the second one.
#[test]
fn a_deduplicated_rule_stays_visible_to_every_list_that_supplied_it() {
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy_config("first-only", &["a"]),
            policy_config("second-only", &["b"]),
        ],
    )
    .unwrap();

    // The same rule from both lists; the second is dropped as a duplicate.
    let texts = [("a", "||ads.example.com^\n"), ("b", "||ads.example.com^\n")];
    let matcher = compile(&texts, &policies);
    assert_eq!(matcher.len(), 1);
    assert_eq!(matcher.duplicates_removed(), 1);

    let second = policies.id_of("second-only").unwrap();
    assert!(
        blocks(&matcher, "ads.example.com", second),
        "the surviving record is attributed to list a, but list b supplied it too"
    );
    assert!(blocks(
        &matcher,
        "ads.example.com",
        policies.id_of("first-only").unwrap()
    ));
}

/// The same union, for the URL tier.
#[test]
fn the_url_tier_unions_policy_visibility_across_lists_too() {
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy_config("first-only", &["a"]),
            policy_config("second-only", &["b"]),
        ],
    )
    .unwrap();

    let texts = [("a", "/ads/banner.gif\n"), ("b", "/ads/banner.gif\n")];
    let matcher = compile(&texts, &policies);
    assert_eq!(matcher.url_len(), 1);
    assert_eq!(matcher.url_duplicates_removed(), 1);

    let request = fah_model::HttpRequest {
        url: "http://example.com/ads/banner.gif",
        host: "example.com",
        method: "GET",
        resource_type: fah_model::ResourceType::Unknown,
        document_host: None,
    };
    for policy in ["first-only", "second-only"] {
        let ctx = ClientContext {
            policy: policies.id_of(policy).unwrap(),
            ..ClientContext::default()
        };
        assert!(
            matches!(
                matcher.lookup_http_in(&request, &ctx),
                fah_rules::MatchDecision::Block(_)
            ),
            "{policy} should see a rule its own list supplied"
        );
    }
}

/// A policy that excludes a list must not see that list's *exceptions* either.
/// Filtering after the walk instead of during it would let an excluded
/// `@@` rule suppress a block the policy still carries.
#[test]
fn an_excluded_lists_exception_cannot_suppress_a_block() {
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy_config("strict", &["blocks"]),
            policy_config("lenient", &["blocks", "allows"]),
        ],
    )
    .unwrap();

    let texts = [
        ("blocks", "||ads.example.com^\n"),
        ("allows", "@@||ads.example.com^\n"),
    ];
    let matcher = compile(&texts, &policies);

    assert!(
        blocks(
            &matcher,
            "ads.example.com",
            policies.id_of("strict").unwrap()
        ),
        "the exception lives in a list `strict` does not enable"
    );
    assert!(!blocks(
        &matcher,
        "ads.example.com",
        policies.id_of("lenient").unwrap()
    ));
}

/// `$client` is an inline per-client policy: same ruleset, same policy,
/// different answers depending on who asked. Activated in p2-05 — before it,
/// these rules were parsed, counted and discarded.
#[test]
fn a_client_scoped_rule_applies_only_to_the_clients_it_names() {
    let matcher = compile(
        &[(
            "user-rules",
            "||kids-blocked.example.com^$client=192.168.1.50\n",
        )],
        &PolicySet::single_default(),
    );
    assert_eq!(matcher.len(), 1);

    let asked_by = |ip: &str| {
        let ctx = ClientContext {
            ip: Some(ip.parse().unwrap()),
            ..ClientContext::default()
        };
        matches!(
            matcher.lookup_in("kids-blocked.example.com", &QueryType::A, &ctx),
            fah_rules::MatchDecision::Block(_)
        )
    };
    assert!(asked_by("192.168.1.50"));
    assert!(!asked_by("192.168.1.51"));
    // A lookup with no client behind it (the API's rules/test) is not the
    // named client either, so the rule does not fire.
    assert!(!matches!(
        matcher.lookup("kids-blocked.example.com", &QueryType::A),
        fah_rules::MatchDecision::Block(_)
    ));
}

/// Negation is the other half: everyone *except* the named client.
#[test]
fn a_negated_client_scope_excludes_only_that_client() {
    let matcher = compile(
        &[("user-rules", "||ads.example.com^$client=~192.168.1.50\n")],
        &PolicySet::single_default(),
    );
    let asked_by = |ip: &str| {
        let ctx = ClientContext {
            ip: Some(ip.parse().unwrap()),
            ..ClientContext::default()
        };
        matches!(
            matcher.lookup_in("ads.example.com", &QueryType::A, &ctx),
            fah_rules::MatchDecision::Block(_)
        )
    };
    assert!(!asked_by("192.168.1.50"));
    assert!(asked_by("192.168.1.51"));
}

/// Two rules that differ only in `$client` are two rules, not a duplicate —
/// and the reported rule text carries the scope back.
#[test]
fn client_scope_is_part_of_a_rules_identity() {
    let matcher = compile(
        &[(
            "user-rules",
            "||ads.example.com^$client=192.168.1.50\n||ads.example.com^$client=192.168.1.51\n",
        )],
        &PolicySet::single_default(),
    );
    assert_eq!(matcher.len(), 2);
    assert_eq!(matcher.duplicates_removed(), 0);

    let ctx = ClientContext {
        ip: Some("192.168.1.51".parse().unwrap()),
        ..ClientContext::default()
    };
    let fah_rules::MatchDecision::Block(rule) =
        matcher.lookup_in("ads.example.com", &QueryType::A, &ctx)
    else {
        panic!("expected a block");
    };
    assert_eq!(
        &*matcher.decisive_rule(rule).rule,
        "||ads.example.com^$client=192.168.1.51"
    );
}
