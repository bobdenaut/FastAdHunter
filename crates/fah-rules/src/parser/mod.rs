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

/// A ceiling on how many rules [`parse`] can produce from `text`, computed
/// without parsing it — what [`crate::MatcherBuilder::with_capacity`] sizes
/// its transient dedup index from, in one allocation, before the first list
/// is parsed.
///
/// Every parser emits at most one rule per whitespace-separated token on a
/// content line: adblock and plain-domain lists produce one rule per *line*,
/// and a hosts line produces one per host token after the address. Blank and
/// comment lines produce none. This is deliberately a ceiling and not an
/// estimate — overshooting costs 4 unused bytes per slot, while undershooting
/// would push the dedup table past its load factor.
pub(crate) fn rule_upper_bound(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                0
            } else {
                line.split_whitespace().count()
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::RuleKind;

    /// The bound is only useful if it is never *under* the real count — the
    /// hosts format is the one that breaks a naive one-rule-per-line guess.
    #[track_caller]
    fn assert_bounds(text: &str) {
        let actual = parse(text)
            .rules
            .iter()
            .filter(|rule| matches!(rule.kind, RuleKind::Active(_)))
            .count();
        let bound = rule_upper_bound(text);
        assert!(
            bound >= actual,
            "upper bound {bound} is below the {actual} rules actually parsed from:\n{text}"
        );
    }

    #[test]
    fn upper_bound_covers_every_format() {
        assert_bounds("0.0.0.0 a.example.com b.example.com c.example.com\n0.0.0.0 d.example\n");
        assert_bounds("# comment\n\nads.example.com\ntracker.example.org\n");
        assert_bounds(
            "! title\n||ads.example.com^\n@@||cdn.example.com^\n||x.example^$dnstype=A\n",
        );
        assert_bounds("");
    }

    #[test]
    fn comment_and_blank_lines_cost_no_slots() {
        assert_eq!(
            rule_upper_bound("# a comment with many words\n\n!another\n"),
            0
        );
    }
}
