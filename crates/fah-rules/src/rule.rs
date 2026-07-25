//! Parsed-rule types: the per-line classification result
//! (RULE_ENGINE.md: Supported formats, CONTEXT.md: Rule).

use std::sync::Arc;

/// What a matched, active [`DomainRule`] does to a query (RULE_ENGINE.md:
/// Verdicts — `Allow`/`Block`; `Pass` has no rule behind it, so it isn't
/// modeled here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Block,
    Allow,
}

/// A domain-anchored, DNS-applicable rule — active in Phase 1
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

/// Why a parsed rule is not active in Phase 1 (RULE_ENGINE.md: Supported
/// formats).
///
/// **Only the classification survives, not the rule.** This enum is the whole
/// of what an inactive rule leaves behind — see [`ParsedRule`] for why the
/// text is dropped. ADR-0003 describes inactive rules as "stored inactive,
/// counted, activated by later phases"; the storage half was never built, so a
/// phase that activates one of these variants has to reintroduce retention for
/// it (and pay the memory), not merely flip a flag. `p2-03` is the first to do
/// so, for [`Self::UrlPattern`] and [`Self::HttpOption`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InactiveReason {
    /// Cosmetic rule (`##`, `#@#`, …) — activates in the HTML phase.
    Cosmetic,
    /// Non-domain-anchored pattern (URL path, substring, regex-like) —
    /// activates in the HTTP phase.
    UrlPattern,
    /// Carries an HTTP-context option (`$script`, `$third-party`, …) DNS
    /// filtering can't honor — activates in the HTTP phase.
    HttpOption,
    /// Carries `$client` — domain-anchored and otherwise active, but scoped
    /// to a client the engine can't apply until Phase 2 (Policies).
    ClientScoped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    Active(DomainRule),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRule {
    pub kind: RuleKind,
}

impl ParsedRule {
    pub fn is_active(&self) -> bool {
        matches!(self.kind, RuleKind::Active(_))
    }
}
