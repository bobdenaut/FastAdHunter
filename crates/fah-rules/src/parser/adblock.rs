//! EasyList / uBlock Origin / AdGuard format parser (RULE_ENGINE.md:
//! Supported formats — one syntax family, one parser). AdGuard's DNS options
//! (`$dnstype`, `$dnsrewrite`, `$client`) are recognized here, not treated
//! as a separate format.

use std::sync::Arc;

use crate::domain::normalize_domain;
use crate::format::RuleFormat;
use crate::rule::{DomainRule, InactiveReason, ParsedRule, RuleAction, RuleKind};
use crate::rule_list::{ParseErrorLog, ParsedRuleList};

const COSMETIC_MARKERS: [&str; 4] = ["##", "#@#", "#?#", "#$#"];

pub(crate) fn parse(text: &str) -> ParsedRuleList {
    let mut rules = Vec::new();
    let mut errors = ParseErrorLog::default();

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
            continue;
        }

        if COSMETIC_MARKERS.iter().any(|marker| line.contains(marker)) {
            rules.push(inactive(InactiveReason::Cosmetic));
            continue;
        }

        let exception = line.starts_with("@@");
        let body = if exception { &line[2..] } else { line };

        let Some(after_anchor) = body.strip_prefix("||") else {
            rules.push(inactive(InactiveReason::UrlPattern));
            continue;
        };

        let end = after_anchor
            .find(['^', '$', '/'])
            .unwrap_or(after_anchor.len());
        let domain_part = &after_anchor[..end];
        let terminator = after_anchor[end..].chars().next();

        if domain_part.is_empty() {
            errors.record(index);
            continue;
        }
        let Some(domain) = normalize_domain(domain_part) else {
            rules.push(inactive(InactiveReason::UrlPattern));
            continue;
        };
        if terminator == Some('/') {
            rules.push(inactive(InactiveReason::UrlPattern));
            continue;
        }

        // What follows the separator decides whether this is still a *domain*
        // rule. After `^` only three things keep it one: nothing, `$options`,
        // or `|` (the end-of-address anchor). Anything else is a path or
        // wildcard qualifier, and the rule addresses a URL, not a domain —
        // `InactiveReason::UrlPattern`, which is where the HTTP phase picks it
        // up. Silently dropping that qualifier and keeping the domain is what
        // turned `||paypal.com^*/pixel.gif` into a block on all of paypal.com.
        let (options_str, include_subdomains) = match terminator {
            Some('$') => (&after_anchor[end + 1..], true),
            Some('^') => {
                let rest = &after_anchor[end + 1..];
                // `|` immediately after the separator anchors the end of the
                // address: `||d^|` addresses `d` itself, so it must not carry
                // the subdomain semantics a bare `||d^` does.
                let (rest, include_subdomains) = match rest.strip_prefix('|') {
                    Some(tail) => (tail, false),
                    None => (rest, true),
                };
                if rest.is_empty() {
                    ("", include_subdomains)
                } else if let Some(options) = rest.strip_prefix('$') {
                    (options, include_subdomains)
                } else {
                    rules.push(inactive(InactiveReason::UrlPattern));
                    continue;
                }
            }
            _ => ("", true),
        };

        match parse_options(options_str) {
            Ok(options) => {
                if options.has_other {
                    rules.push(inactive(InactiveReason::HttpOption));
                } else if options.client {
                    rules.push(inactive(InactiveReason::ClientScoped));
                } else {
                    rules.push(ParsedRule {
                        kind: RuleKind::Active(DomainRule {
                            domain,
                            action: if exception {
                                RuleAction::Allow
                            } else {
                                RuleAction::Block
                            },
                            include_subdomains,
                            dns_types: options.dns_types,
                            dns_rewrite: options.dns_rewrite,
                        }),
                    });
                }
            }
            Err(()) => errors.record(index),
        }
    }

    let (parse_errors, parse_error_lines) = errors.into_parts();
    ParsedRuleList {
        format: RuleFormat::Adblock,
        rules,
        parse_errors,
        parse_error_lines,
    }
}

fn inactive(reason: InactiveReason) -> ParsedRule {
    ParsedRule {
        kind: RuleKind::Inactive(reason),
    }
}

struct Options {
    dns_types: Option<Arc<str>>,
    dns_rewrite: Option<Arc<str>>,
    client: bool,
    has_other: bool,
}

/// Splits `$option,option=value,...` and recognizes the DNS-safe subset
/// (RULE_ENGINE.md: AdGuard DNS extensions). Anything else — including
/// unrecognized options — marks `has_other`, which pushes the rule to
/// `InactiveReason::HttpOption` since DNS filtering can't honor request
/// context it never sees.
fn parse_options(raw: &str) -> Result<Options, ()> {
    let mut options = Options {
        dns_types: None,
        dns_rewrite: None,
        client: false,
        has_other: false,
    };
    if raw.is_empty() {
        return Ok(options);
    }
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(());
        }
        let (name, value) = match token.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (token, None),
        };
        let name = name.trim_start_matches('~');
        match name {
            "dnstype" => options.dns_types = value.map(Arc::from),
            "dnsrewrite" => options.dns_rewrite = value.map(Arc::from),
            "client" => options.client = true,
            _ => options.has_other = true,
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::RuleKind;

    #[test]
    fn parses_plain_block_rule() {
        let result = parse("||ads.example.com^\n");
        assert_eq!(result.active_count(), 1);
        match &result.rules[0].kind {
            RuleKind::Active(rule) => {
                assert_eq!(&*rule.domain, "ads.example.com");
                assert_eq!(rule.action, RuleAction::Block);
            }
            RuleKind::Inactive(_) => panic!("expected active rule"),
        }
    }

    #[test]
    fn parses_exception_rule() {
        let result = parse("@@||cdn.example.com^\n");
        match &result.rules[0].kind {
            RuleKind::Active(rule) => assert_eq!(rule.action, RuleAction::Allow),
            RuleKind::Inactive(_) => panic!("expected active rule"),
        }
    }

    #[test]
    fn keeps_dnstype_and_dnsrewrite_active() {
        let result =
            parse("||a.example.com^$dnstype=A|AAAA\n||b.example.com^$dnsrewrite=0.0.0.0\n");
        assert_eq!(result.active_count(), 2);
    }

    #[test]
    fn client_option_is_inactive_and_deferred() {
        let result = parse("||kids.example.com^$client=192.168.1.5\n");
        assert_eq!(result.active_count(), 0);
        assert_eq!(
            result.rules[0].kind,
            RuleKind::Inactive(InactiveReason::ClientScoped)
        );
    }

    #[test]
    fn http_option_is_inactive() {
        let result = parse("||ads.example.com^$third-party\n");
        assert_eq!(
            result.rules[0].kind,
            RuleKind::Inactive(InactiveReason::HttpOption)
        );
    }

    #[test]
    fn cosmetic_rule_is_inactive() {
        let result = parse("example.com##.ad-banner\n");
        assert_eq!(
            result.rules[0].kind,
            RuleKind::Inactive(InactiveReason::Cosmetic)
        );
    }

    /// p2-00 / U2: the qualifier after `^` used to be dropped, turning a rule
    /// about one path into a block on the whole domain.
    #[test]
    fn path_qualified_rule_is_a_url_pattern_not_a_domain_rule() {
        for rule in [
            "||paypal.com^*/pixel.gif$third-party",
            "||googleapis.com^*/gen_204?",
            "||dev.to^*/billboards/post_comments^",
            "||hltv.org^*=|$popup,domain=hltv.org",
        ] {
            let result = parse(&format!("{rule}\n"));
            assert_eq!(
                result.rules[0].kind,
                RuleKind::Inactive(InactiveReason::UrlPattern),
                "{rule} addresses a path, not a domain"
            );
            assert_eq!(result.active_count(), 0, "{rule} must yield no DNS rule");
        }
    }

    /// `|` after the separator anchors the end of the address, so the rule is
    /// still a domain rule — just not a subdomain-inclusive one.
    #[test]
    fn end_anchor_stays_active_without_subdomains() {
        let result = parse("||clarity.ms^|\n");
        match &result.rules[0].kind {
            RuleKind::Active(rule) => {
                assert_eq!(&*rule.domain, "clarity.ms");
                assert_eq!(rule.action, RuleAction::Block);
                assert!(!rule.include_subdomains, "`|` anchors the address end");
            }
            RuleKind::Inactive(_) => panic!("`||d^|` is a DNS rule"),
        }
    }

    #[test]
    fn end_anchored_exception_stays_an_allow_rule() {
        let result = parse("@@||data.notify.macys.com^|\n");
        match &result.rules[0].kind {
            RuleKind::Active(rule) => {
                assert_eq!(rule.action, RuleAction::Allow);
                assert!(!rule.include_subdomains);
            }
            RuleKind::Inactive(_) => panic!("an exception must not be dropped"),
        }
    }

    /// `||d^|$opts` does not occur in any list measured for p2-00, but it is
    /// legal syntax and must not be mistaken for a path qualifier.
    #[test]
    fn end_anchor_followed_by_options_is_still_parsed() {
        let result = parse("||a.example.com^|$dnstype=A\n");
        match &result.rules[0].kind {
            RuleKind::Active(rule) => {
                assert!(!rule.include_subdomains);
                assert_eq!(rule.dns_types.as_deref(), Some("A"));
            }
            RuleKind::Inactive(_) => panic!("expected an active rule"),
        }
    }

    /// A bare `||d^` is unchanged by the U2 fix — it still carries subdomains.
    #[test]
    fn bare_separator_still_includes_subdomains() {
        let result = parse("||ads.example.com^\n||b.example.com^$dnstype=A\n||c.example.com\n");
        for rule in &result.rules {
            match &rule.kind {
                RuleKind::Active(rule) => assert!(rule.include_subdomains),
                RuleKind::Inactive(_) => panic!("expected active rules"),
            }
        }
    }

    #[test]
    fn url_path_is_inactive() {
        let result = parse("||example.com/ads/banner.js\n");
        assert_eq!(
            result.rules[0].kind,
            RuleKind::Inactive(InactiveReason::UrlPattern)
        );
    }

    #[test]
    fn non_anchored_pattern_is_inactive() {
        let result = parse("/banner/*/ad.js\n");
        assert_eq!(
            result.rules[0].kind,
            RuleKind::Inactive(InactiveReason::UrlPattern)
        );
    }

    #[test]
    fn empty_domain_anchor_is_parse_error() {
        let result = parse("||^\n");
        assert_eq!(result.parse_errors, 1);
        assert_eq!(result.rules.len(), 0);
    }

    #[test]
    fn trailing_comma_in_options_is_parse_error() {
        let result = parse("||example.com^$script,\n");
        assert_eq!(result.parse_errors, 1);
    }

    #[test]
    fn comment_and_header_lines_are_skipped() {
        let result = parse("! comment\n[Adblock Plus 2.0]\n\n");
        assert_eq!(result.rules.len(), 0);
        assert_eq!(result.parse_errors, 0);
    }
}
