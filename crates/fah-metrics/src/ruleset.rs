//! Compiled-ruleset snapshot — this crate's own DTO. The binary reads
//! `Matcher::len()` / `Matcher::heap_bytes()` off `ListManager::matcher()`
//! and times its own `refresh_list`/`boot`/`set_user_rules` calls, then pushes
//! the result through [`Metrics::set_ruleset`] (siblings never import each
//! other, ARCHITECTURE.md §Dependency Layering).

use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RulesetSnapshot {
    pub rules: usize,
    pub heap_bytes: usize,
    pub compile_duration: Duration,
}
