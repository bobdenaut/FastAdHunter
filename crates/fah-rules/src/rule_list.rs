//! The parsed result of one rule list (RULE_ENGINE.md: List lifecycle,
//! CONTEXT.md: Rule List).

use crate::format::RuleFormat;
use crate::rule::{ParsedRule, RuleKind};

/// Cap on how many error line numbers [`ParsedRuleList::parse_error_lines`]
/// retains. A corrupt 1M-line list must not turn its own errors into a
/// multi-megabyte allocation (hard rule 4, bounded everything), and the one
/// caller that needs line numbers — `PUT /api/v1/rules/user`, reporting a
/// hand-written rule block — never has anywhere near this many.
const MAX_REPORTED_ERROR_LINES: usize = 100;

/// Every rule from one list's raw text, fully parsed and classified.
/// Non-DNS and deferred rules are kept, not discarded — later phases
/// activate them without reparsing (ADR-0003). Unparseable lines are
/// skipped and only counted in `parse_errors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRuleList {
    pub format: RuleFormat,
    pub rules: Vec<ParsedRule>,
    pub parse_errors: u32,
    /// 1-based line numbers of the first [`MAX_REPORTED_ERROR_LINES`] parse
    /// errors — a bounded, truncated view of the same failures
    /// `parse_errors` totals, so `parse_error_lines.len() <= parse_errors`
    /// always. Present so the API can name the offending lines
    /// (API.md `PUT /api/v1/rules/user`: "invalid lines → 422 with per-line
    /// messages") without re-parsing each line on its own, which would
    /// misclassify formats that only make sense in context.
    pub parse_error_lines: Vec<u32>,
}

/// Running tally of a parse's failures. Parsers own one and call
/// [`ParseErrorLog::record`] wherever they previously did `parse_errors += 1`.
#[derive(Debug, Default)]
pub(crate) struct ParseErrorLog {
    count: u32,
    lines: Vec<u32>,
}

impl ParseErrorLog {
    /// Records one failure at a 0-based line index (stored 1-based, as
    /// humans and editors count).
    pub(crate) fn record(&mut self, line_index: usize) {
        self.count += 1;
        if self.lines.len() < MAX_REPORTED_ERROR_LINES {
            self.lines.push(line_index as u32 + 1);
        }
    }

    pub(crate) fn into_parts(self) -> (u32, Vec<u32>) {
        (self.count, self.lines)
    }
}

/// How many rules of each kind one [`ParsedRuleList`] holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuleCounts {
    /// Rules that answer a **domain** question — the API's `rules_active_dns`.
    pub active: usize,
    /// Rules that answer an **HTTP request** (p2-03). Reported separately
    /// rather than folded into either neighbour: they are not DNS-applicable,
    /// so they cannot join `active`, and they do filter, so calling them
    /// inactive would misreport ~22k EasyList rules as doing nothing.
    pub url: usize,
    /// Rules no tier answers yet — cosmetic (Phase 4) and patterns no
    /// supported syntax expresses.
    pub inactive: usize,
}

impl ParsedRuleList {
    /// All three counts from one walk of the rules. `RefreshStats` needs every
    /// one of them after each parse, and the per-kind accessors below delegate
    /// here, so no caller can pay a separate pass per counter.
    pub fn counts(&self) -> RuleCounts {
        let mut counts = RuleCounts::default();
        for rule in &self.rules {
            match rule.kind {
                RuleKind::Active(_) => counts.active += 1,
                RuleKind::Url(_) => counts.url += 1,
                RuleKind::Inactive(_) => counts.inactive += 1,
            }
        }
        counts
    }

    /// See [`RuleCounts::active`].
    pub fn active_count(&self) -> usize {
        self.counts().active
    }

    /// See [`RuleCounts::url`].
    pub fn url_count(&self) -> usize {
        self.counts().url
    }

    /// See [`RuleCounts::inactive`].
    pub fn inactive_count(&self) -> usize {
        self.counts().inactive
    }
}
