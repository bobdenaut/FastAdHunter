//! Compiled-ruleset snapshot — this crate's own DTO. The binary reads
//! `Matcher::len()` / `Matcher::heap_bytes()` off `ListManager::matcher()`
//! and times its own `refresh_list`/`boot`/`set_user_rules` calls, then pushes
//! the result through [`Metrics::set_ruleset`] (siblings never import each
//! other, ARCHITECTURE.md §Dependency Layering).

use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RulesetSnapshot {
    /// Distinct compiled rules — `rules` + `duplicates_removed` is what the
    /// lists parsed to before the merge collapsed identical rules.
    pub rules: usize,
    pub heap_bytes: usize,
    pub compile_duration: Duration,
    /// Rules the last compile dropped as exact duplicates of one already
    /// present (`Matcher::duplicates_removed`). Zero is the normal reading
    /// for a single list; a large number means two lists carry the same
    /// corpus, which is the memory this figure exists to make visible.
    pub duplicates_removed: usize,
}
