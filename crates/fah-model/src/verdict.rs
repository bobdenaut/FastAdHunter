use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The rule and rule list that decided a [`Verdict`] (RULE_ENGINE.md: Verdicts).
///
/// `Arc<str>` rather than `String`: the Rule Engine builds one of these per
/// matched query, on the hot path (runs before the cache). Cloning an `Arc`
/// is an atomic refcount bump, not an allocation (PERFORMANCE.md: hot path
/// is allocation-free; allocations happen at load/reload time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisiveRule {
    pub list: Arc<str>,
    pub rule: Arc<str>,
}

impl DecisiveRule {
    pub fn new(list: impl Into<Arc<str>>, rule: impl Into<Arc<str>>) -> Self {
        Self {
            list: list.into(),
            rule: rule.into(),
        }
    }
}

/// The Rule Engine's decision for one query (CONTEXT.md: Verdict).
///
/// Allow always wins over Block; Pass means no rule matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Allow(DecisiveRule),
    Block(DecisiveRule),
    Pass,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_serde_roundtrip() {
        for verdict in [
            Verdict::Allow(DecisiveRule::new("allowlist", "@@||cdn.example.com^")),
            Verdict::Block(DecisiveRule::new("blocklist", "||ads.example.com^")),
            Verdict::Pass,
        ] {
            let json = serde_json::to_string(&verdict).unwrap();
            let back: Verdict = serde_json::from_str(&json).unwrap();
            assert_eq!(verdict, back);
        }
    }
}
