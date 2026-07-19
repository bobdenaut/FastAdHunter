//! hosts-file format parser (RULE_ENGINE.md: Supported formats — hosts).
//!
//! `<ip> <hostname> [hostname...] [# comment]` per line, matching standard
//! hosts-file syntax (multiple aliases per line allowed).

use std::sync::Arc;

use crate::domain::normalize_domain;
use crate::format::{looks_like_ip, RuleFormat};
use crate::rule::{DomainRule, ParsedRule, RuleAction, RuleKind};
use crate::rule_list::{ParseErrorLog, ParsedRuleList};

/// Standard loopback/broadcast aliases that head almost every real hosts file
/// (StevenBlack et al.). These map a reserved address to the machine's own
/// names — not a block instruction. Emitting block rules for them would
/// synthesize `0.0.0.0` for `localhost` queries. Matched by hostname, so a
/// genuine `127.0.0.1 ads.example.com` block entry is untouched.
const LOOPBACK_ALIASES: [&str; 11] = [
    "localhost",
    "localhost.localdomain",
    "local",
    "broadcasthost",
    "ip6-localhost",
    "ip6-loopback",
    "ip6-localnet",
    "ip6-mcastprefix",
    "ip6-allnodes",
    "ip6-allrouters",
    "ip6-allhosts",
];

fn is_loopback_alias(host: &str) -> bool {
    LOOPBACK_ALIASES
        .iter()
        .any(|alias| host.eq_ignore_ascii_case(alias))
}

pub(crate) fn parse(text: &str) -> ParsedRuleList {
    let mut rules = Vec::new();
    let mut errors = ParseErrorLog::default();

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let Some(ip) = tokens.next() else {
            errors.record(index);
            continue;
        };
        if !looks_like_ip(ip) {
            errors.record(index);
            continue;
        }

        // `handled` covers both a real rule and a deliberately-skipped
        // loopback alias — either way the line was understood, so it must not
        // count as a parse error.
        let mut handled = false;
        for host in tokens.take_while(|token| !token.starts_with('#')) {
            if is_loopback_alias(host) {
                handled = true;
                continue;
            }
            let Some(domain) = normalize_domain(host) else {
                continue;
            };
            handled = true;
            rules.push(ParsedRule {
                raw: Arc::from(line),
                kind: RuleKind::Active(DomainRule {
                    domain,
                    action: RuleAction::Block,
                    include_subdomains: true,
                    dns_types: None,
                    dns_rewrite: None,
                }),
            });
        }
        if !handled {
            errors.record(index);
        }
    }

    let (parse_errors, parse_error_lines) = errors.into_parts();
    ParsedRuleList {
        format: RuleFormat::Hosts,
        rules,
        parse_errors,
        parse_error_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_hostname_line() {
        let result = parse("0.0.0.0 ads.example.com\n");
        assert_eq!(result.active_count(), 1);
        assert_eq!(result.parse_errors, 0);
    }

    #[test]
    fn parses_multiple_hostnames_per_line() {
        let result = parse("0.0.0.0 a.example.com b.example.com\n");
        assert_eq!(result.active_count(), 2);
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let result = parse("# header\n\n0.0.0.0 ads.example.com\n");
        assert_eq!(result.active_count(), 1);
        assert_eq!(result.parse_errors, 0);
    }

    #[test]
    fn counts_line_without_ip_as_error() {
        let result = parse("not-an-ip-line\n");
        assert_eq!(result.parse_errors, 1);
        assert_eq!(result.rules.len(), 0);
    }

    #[test]
    fn counts_ip_without_hostname_as_error() {
        let result = parse("0.0.0.0\n");
        assert_eq!(result.parse_errors, 1);
    }

    #[test]
    fn skips_standard_loopback_preamble_without_error() {
        let result = parse(
            "127.0.0.1 localhost\n\
             127.0.0.1 localhost.localdomain\n\
             255.255.255.255 broadcasthost\n\
             ::1 ip6-localhost ip6-loopback\n\
             0.0.0.0 ads.example.com\n",
        );
        // Only the real ad domain becomes a rule; the loopback preamble is
        // understood and skipped, not counted as an error.
        assert_eq!(result.active_count(), 1);
        assert_eq!(result.parse_errors, 0);
    }

    #[test]
    fn loopback_ip_pointed_at_real_domain_still_blocks() {
        let result = parse("127.0.0.1 ads.example.com\n");
        assert_eq!(result.active_count(), 1);
    }
}
