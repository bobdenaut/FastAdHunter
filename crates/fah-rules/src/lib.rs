//! Rule Engine: rule-format parsers and compiled matchers (ARCHITECTURE.md L2).

mod domain;
mod format;
mod lifecycle;
mod matcher;
mod parser;
mod policy;
mod resource;
mod rule;
mod rule_list;
mod url_matcher;

pub use format::{detect_format, RuleFormat};
pub use lifecycle::{
    HostResolver, LifecycleError, ListEntryView, ListManager, ListPatch, ListRefreshOutcome,
    ListStatus, RefreshResult, RefreshStats, Resolving,
};
pub use matcher::{ClientContext, MatchDecision, Matcher, MatcherBuilder, RuleRef};
pub use policy::{ActivePolicies, PolicyError, PolicySet, PolicyState};
pub use rule::{
    DomainOpts, DomainRule, InactiveReason, ParsedRule, Party, RuleAction, RuleKind, UrlAnchor,
    UrlRule,
};
pub use rule_list::{ParsedRuleList, RuleCounts};

/// Parses one rule list's raw text, auto-detecting its format
/// (RULE_ENGINE.md: Supported formats). Never fails — unparseable lines are
/// skipped and counted in `ParsedRuleList::parse_errors`.
pub fn parse_rule_list(text: &str) -> ParsedRuleList {
    parser::parse(text)
}
