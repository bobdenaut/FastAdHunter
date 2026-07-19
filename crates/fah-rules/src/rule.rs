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
/// formats). Stored and counted regardless — later phases activate some of
/// these without reparsing (ADR-0003).
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

/// One parsed line from a rule list: its original text plus classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRule {
    pub raw: Arc<str>,
    pub kind: RuleKind,
}

impl ParsedRule {
    pub fn is_active(&self) -> bool {
        matches!(self.kind, RuleKind::Active(_))
    }
}
