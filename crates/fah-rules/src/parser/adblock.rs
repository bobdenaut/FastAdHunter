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
//! - **inactive** — cosmetic (Phase 4), and patterns or options no supported
//!   syntax can express. `$client` left this list in p2-05: it scopes a rule to
//!   a client, which is orthogonal to which tier answers it, so it is now
//!   retained on whichever rule carries it.
//!
//! The domain tier is tried first and is deliberately narrow: only a rule that
//! is *purely* about a name stays there. `||paypal.com^*/pixel.gif` addresses a
//! URL, and treating it as a domain rule is what once blocked all of PayPal.

use std::sync::Arc;

use crate::domain::normalize_domain;
use crate::format::RuleFormat;
use crate::policy::ClientScope;
use crate::resource::{bit_for_option, ALL_TYPES};
use crate::rule::{
    DomainOpts, DomainRule, InactiveReason, ParsedRule, Party, RuleAction, RuleKind, UrlAnchor,
    UrlRule,
};
use crate::rule_list::{ParseErrorLog, ParsedRuleList};

const COSMETIC_MARKERS: [&str; 4] = ["##", "#@#", "#?#", "#$#"];

pub(crate) fn parse(text: &str) -> ParsedRuleList {
    let mut rules = Vec::new();
    let mut errors = ParseErrorLog::default();

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        // One byte load for all three prefix tests — a multi-`char` pattern
        // costs a searcher, and this runs on every line of every list.
        let Some(&first) = line.as_bytes().first() else {
            continue;
        };
        // `#` is not tested here: `###id` opens with it and is a cosmetic rule,
        // so it can only be read as a comment once the marker scan has ruled
        // that out.
        if first == b'!' || first == b'[' {
            continue;
        }

        if let Some(at) = COSMETIC_MARKERS
            .iter()
            .filter_map(|marker| line.find(marker))
            .min()
        {
            if line[..at].bytes().any(|byte| byte.is_ascii_whitespace()) {
                errors.record(index);
            } else {
                rules.push(inactive(InactiveReason::Cosmetic));
            }
            continue;
        }

        // With cosmetic syntax ruled out, `#` is the comment it is in every
        // other format (`format::is_ignorable`). Falling through here compiles
        // the comment's text into a live URL substring rule.
        if first == b'#' {
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

        // Checked before either tier is tried, because an option nothing can
        // honour is not a property of the pattern's shape: `||ads.example.com^
        // $client` is domain-shaped and still unusable. Until p2-05 every site
        // that set `unsupported` happened to set `http_scoped` too, so the URL
        // arm caught them all — an invariant held by accident, which `$client`
        // (scoped to a client, not to a request) breaks.
        if options.unsupported {
            rules.push(inactive(InactiveReason::Unsupported));
            continue;
        }

        match domain_rule(pattern, exception, &options) {
            DomainVerdict::Rule(rule) => rules.push(ParsedRule {
                kind: RuleKind::Active(rule),
            }),
            DomainVerdict::Malformed => errors.record(index),
            DomainVerdict::NotADomainRule => {
                if pattern.bytes().any(|byte| byte.is_ascii_whitespace()) {
                    errors.record(index);
                } else {
                    rules.push(url_rule(pattern, exception, &options));
                }
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
        opts: DomainOpts {
            dns_types: options.dns_types.clone(),
            dns_rewrite: options.dns_rewrite.clone(),
            client: options.client.clone(),
        }
        .boxed(),
    })
}

/// Compiles what the domain tier rejected into a URL-tier rule — or classifies
/// it inactive when no supported syntax expresses it.
fn url_rule(pattern: &str, exception: bool, options: &Options) -> ParsedRule {
    // A `/regex/` literal would need a regex engine, which PERFORMANCE.md
    // forbids on the hot path. Recognized so it is classified rather than
    // matched literally, which would silently never fire.
    if pattern.len() > 2 && pattern.starts_with('/') && pattern.ends_with('/') {
        return inactive(InactiveReason::Unsupported);
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
        return inactive(InactiveReason::Unsupported);
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
        return inactive(InactiveReason::Unsupported);
    }

    // Matching is case-insensitive unless `$match-case`, so the stored pattern
    // is folded once here rather than per lookup. `to_ascii_lowercase` on the
    // rare `match_case` rule would corrupt it, hence the branch.
    let text: Arc<str> = if options.match_case {
        Arc::from(body)
    } else {
        folded(body, AsciiCase::Lower)
    };

    ParsedRule {
        kind: RuleKind::Url(Box::new(UrlRule {
            pattern: text,
            action: action(exception),
            anchor,
            end_anchored,
            match_case: options.match_case,
            party: options.party,
            resource_types: options.resource_types,
            domains: options.domains.clone(),
            methods: options.methods.clone(),
            client: options.client.clone(),
        })),
    }
}

fn action(exception: bool) -> RuleAction {
    if exception {
        RuleAction::Allow
    } else {
        RuleAction::Block
    }
}

/// The ASCII case a payload is folded to before it is stored, so matching can
/// compare without folding either side.
#[derive(Clone, Copy)]
enum AsciiCase {
    Lower,
    Upper,
}

/// `text` as an `Arc<str>` in `case`, skipping the intermediate `String` that
/// `Arc::from(text.to_ascii_lowercase().as_str())` allocates and then copies.
/// Payloads arrive in the target case often enough that the scan deciding it
/// usually replaces an allocation. `normalize_domain` folds the same way, fused
/// into the validity scan it has to run regardless.
fn folded(text: &str, case: AsciiCase) -> Arc<str> {
    let folds = match case {
        AsciiCase::Lower => text.bytes().any(|byte| byte.is_ascii_uppercase()),
        AsciiCase::Upper => text.bytes().any(|byte| byte.is_ascii_lowercase()),
    };
    if !folds {
        return Arc::from(text);
    }
    match case {
        AsciiCase::Lower => Arc::from(text.to_ascii_lowercase().as_str()),
        AsciiCase::Upper => Arc::from(text.to_ascii_uppercase().as_str()),
    }
}

struct Options {
    dns_types: Option<Arc<str>>,
    dns_rewrite: Option<Arc<str>>,
    /// Raw `$client` payload. Active since p2-05 in **both** tiers: it scopes
    /// who a rule applies to, which says nothing about whether a domain or a
    /// URL answers it.
    client: Option<Arc<str>>,
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
        client: None,
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
            // Validated with the compiler's own parser, so both tiers agree on
            // which payloads are usable and the rule is refused *here*, where
            // it is counted. A payload that compiles to no selector is dropped
            // rather than applied without its restriction, which would widen a
            // one-device rule to the whole network.
            "client" => match value {
                Some(value) if ClientScope::parse(value).is_some() => {
                    options.client = Some(Arc::from(value))
                }
                _ => options.unsupported = true,
            },
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
                        options.domains = Some(folded(value, AsciiCase::Lower))
                    }
                    // `$domain` with no value restricts to nothing at all.
                    _ => options.unsupported = true,
                }
            }
            "method" => {
                options.http_scoped = true;
                match value {
                    Some(value) if !value.is_empty() => {
                        options.methods = Some(folded(value, AsciiCase::Upper))
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
            RuleKind::Url(rule) => *rule,
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

    /// p2-05: a `$client` rule is active and keeps its payload. It used to be
    /// classified inactive, on the reasoning that activating it would apply it
    /// to every client — true only while nothing could identify one.
    #[test]
    fn client_option_is_active_and_retains_its_payload() {
        let result = parse("||kids.example.com^$client=192.168.1.5\n");
        assert_eq!(result.active_count(), 1);
        let RuleKind::Active(rule) = &result.rules[0].kind else {
            panic!("expected a domain rule, got {:?}", result.rules[0].kind);
        };
        assert_eq!(rule.client(), Some("192.168.1.5"));
    }

    /// `$client` says *who*, an HTTP option says *what*. A rule carrying both
    /// is a URL rule scoped to a client, not an inactive one.
    #[test]
    fn client_scoping_survives_alongside_http_options() {
        let RuleKind::Url(rule) = one("||ads.example.com^$client=192.168.1.5,third-party") else {
            panic!("expected a URL rule");
        };
        assert_eq!(rule.client.as_deref(), Some("192.168.1.5"));
        assert_eq!(rule.party, Party::Third);
    }

    /// A `$client` with nothing after it restricts to nothing nameable, and is
    /// dropped rather than applied to the whole network.
    #[test]
    fn a_valueless_client_option_is_unsupported() {
        assert_eq!(
            one("||ads.example.com^$client"),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
        assert_eq!(
            one("||ads.example.com^$client="),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
    }

    /// The compiler drops a `$client` payload that compiles to no selector, so
    /// the parser refuses it first — otherwise the rule is counted active and
    /// filters nothing.
    #[test]
    fn a_client_payload_the_compiler_cannot_use_is_unsupported() {
        assert_eq!(
            one("||ads.example.com^$client=10.0.0.1/99"),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
        assert_eq!(
            one("||ads.example.com^$client=|"),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
    }

    /// `#` is a comment here as it is in every other format. Read as a rule it
    /// becomes a live URL substring pattern counted in `rules_active_url`.
    #[test]
    fn hash_comments_produce_no_rule() {
        let result = parse("# Title: a hybrid list\n#\n||ads.example.com^\n");
        assert_eq!(result.rules.len(), 1);
        assert_eq!(result.active_count(), 1);
        assert_eq!(result.url_count(), 0);
        assert_eq!(result.parse_errors, 0);
    }

    /// Why `#` is tested *after* the marker scan: a generic cosmetic rule opens
    /// with it.
    #[test]
    fn a_generic_cosmetic_rule_still_opens_with_a_hash() {
        assert_eq!(
            one("###banner-ad-container"),
            RuleKind::Inactive(InactiveReason::Cosmetic)
        );
    }

    /// Why `!` is tested *before* it: a comment may quote cosmetic syntax.
    #[test]
    fn a_comment_quoting_a_cosmetic_marker_is_not_a_rule() {
        let result = parse("! use example.com##.ad-banner to hide banners\n");
        assert_eq!(result.rules.len(), 0);
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
            RuleKind::Inactive(InactiveReason::Unsupported)
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
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
    }

    #[test]
    fn an_unrecognized_option_drops_the_rule_rather_than_widening_it() {
        // `$popup` restricts the rule; honoring the pattern while ignoring the
        // restriction would apply it to ordinary navigation too.
        assert_eq!(
            one("||hltv.org^*=|$popup,domain=hltv.org"),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
        assert_eq!(
            one("||ads.example.com^$removeparam=utm_source"),
            RuleKind::Inactive(InactiveReason::Unsupported)
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
            RuleKind::Inactive(InactiveReason::Unsupported)
        );

        let entries: Vec<String> = (0..7_000).map(|i| format!("d{i}.example.com")).collect();
        let long_domains = format!("||big.example.com^$domain={}", entries.join("|"));
        assert_eq!(
            one(&long_domains),
            RuleKind::Inactive(InactiveReason::Unsupported)
        );
    }

    #[test]
    fn an_options_only_line_matches_nothing_rather_than_everything() {
        assert_eq!(
            one("$third-party"),
            RuleKind::Inactive(InactiveReason::Unsupported)
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
                assert_eq!(rule.dns_types(), Some("A"));
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

    #[test]
    fn a_pattern_containing_whitespace_is_a_parse_error() {
        let result = parse("||valid.example^\nthis is not a rule at all\n");
        assert_eq!(result.parse_errors, 1);
        assert_eq!(result.parse_error_lines, vec![2]);
        assert_eq!(result.active_count(), 1);

        let result = parse("||a.example^\n!!x\nbad line ###\n");
        assert_eq!(result.parse_errors, 1);
    }

    #[test]
    fn whitespace_in_option_payload_is_not_the_pattern_error_path() {
        let result = parse("||ads.example.com^$dnsrewrite=NOERROR\n");
        assert_eq!(result.parse_errors, 0);
        assert_eq!(result.active_count(), 1);
    }
}
