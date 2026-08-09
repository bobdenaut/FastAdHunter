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

/// The `$options` a domain rule may carry, held behind a `Box` on
/// [`DomainRule`] because inline they are 48 of its bytes and 0.11 % of the
/// deployed corpus sets any of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainOpts {
    /// Raw `$dnstype` value (e.g. `"A|AAAA"`), parsed and stored, not yet
    /// interpreted — RULE_ENGINE.md lists it DNS-applicable and active.
    pub dns_types: Option<Arc<str>>,
    /// Raw `$dnsrewrite` value, parsed and stored, not yet interpreted.
    pub dns_rewrite: Option<Arc<str>>,
    /// Raw `$client` value (`"192.168.1.5|~laptop"`) — an inline per-client
    /// policy (RULE_ENGINE.md §Policies).
    pub client: Option<Arc<str>>,
}

impl DomainOpts {
    /// `None` when no option is set, so the rule carrying none — nearly every
    /// rule — costs no allocation.
    pub fn boxed(self) -> Option<Box<Self>> {
        let bare = self.dns_types.is_none() && self.dns_rewrite.is_none() && self.client.is_none();
        if bare {
            None
        } else {
            Some(Box::new(self))
        }
    }
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
    pub opts: Option<Box<DomainOpts>>,
}

impl DomainRule {
    /// A rule carrying no `$option` — the shape of nearly every parsed line.
    pub fn plain(domain: Arc<str>, action: RuleAction, include_subdomains: bool) -> Self {
        Self {
            domain,
            action,
            include_subdomains,
            opts: None,
        }
    }

    pub fn dns_types(&self) -> Option<&str> {
        self.opts.as_ref().and_then(|o| o.dns_types.as_deref())
    }

    pub fn dns_rewrite(&self) -> Option<&str> {
        self.opts.as_ref().and_then(|o| o.dns_rewrite.as_deref())
    }

    pub fn client(&self) -> Option<&str> {
        self.opts.as_ref().and_then(|o| o.client.as_deref())
    }
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
    /// Raw `$client` value — see [`DomainRule::client`]. A URL rule can carry
    /// one too: `$client` scopes *who* a rule applies to, which is orthogonal
    /// to which tier answers it.
    pub client: Option<Arc<str>>,
}

/// Why a parsed rule is not active in any tier (RULE_ENGINE.md: Supported
/// formats).
///
/// **Only the classification survives, not the rule.** ADR-0003 describes
/// inactive rules as "stored inactive, counted, activated by later phases";
/// the storage half was never built, so a phase that activates one of these
/// has to reintroduce retention for it (and pay the memory), not merely flip a
/// flag. p2-03 did exactly that for URL patterns and HTTP options, and p2-05
/// for `$client` — which is why none of the three is listed here any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InactiveReason {
    /// Cosmetic rule (`##`, `#@#`, …) — activates in the HTML phase (Phase 4).
    /// Deliberately still payload-free: cosmetic rules are 24,368 of EasyList
    /// alone, and retaining their text would cost the allocation-per-rule that
    /// [`ParsedRule`] exists to avoid, for a phase that cannot use them yet.
    Cosmetic,
    /// Carries something no tier can honour: a `/regex/` literal
    /// (PERFORMANCE.md forbids regex on the hot path), a pattern that survives
    /// none of p2-03's supported syntax, or an option whose restriction cannot
    /// be applied — `$client` with no value, an unknown `$option`.
    ///
    /// The rule is dropped rather than applied without its restriction, which
    /// is how a narrow rule turns into a broad one.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// DNS tier — answers a domain question.
    Active(DomainRule),
    /// URL tier — answers an HTTP request (p2-03). Boxed: it is the widest
    /// variant and 0.07 % of the deployed corpus, so inline it would set the
    /// size of every rule.
    Url(Box<UrlRule>),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The width is the whole point: it multiplies by the largest list's rule
    /// count, and the compile transient peaks while that `Vec` is live.
    #[test]
    fn a_parsed_rule_is_32_bytes() {
        assert_eq!(std::mem::size_of::<ParsedRule>(), 32);
        assert_eq!(std::mem::size_of::<DomainRule>(), 32);
    }

    #[test]
    fn a_rule_carrying_no_option_allocates_nothing() {
        assert!(DomainOpts::default().boxed().is_none());
        assert!(
            DomainRule::plain(Arc::from("a.example.com"), RuleAction::Block, true)
                .opts
                .is_none()
        );
    }

    #[test]
    fn each_option_alone_is_enough_to_box_and_reads_back() {
        let one = |opts: DomainOpts| DomainRule {
            opts: opts.boxed(),
            ..DomainRule::plain(Arc::from("a.example.com"), RuleAction::Block, true)
        };
        let typed = one(DomainOpts {
            dns_types: Some(Arc::from("A")),
            ..DomainOpts::default()
        });
        let rewritten = one(DomainOpts {
            dns_rewrite: Some(Arc::from("0.0.0.0")),
            ..DomainOpts::default()
        });
        let scoped = one(DomainOpts {
            client: Some(Arc::from("192.168.1.5")),
            ..DomainOpts::default()
        });
        assert_eq!(typed.dns_types(), Some("A"));
        assert_eq!(typed.dns_rewrite(), None);
        assert_eq!(rewritten.dns_rewrite(), Some("0.0.0.0"));
        assert_eq!(scoped.client(), Some("192.168.1.5"));
    }
}
