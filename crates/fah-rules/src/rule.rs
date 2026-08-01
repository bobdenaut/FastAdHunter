//! Parsed-rule types: the per-line classification result
//! (RULE_ENGINE.md: Supported formats, CONTEXT.md: Rule).

use std::sync::Arc;

/// What a matched, active rule does to a query or request (RULE_ENGINE.md:
/// Verdicts — `Allow`/`Block`; `Pass` has no rule behind it, so it isn't
/// modeled here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Block,
    Allow,
}

/// A domain-anchored, DNS-applicable rule — active in the domain tier
/// (RULE_ENGINE.md: Supported formats).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainRule {
    /// Lowercased, trailing-dot-stripped domain this rule anchors to.
    pub domain: Arc<str>,
    pub action: RuleAction,
    /// True for every Phase-1 syntax this parser recognizes (`||domain^`,
    /// hosts entries, plain domain lines) — RULE_ENGINE.md's matching rules
    /// give all of them subdomain semantics.
    pub include_subdomains: bool,
    /// Raw `$dnstype` value (e.g. `"A|AAAA"`), parsed and stored, not yet
    /// interpreted — RULE_ENGINE.md lists it DNS-applicable and active.
    pub dns_types: Option<Arc<str>>,
    /// Raw `$dnsrewrite` value, parsed and stored, not yet interpreted.
    pub dns_rewrite: Option<Arc<str>>,
}

/// Where a URL pattern is anchored (RULE_ENGINE.md §HTTP matching).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlAnchor {
    /// `||example.com/ads` — matches at a domain boundary: the pattern starts
    /// at the host, or at any label boundary within it.
    Domain,
    /// `|http://example.com` — matches only at the very start of the URL.
    Start,
    /// `/ads/banner.gif` — matches anywhere in the URL.
    Anywhere,
}

/// Which party a rule applies to, from the request's relation to its referer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Any,
    /// `$third-party` — request host differs from the document's.
    Third,
    /// `$first-party` / `~third-party`.
    First,
}

/// A URL-level rule — active in the URL tier from p2-03.
///
/// This is what retention buys. Before p2-03 nothing of
/// `||paypal.com^*/pixel.gif` survived parsing except one discriminant saying
/// "URL pattern"; the text is now kept so it can be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlRule {
    /// The pattern with `@@`, anchors and `$options` stripped — the part that
    /// is matched against the URL. Lowercased unless `$match-case`.
    pub pattern: Arc<str>,
    pub action: RuleAction,
    pub anchor: UrlAnchor,
    /// Trailing `|` — the pattern must reach the end of the URL.
    pub end_anchored: bool,
    /// `$match-case`; without it the pattern and URL are compared lowercased.
    pub match_case: bool,
    pub party: Party,
    /// `$script`, `$image`, `~font`, … — a mask of [`fah_model::ResourceType`]
    /// bits the rule applies to, with negation already folded in the way
    /// `$dnstype=~A` is. **Zero means "any type"**, so a set that folds to
    /// nothing (`$script,~script`) is rejected at parse time rather than
    /// compiled into a rule that would match everything.
    pub resource_types: u16,
    /// Raw `$domain=` value (`"a.com|~b.com"`), kept verbatim; interpreting it
    /// needs the document host, which only the request has.
    pub domains: Option<Arc<str>>,
    /// Raw `$method=` value.
    pub methods: Option<Arc<str>>,
}

/// Why a parsed rule is not active in any tier (RULE_ENGINE.md: Supported
/// formats).
///
/// **Only the classification survives, not the rule.** ADR-0003 describes
/// inactive rules as "stored inactive, counted, activated by later phases";
/// the storage half was never built, so a phase that activates one of these
/// has to reintroduce retention for it (and pay the memory), not merely flip a
/// flag. p2-03 did exactly that for URL patterns and HTTP options, which is why
/// those two are no longer listed here — they compile into [`UrlRule`] now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InactiveReason {
    /// Cosmetic rule (`##`, `#@#`, …) — activates in the HTML phase (Phase 4).
    /// Deliberately still payload-free: cosmetic rules are 24,368 of EasyList
    /// alone, and retaining their text would cost the allocation-per-rule that
    /// [`ParsedRule`] exists to avoid, for a phase that cannot use them yet.
    Cosmetic,
    /// Carries `$client` — otherwise active, but scoped to a client the engine
    /// cannot apply until Policies (p2-05).
    ClientScoped,
    /// URL-shaped, but in a form the URL tier cannot express: a `/regex/`
    /// literal (PERFORMANCE.md forbids regex on the hot path) or a pattern
    /// that survives none of p2-03's supported syntax.
    UnsupportedUrlPattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// DNS tier — answers a domain question.
    Active(DomainRule),
    /// URL tier — answers an HTTP request (p2-03).
    Url(UrlRule),
    /// No tier yet.
    Inactive(InactiveReason),
}

/// One parsed line from a rule list, reduced to its classification.
///
/// Deliberately does **not** retain the original line. The compiled matcher
/// never stored it either — `Matcher::decisive_rule` reconstructs canonical
/// AdGuard syntax from the compiled record, so the API's `rule` field is fed
/// by reconstruction, not retention. Keeping the text here cost one
/// allocation, one copy and one free per rule across the whole list, on the
/// phase that dominates startup, for a field nothing read.
///
/// [`RuleKind::Url`] is the deliberate exception: its pattern *is* the rule, so
/// there is nothing to reconstruct it from. It pays that allocation for the
/// ~3 % of lines that are URL-tier, not for every line — which is the whole
/// reason [`InactiveReason::Cosmetic`] stays payload-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRule {
    pub kind: RuleKind,
}

impl ParsedRule {
    /// DNS-applicable. Unchanged in meaning: `active` in the API's list status
    /// has always meant "answers a domain question", and a URL rule does not.
    pub fn is_active(&self) -> bool {
        matches!(self.kind, RuleKind::Active(_))
    }

    /// HTTP-applicable — compiled into the URL tier.
    pub fn is_url(&self) -> bool {
        matches!(self.kind, RuleKind::Url(_))
    }
}
