//! Fixture-driven parser tests (p1-01-rule-parsers.md acceptance criteria):
//! each real-world-shaped fixture must parse to the expected
//! active/inactive/parse_errors counts.

use fah_model::{QueryType, Verdict};
use fah_rules::{parse_rule_list, MatcherBuilder, RuleFormat, RuleKind};

#[test]
fn hosts_fixture_parses_expected_counts() {
    let result = parse_rule_list(include_str!("fixtures/hosts.txt"));
    assert_eq!(result.format, RuleFormat::Hosts);
    assert_eq!(result.active_count(), 5);
    assert_eq!(result.inactive_count(), 0);
    assert_eq!(result.parse_errors, 2);
}

#[test]
fn plain_domain_list_fixture_parses_expected_counts() {
    let result = parse_rule_list(include_str!("fixtures/plain_domain_list.txt"));
    assert_eq!(result.format, RuleFormat::PlainDomainList);
    assert_eq!(result.active_count(), 3);
    assert_eq!(result.inactive_count(), 0);
    assert_eq!(result.parse_errors, 2);
}

#[test]
fn easylist_fixture_parses_expected_counts() {
    let result = parse_rule_list(include_str!("fixtures/easylist.txt"));
    assert_eq!(result.format, RuleFormat::Adblock);
    assert_eq!(result.active_count(), 1);
    // Four of the six formerly-inactive rules are URL-tier rules since p2-03;
    // the two cosmetic ones stay inactive until Phase 4.
    assert_eq!(result.url_count(), 4);
    assert_eq!(result.inactive_count(), 2);
    assert_eq!(result.parse_errors, 2);
}

#[test]
fn adguard_dns_fixture_parses_expected_counts() {
    let result = parse_rule_list(include_str!("fixtures/adguard_dns.txt"));
    assert_eq!(result.format, RuleFormat::Adblock);
    assert_eq!(result.active_count(), 4);
    assert_eq!(result.inactive_count(), 2);
    assert_eq!(result.parse_errors, 0);
}

/// The p2-00 regression, kept on its own so a failure names the defect.
///
/// `strip_prefix('$').unwrap_or("")` used to discard whatever followed `^`,
/// collapsing a path-qualified rule into a bare domain rule. Every domain below
/// carries a real EasyPrivacy/EasyList rule about one *path* on it, and every
/// one of them used to compile into a subdomain-inclusive DNS block on the
/// whole domain: `||paypal.com^*/pixel.gif$third-party` blocked all of PayPal.
#[test]
fn path_qualified_rules_never_block_the_whole_domain() {
    // Verbatim from EasyList / EasyPrivacy.
    const RULES: &[(&str, &str)] = &[
        ("||paypal.com^*/pixel.gif$third-party", "paypal.com"),
        ("||googleapis.com^*/gen_204?", "googleapis.com"),
        ("||amazonaws.com^*/prod_analytics", "amazonaws.com"),
        ("||s3.amazonaws.com^*/secure.js", "s3.amazonaws.com"),
        ("||ebay.com^*/madrona_loadscripts.js", "ebay.com"),
        ("||x.com^*/log.json", "x.com"),
        ("||cloudfront.net^*.bmp?", "cloudfront.net"),
        ("||akamai.net^*/sitetracking/", "akamai.net"),
        (
            "||bing.com^*/glinkping.aspx$ping,xmlhttprequest",
            "bing.com",
        ),
        ("||opera.com^*admaven$document", "opera.com"),
    ];

    for (rule, domain) in RULES {
        let parsed = parse_rule_list(&format!("! Title: t\n||seed.invalid^\n{rule}\n"));
        let mut builder = MatcherBuilder::new();
        builder.add_parsed_list("test", &parsed);
        let matcher = builder.build();

        assert_eq!(
            matcher.verdict(domain, &QueryType::A),
            Verdict::Pass,
            "{rule} must not produce a DNS verdict for {domain} — it addresses a \
             path on that domain, not the domain itself"
        );
        assert_eq!(
            matcher.verdict(&format!("www.{domain}"), &QueryType::A),
            Verdict::Pass,
            "{rule} must not reach subdomains of {domain} either"
        );
        // It is not dropped — since p2-03 it compiles into the URL tier, which
        // is what "the HTTP phase activates it" was always supposed to mean.
        // Located by kind rather than by index: the seed rule happens to come
        // first today, but a parser that ever emits an extra entry (metadata,
        // say) would silently move the rule under test out from under a
        // positional check.
        let url: Vec<_> = parsed
            .rules
            .iter()
            .filter(|parsed| parsed.is_url())
            .collect();
        assert_eq!(
            url.len(),
            1,
            "{rule} should be the only URL rule in the fixture"
        );
        assert!(
            matches!(url[0].kind, RuleKind::Url(_)),
            "{rule} belongs in the URL tier for p2-03"
        );
        assert_eq!(
            matcher.url_len(),
            1,
            "{rule} must reach the compiled URL tier, not stop at the parser"
        );
    }
}

/// The other half of the pair: the *same* domains, addressed as domains, must
/// still compile to DNS rules. Together with
/// `path_qualified_rules_never_block_the_whole_domain` this pins the boundary
/// from both sides — `||domain^` is a DNS rule, `||domain^*/path` is a URL rule
/// — so neither a regression nor an over-correction can slip through.
#[test]
fn domain_anchored_rules_stay_dns_rules() {
    for domain in ["paypal.com", "googleapis.com", "amazonaws.com", "x.com"] {
        let parsed = parse_rule_list(&format!("! Title: t\n||{domain}^\n"));
        let mut builder = MatcherBuilder::new();
        builder.add_parsed_list("test", &parsed);
        let matcher = builder.build();

        assert!(
            matches!(matcher.verdict(domain, &QueryType::A), Verdict::Block(_)),
            "||{domain}^ addresses the domain itself and must block it"
        );
        assert!(
            matches!(
                matcher.verdict(&format!("www.{domain}"), &QueryType::A),
                Verdict::Block(_)
            ),
            "||{domain}^ carries subdomain semantics"
        );
        assert_eq!(parsed.active_count(), 1);
    }
}

/// `|` after the separator anchors the end of the address, so the rule
/// addresses that host and not its subdomains. The exceptions in AdGuard's DNS
/// filter use this form; widening them to subdomains was the milder half of the
/// same defect.
#[test]
fn end_anchored_rules_match_the_host_but_not_its_subdomains() {
    let parsed = parse_rule_list("! Title: t\n||clarity.ms^|\n@@||link.nzz.ch^|\n");
    let mut builder = MatcherBuilder::new();
    builder.add_parsed_list("test", &parsed);
    let matcher = builder.build();

    assert!(matches!(
        matcher.verdict("clarity.ms", &QueryType::A),
        Verdict::Block(_)
    ));
    assert_eq!(
        matcher.verdict("sub.clarity.ms", &QueryType::A),
        Verdict::Pass,
        "`||d^|` anchors the address end — it must not carry subdomain semantics"
    );
    assert!(matches!(
        matcher.verdict("link.nzz.ch", &QueryType::A),
        Verdict::Allow(_)
    ));
    assert_eq!(
        matcher.verdict("sub.link.nzz.ch", &QueryType::A),
        Verdict::Pass,
        "an end-anchored exception must not widen to subdomains either"
    );
}

/// Real EasyList opens with URL-substring patterns, not with `||` rules. First
/// content line: `&rb=&uuid=$third-party`. Detecting from that one line sent
/// the entire list to the bare-domain parser.
#[test]
fn real_easylist_head_is_detected_as_adblock() {
    let result = parse_rule_list(include_str!("fixtures/easylist_head.txt"));
    assert_eq!(
        result.format,
        RuleFormat::Adblock,
        "EasyList must not be mistaken for a plain domain list"
    );
    assert_eq!(
        result.parse_errors, 0,
        "every line in the excerpt is valid adblock syntax"
    );
    assert_eq!(
        result.active_count(),
        0,
        "the excerpt is all URL patterns — none of them is a DNS rule"
    );
    // Since p2-03 all 30 compile into the URL tier. Before it, every one of
    // them was an `Inactive(UrlPattern)` discriminant with its text discarded,
    // which is what made "EasyList is supported" untrue in practice.
    assert_eq!(result.url_count(), 30);
    assert_eq!(result.inactive_count(), 0);
}

#[test]
fn no_fixture_line_ever_causes_a_rejected_list() {
    for fixture in [
        include_str!("fixtures/hosts.txt"),
        include_str!("fixtures/plain_domain_list.txt"),
        include_str!("fixtures/easylist.txt"),
        include_str!("fixtures/adguard_dns.txt"),
        include_str!("fixtures/easylist_head.txt"),
    ] {
        // parse_rule_list has no Result/panic path — this just documents the
        // "never reject a list" guarantee (RULE_ENGINE.md: Supported formats).
        let _ = parse_rule_list(fixture);
    }
}
