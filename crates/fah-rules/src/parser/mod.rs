//! Format dispatch: routes a rule list's raw text to its parser
//! (RULE_ENGINE.md: Supported formats).

mod adblock;
mod domain_list;
mod hosts;

use crate::format::{detect_format, RuleFormat};
use crate::rule_list::ParsedRuleList;

pub(crate) fn parse(text: &str) -> ParsedRuleList {
    match detect_format(text) {
        RuleFormat::Hosts => hosts::parse(text),
        RuleFormat::PlainDomainList => domain_list::parse(text),
        RuleFormat::Adblock => adblock::parse(text),
    }
}
