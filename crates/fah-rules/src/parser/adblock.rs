//! EasyList / uBlock Origin / AdGuard format parser (RULE_ENGINE.md:
//! Supported formats — one syntax family, one parser). AdGuard's DNS options
//! (`$dnstype`, `$dnsrewrite`, `$client`) are recognized here, not treated
//! as a separate format.
//!
//! # Two tiers out of one syntax
//!
//! A line is classified into the tier that can actually answer it:
//!
//! - **domain tier** — `||domain^`, optionally with DNS-safe options. Answers
//!   a DNS question.
//! - **URL tier** (p2-03) — everything else that addresses a request: path
//!   qualifiers, wildcards, non-anchored substrings, and the HTTP `$options`.
//!   Its pattern text is **retained**, because unlike a domain rule there is
//!   nothing to reconstruct it from.
//! - **inactive** — cosmetic (Phase 4), `$client` (p2-05), and patterns no
//!   supported syntax can express.
//!
//! The domain tier is tried first and is deliberately narrow: only a rule that
//! is *purely* about a name stays there. `||paypal.com^*/pixel.gif` addresses a
//! URL, and treating it as a domain rule is what once blocked all of PayPal.

use std::sync::Arc;

use crate::domain::normalize_domain;
use crate::format::RuleFormat;
use crate::resource::{bit_for_option, ALL_TYPES};
use crate::rule::{
    DomainRule, InactiveReason, ParsedRule, Party, RuleAction, RuleKind, UrlAnchor, UrlRule,
};
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
        let (pattern, options_str) = split_options(body);

        let options = match parse_options(options_str) {
            Ok(options) => options,
            Err(()) => {
                errors.record(index);
                continue;
            }
        };

        // `$client` scopes a rule to a client the engine cannot identify until
        // Policies (p2-05). That is true whichever tier the pattern would
        // otherwise land in, so it is decided before either — activating such a
        // rule in the URL tier would apply it to *every* client.
        if options.client {
            rules.push(inactive(InactiveReason::ClientScoped));
            continue;
        }

        match domain_rule(pattern, exception, &options) {
            DomainVerdict::Rule(rule) => rules.push(ParsedRule {
                kind: RuleKind::Active(rule),
            }),
            DomainVerdict::Malformed => errors.record(index),
            DomainVerdict::NotADomainRule => {
                rules.push(url_rule(pattern, exception, &options));
            }
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

/// Splits `pattern$option,option=value` at the option separator.
///
/// The **last** unescaped `$` wins, and whatever follows it is options — so a
/// malformed option list is a parse error rather than being silently re-read as
/// part of the pattern. A leading `$` splits too, leaving an empty pattern,
/// which [`url_rule`] then refuses rather than compiling into a match-anything
/// rule.
fn split_options(body: &str) -> (&str, &str) {
    let bytes = body.as_bytes();
    let mut at = None;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'$' && (index == 0 || bytes[index - 1] != b'\\') {
            at = Some(index);
        }
    }
    match at {
        Some(index) => (&body[..index], &body[index + 1..]),
        None => (body, ""),
    }
}

enum DomainVerdict {
    Rule(DomainRule),
    /// Syntactically broken as a domain rule and not salvageable as a URL one
    /// either — `||^` has no name and no pattern.
    Malformed,
    NotADomainRule,
}

/// Decides whether `pattern` is *purely* about a name, and builds the domain
/// rule if so.
///
/// Only three things may follow `||domain`: nothing, `^`, or `^|`. Anything
/// else is a path or wildcard qualifier and the rule addresses a URL. Options
/// narrow it further — a rule carrying `$script` is about a request even when
/// its pattern is a bare domain.
fn domain_rule(pattern: &str, exception: bool, options: &Options) -> DomainVerdict {
    let Some(after_anchor) = pattern.strip_prefix("||") else {
        return DomainVerdict::NotADomainRule;
    };

    let end = after_anchor.find(['^', '/']).unwrap_or(after_anchor.len());
    let domain_part = &after_anchor[..end];
    if domain_part.is_empty() {
        // `||^` and `||/path` name nothing; there is no pattern to keep.
        return DomainVerdict::Malformed;
    }
    let Some(domain) = normalize_domain(domain_part) else {
        return DomainVerdict::NotADomainRule;
    };

    // `|` immediately after the separator anchors the end of the address:
    // `||d^|` addresses `d` itself, so it must not carry the subdomain
    // semantics a bare `||d^` does.
    let include_subdomains = match &after_anchor[end..] {
        "" => true,
        "^" => true,
        "^|" => false,
        _ => return DomainVerdict::NotADomainRule,
    };

    if options.http_scoped {
        return DomainVerdict::NotADomainRule;
    }

    DomainVerdict::Rule(DomainRule {
        domain,
        action: action(exception),
        include_subdomains,
        dns_types: options.dns_types.clone(),
        dns_rewrite: options.dns_rewrite.clone(),
    })
}

/// Compiles what the domain tier rejected into a URL-tier rule — or classifies
/// it inactive when no supported syntax expresses it.
fn url_rule(pattern: &str, exception: bool, options: &Options) -> ParsedRule {
    if options.unsupported {
        return inactive(InactiveReason::UnsupportedUrlPattern);
    }
    // A `/regex/` literal would need a regex engine, which PERFORMANCE.md
    // forbids on the hot path. Recognized so it is classified rather than
    // matched literally, which would silently never fire.
    if pattern.len() > 2 && pattern.starts_with('/') && pattern.ends_with('/') {
        return inactive(InactiveReason::UnsupportedUrlPattern);
    }

    let (anchor, body) = if let Some(rest) = pattern.strip_prefix("||") {
        (UrlAnchor::Domain, rest)
    } else if let Some(rest) = pattern.strip_prefix('|') {
        (UrlAnchor::Start, rest)
    } else {
        (UrlAnchor::Anywhere, pattern)
    };
    let (body, end_anchored) = match body.strip_suffix('|') {
        Some(head) => (head, true),
        None => (body, false),
    };

    // An empty pattern matches every URL, so a bare `$third-party` would block
    // the whole web on the strength of one option. Refused rather than
    // compiled.
    if body.is_empty() {
        return inactive(InactiveReason::UnsupportedUrlPattern);
    }

    // The compiled record addresses both arenas with 16-bit lengths, so a
    // payload past that cannot be compiled. Refused *here*, where it is
    // counted: the compiler used to drop it silently, which left the rule in
    // `rules_active_url` — reported as filtering while filtering nothing, the
    // exact class of lie that counter was split out to end.
    let too_long = body.len() > u16::MAX as usize
        || options
            .domains
            .as_deref()
            .is_some_and(|domains| domains.len() > u16::MAX as usize);
    if too_long {
        return inactive(InactiveReason::UnsupportedUrlPattern);
    }

    // Matching is case-insensitive unless `$match-case`, so the stored pattern
    // is folded once here rather than per lookup. `to_ascii_lowercase` on the
    // rare `match_case` rule would corrupt it, hence the branch.
    let text: Arc<str> = if options.match_case {
        Arc::from(body)
    } else {
        Arc::from(body.to_ascii_lowercase().as_str())
    };

    ParsedRule {
        kind: RuleKind::Url(UrlRule {
            pattern: text,
            action: action(exception),
            anchor,
            end_anchored,
            match_case: options.match_case,
            party: options.party,
            resource_types: options.resource_types,
            domains: options.domains.clone(),
            methods: options.methods.clone(),
        }),
    }
}

fn action(exception: bool) -> RuleAction {
    if exception {
        RuleAction::Allow
    } else {
        RuleAction::Block
    }
}

struct Options {
    dns_types: Option<Arc<str>>,
    dns_rewrite: Option<Arc<str>>,
    client: bool,
    party: Party,
    resource_types: u16,
    domains: Option<Arc<str>>,
    methods: Option<Arc<str>>,
    match_case: bool,
    /// Carries an option only a request can answer, so the rule cannot be a
    /// domain rule however name-shaped its pattern is.
    http_scoped: bool,
    /// Carries an option no tier can honor. The rule is dropped rather than
    /// applied without its restriction, which would over-block.
    unsupported: bool,
}

/// Splits `$option,option=value,...` and recognizes the DNS-safe subset
/// (RULE_ENGINE.md: AdGuard DNS extensions) plus the HTTP subset p2-03
/// activates. An option in neither set marks `unsupported`: applying a rule
/// while ignoring the restriction it carries is how a narrow rule turns into a
/// broad one.
fn parse_options(raw: &str) -> Result<Options, ()> {
    let mut options = Options {
        dns_types: None,
        dns_rewrite: None,
        client: false,
        party: Party::Any,
        resource_types: 0,
        domains: None,
        methods: None,
        match_case: false,
        http_scoped: false,
        unsupported: false,
    };
    if raw.is_empty() {
        return Ok(options);
    }

    let mut positive_types = 0u16;
    let mut negated_types = 0u16;
    let mut saw_type = false;

    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(());
        }
        let (name, value) = match token.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (token, None),
        };
        let (name, negated) = match name.strip_prefix('~') {
            Some(rest) => (rest, true),
            None => (name, false),
        };

        if let Some(bit) = bit_for_option(name) {
            saw_type = true;
            options.http_scoped = true;
            if negated {
                negated_types |= bit;
            } else {
                positive_types |= bit;
            }
            continue;
        }

        match name {
            "dnstype" => options.dns_types = value.map(Arc::from),
            "dnsrewrite" => options.dns_rewrite = value.map(Arc::from),
            "client" => options.client = true,
            "third-party" | "3p" => {
                options.http_scoped = true;
                options.party = if negated { Party::First } else { Party::Third };
            }
            "first-party" | "1p" => {
                options.http_scoped = true;
                options.party = if negated { Party::Third } else { Party::First };
            }
            "domain" | "from" => {
                options.http_scoped = true;
                match value {
                    Some(value) if !value.is_empty() => {
                        options.domains = Some(Arc::from(value.to_ascii_lowercase().as_str()))
                    }
                    // `$domain` with no value restricts to nothing at all.
                    _ => options.unsupported = true,
                }
            }
            "method" => {
                options.http_scoped = true;
                match value {
                    Some(value) if !value.is_empty() => {
                        options.methods = Some(Arc::from(value.to_ascii_uppercase().as_str()))
                    }
                    _ => options.unsupported = true,
                }
            }
            "match-case" => {
                options.http_scoped = true;
                options.match_case = !negated;
            }
            _ => {
                options.http_scoped = true;
                options.unsupported = true;
            }
        }
    }

    if saw_type {
        // Negation folds away at compile time, exactly as `$dnstype=~A` does:
        // a purely negated set subtracts from everything, a mixed one
        // intersects. A set that folds to nothing can never match, so it is
        // unsupported rather than a rule with `resource_types == 0` — which
        // means "any type".
        options.resource_types = if positive_types == 0 {
            ALL_TYPES & !negated_types
        } else {
            positive_types & !negated_types
        };
        if options.resource_types == 0 {
            options.unsupported = true;
        }
    }

    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::RuleKind;

    fn one(text: &str) -> RuleKind {
        let result = parse(&format!("{text}\n"));
        assert_eq!(result.rules.len(), 1, "{text} should parse to one rule");
        result.rules.into_iter().next().unwrap().kind
    }

    fn url_of(text: &str) -> UrlRule {
        match one(text) {
            RuleKind::Url(rule) => rule,
            other => panic!("{text} should be a URL rule, got {other:?}"),
        }
    }

    #[test]
    fn parses_plain_block_rule() {
        let result = parse("||ads.example.com^\n");
        assert_eq!(result.active_count(), 1);
        match &result.rules[0].kind {
            RuleKind::Active(rule) => {
                assert_eq!(&*rule.domain, "ads.example.com");
                assert_eq!(rule.action, RuleAction::Block);
            }
            other => panic!("expected active rule, got {other:?}"),
        }
    }

    #[test]
    fn parses_exception_rule() {
        let result = parse("@@||cdn.example.com^\n");
        match &result.rules[0].kind {
            RuleKind::Active(rule) => assert_eq!(rule.action, RuleAction::Allow),
            other => panic!("expected active rule, got {other:?}"),
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

    /// `$client` decides the tier on its own: activating the URL half would
    /// apply a client-scoped rule to every client.
    #[test]
    fn client_option_wins_over_http_options() {
        assert_eq!(
            one("||ads.example.com^$client=192.168.1.5,third-party"),
            RuleKind::Inactive(InactiveReason::ClientScoped)
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

    // ─── Retention (p2-03) ────────────────────────────────────────────────

    /// The heart of the retention change: before p2-03 every one of these kept
    /// nothing but a discriminant saying "URL pattern".
    #[test]
    fn path_qualified_rules_become_url_rules_with_their_text() {
        let rule = url_of("||paypal.com^*/pixel.gif$third-party");
        assert_eq!(&*rule.pattern, "paypal.com^*/pixel.gif");
        assert_eq!(rule.anchor, UrlAnchor::Domain);
        assert_eq!(rule.party, Party::Third);
        assert_eq!(rule.action, RuleAction::Block);
        assert!(!rule.end_anchored);

        let rule = url_of("||googleapis.com^*/gen_204?");
        assert_eq!(&*rule.pattern, "googleapis.com^*/gen_204?");

        let rule = url_of("||example.com/ads/banner.js");
        assert_eq!(&*rule.pattern, "example.com/ads/banner.js");
        assert_eq!(rule.anchor, UrlAnchor::Domain);
    }

    /// A path-qualified rule must still yield no DNS rule — the p2-00 fix.
    #[test]
    fn a_url_rule_is_not_dns_applicable() {
        let result = parse("||paypal.com^*/pixel.gif$third-party\n");
        assert_eq!(result.active_count(), 0);
        assert!(result.rules[0].is_url());
        assert!(!result.rules[0].is_active());
    }

    #[test]
    fn non_anchored_pattern_is_a_url_rule() {
        let rule = url_of("/banner/*/ad.js");
        assert_eq!(rule.anchor, UrlAnchor::Anywhere);
        assert_eq!(&*rule.pattern, "/banner/*/ad.js");
    }

    /// EasyList's actual first content line — the one that used to defeat
    /// format detection entirely.
    #[test]
    fn a_bare_substring_pattern_is_a_url_rule() {
        let rule = url_of("&rb=&uuid=$third-party");
        assert_eq!(rule.anchor, UrlAnchor::Anywhere);
        assert_eq!(&*rule.pattern, "&rb=&uuid=");
        assert_eq!(rule.party, Party::Third);
    }

    #[test]
    fn address_anchors_are_recorded_not_kept_in_the_pattern() {
        let rule = url_of("|http://ads.example.com/track|");
        assert_eq!(rule.anchor, UrlAnchor::Start);
        assert!(rule.end_anchored);
        assert_eq!(&*rule.pattern, "http://ads.example.com/track");
    }

    /// A domain-shaped pattern with an HTTP option is a *request* rule: it can
    /// only be judged once the request exists, so it must not stay in the DNS
    /// tier where the option would be ignored.
    #[test]
    fn a_domain_pattern_with_an_http_option_moves_to_the_url_tier() {
        let rule = url_of("||ads.example.com^$third-party");
        assert_eq!(&*rule.pattern, "ads.example.com^");
        assert_eq!(rule.anchor, UrlAnchor::Domain);
        assert_eq!(rule.party, Party::Third);
    }

    #[test]
    fn resource_type_options_fold_into_a_mask() {
        let script = url_of("||ads.example.com^$script").resource_types;
        let image = url_of("||ads.example.com^$image").resource_types;
        assert_ne!(script, 0);
        assert_ne!(script, image);
        assert_eq!(
            url_of("||ads.example.com^$script,image").resource_types,
            script | image
        );
        // Negation subtracts from everything, as `$dnstype=~A` does.
        assert_eq!(
            url_of("||ads.example.com^$~script").resource_types,
            ALL_TYPES & !script
        );
        // No type option at all means "any type".
        assert_eq!(url_of("||ads.example.com^$third-party").resource_types, 0);
    }

    #[test]
    fn a_type_set_that_folds_to_nothing_is_refused() {
        assert_eq!(
            one("||ads.example.com^$script,~script"),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
    }

    #[test]
    fn domain_and_method_payloads_are_retained_normalized() {
        let rule = url_of("||ads.example.com^$domain=A.Com|~b.com");
        assert_eq!(rule.domains.as_deref(), Some("a.com|~b.com"));
        let rule = url_of("||ads.example.com^$method=get|post");
        assert_eq!(rule.methods.as_deref(), Some("GET|POST"));
    }

    #[test]
    fn patterns_are_folded_unless_match_case_says_otherwise() {
        assert_eq!(
            &*url_of("||Ads.Example.COM/Track").pattern,
            "ads.example.com/track"
        );
        let cased = url_of("||Ads.Example.COM/Track$match-case");
        assert_eq!(&*cased.pattern, "Ads.Example.COM/Track");
        assert!(cased.match_case);
    }

    #[test]
    fn party_negation_flips_the_side() {
        assert_eq!(url_of("||a.example.com^$~third-party").party, Party::First);
        assert_eq!(url_of("||a.example.com^$first-party").party, Party::First);
        assert_eq!(url_of("||a.example.com^$~first-party").party, Party::Third);
    }

    #[test]
    fn exceptions_survive_into_the_url_tier() {
        let rule = url_of("@@||cdn.example.com/assets/$script");
        assert_eq!(rule.action, RuleAction::Allow);
    }

    // ─── Rules no tier can express ────────────────────────────────────────

    #[test]
    fn a_regex_literal_is_unsupported() {
        assert_eq!(
            one("/^https?:\\/\\/ads\\./"),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
    }

    #[test]
    fn an_unrecognized_option_drops_the_rule_rather_than_widening_it() {
        // `$popup` restricts the rule; honoring the pattern while ignoring the
        // restriction would apply it to ordinary navigation too.
        assert_eq!(
            one("||hltv.org^*=|$popup,domain=hltv.org"),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
        assert_eq!(
            one("||ads.example.com^$removeparam=utm_source"),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
    }

    /// The compiled record addresses both arenas with 16-bit lengths. A payload
    /// past that used to parse as a URL rule and then be dropped by the
    /// compiler without a word, leaving it counted in `rules_active_url` while
    /// filtering nothing.
    #[test]
    fn a_payload_too_large_for_the_compiled_record_is_refused_where_it_is_counted() {
        let long_pattern = format!("||big.example.com/{}", "a".repeat(70_000));
        assert_eq!(
            one(&long_pattern),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );

        let entries: Vec<String> = (0..7_000).map(|i| format!("d{i}.example.com")).collect();
        let long_domains = format!("||big.example.com^$domain={}", entries.join("|"));
        assert_eq!(
            one(&long_domains),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
    }

    #[test]
    fn an_options_only_line_matches_nothing_rather_than_everything() {
        assert_eq!(
            one("$third-party"),
            RuleKind::Inactive(InactiveReason::UnsupportedUrlPattern)
        );
    }

    // ─── Domain tier, unchanged by p2-03 ──────────────────────────────────

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
            other => panic!("`||d^|` is a DNS rule, got {other:?}"),
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
            other => panic!("an exception must not be dropped, got {other:?}"),
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
            other => panic!("expected an active rule, got {other:?}"),
        }
    }

    /// A bare `||d^` is unchanged by the U2 fix — it still carries subdomains.
    #[test]
    fn bare_separator_still_includes_subdomains() {
        let result = parse("||ads.example.com^\n||b.example.com^$dnstype=A\n||c.example.com\n");
        for rule in &result.rules {
            match &rule.kind {
                RuleKind::Active(rule) => assert!(rule.include_subdomains),
                other => panic!("expected active rules, got {other:?}"),
            }
        }
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

    /// The option split takes the *last* `$`, so a pattern containing one is
    /// still read as a pattern up to the real option separator.
    #[test]
    fn a_dollar_inside_the_pattern_does_not_swallow_the_options() {
        let rule = url_of("||shop.example.com/price$5/track$image");
        assert_eq!(&*rule.pattern, "shop.example.com/price$5/track");
        assert_ne!(rule.resource_types, 0);
    }

    #[test]
    fn comment_and_header_lines_are_skipped() {
        let result = parse("! comment\n[Adblock Plus 2.0]\n\n");
        assert_eq!(result.rules.len(), 0);
        assert_eq!(result.parse_errors, 0);
    }
}
