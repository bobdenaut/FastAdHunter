//! Plain domain list format parser (RULE_ENGINE.md: Supported formats —
//! plain domain list). One bare domain per line.

use crate::domain::normalize_domain;
use crate::format::RuleFormat;
use crate::rule::{DomainRule, ParsedRule, RuleAction, RuleKind};
use crate::rule_list::{ParseErrorLog, ParsedRuleList};

pub(crate) fn parse(text: &str) -> ParsedRuleList {
    let mut rules = Vec::new();
    let mut errors = ParseErrorLog::default();

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if line.split_whitespace().count() != 1 {
            errors.record(index);
            continue;
        }
        match normalize_domain(line) {
            Some(domain) => rules.push(ParsedRule {
                kind: RuleKind::Active(DomainRule {
                    domain,
                    action: RuleAction::Block,
                    include_subdomains: true,
                    dns_types: None,
                    dns_rewrite: None,
                }),
            }),
            None => errors.record(index),
        }
    }

    let (parse_errors, parse_error_lines) = errors.into_parts();
    ParsedRuleList {
        format: RuleFormat::PlainDomainList,
        rules,
        parse_errors,
        parse_error_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_domain_lines() {
        let result = parse("ads.example.com\ntracker.example.net\n");
        assert_eq!(result.active_count(), 2);
        assert_eq!(result.parse_errors, 0);
    }

    #[test]
    fn rejects_lines_with_whitespace() {
        let result = parse("bad domain.com\n");
        assert_eq!(result.parse_errors, 1);
    }

    #[test]
    fn rejects_wildcard_domain() {
        let result = parse("*.example.com\n");
        assert_eq!(result.parse_errors, 1);
    }
}
