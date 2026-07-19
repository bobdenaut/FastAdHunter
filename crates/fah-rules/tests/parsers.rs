//! Fixture-driven parser tests (p1-01-rule-parsers.md acceptance criteria):
//! each real-world-shaped fixture must parse to the expected
//! active/inactive/parse_errors counts.

use fah_rules::{parse_rule_list, RuleFormat};

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
    assert_eq!(result.inactive_count(), 6);
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

#[test]
fn no_fixture_line_ever_causes_a_rejected_list() {
    for fixture in [
        include_str!("fixtures/hosts.txt"),
        include_str!("fixtures/plain_domain_list.txt"),
        include_str!("fixtures/easylist.txt"),
        include_str!("fixtures/adguard_dns.txt"),
    ] {
        // parse_rule_list has no Result/panic path — this just documents the
        // "never reject a list" guarantee (RULE_ENGINE.md: Supported formats).
        let _ = parse_rule_list(fixture);
    }
}
