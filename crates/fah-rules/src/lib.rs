//! Rule Engine: rule-format parsers and compiled matchers (ARCHITECTURE.md L2).

mod domain;
mod format;
mod lifecycle;
mod matcher;
mod parser;
mod rule;
mod rule_list;

pub use format::{detect_format, RuleFormat};
pub use lifecycle::{
    HostResolver, LifecycleError, ListEntryView, ListManager, ListPatch, ListStatus, RefreshResult,
    RefreshStats, Resolving,
};
pub use matcher::{MatchDecision, Matcher, MatcherBuilder, RuleRef};
pub use rule::{DomainRule, InactiveReason, ParsedRule, RuleAction, RuleKind};
pub use rule_list::ParsedRuleList;

/// Parses one rule list's raw text, auto-detecting its format
/// (RULE_ENGINE.md: Supported formats). Never fails — unparseable lines are
/// skipped and counted in `ParsedRuleList::parse_errors`.
pub fn parse_rule_list(text: &str) -> ParsedRuleList {
    parser::parse(text)
}
