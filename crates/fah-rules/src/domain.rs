//! Domain-string normalization shared by every rule parser.

use std::sync::Arc;

/// Longest name a rule may anchor to — the maximum length of a DNS name
/// (RFC 1035), and inside the `u8` length a compiled `Record` addresses the
/// arena with.
///
/// Refused here, where the parse counters see it: `MatcherBuilder::add_rule`
/// drops a longer name silently, which would leave it counted in
/// `rules_active_dns` while filtering nothing.
const MAX_DOMAIN_LEN: usize = 253;

/// Normalizes a candidate domain: lowercases and strips a trailing dot.
/// Rejects anything containing whitespace, wildcards, or path separators —
/// those belong to non-DNS rule syntax, not a domain anchor.
pub(crate) fn normalize_domain(candidate: &str) -> Option<Arc<str>> {
    let candidate = candidate.trim().trim_end_matches('.');
    if candidate.is_empty() || candidate.len() > MAX_DOMAIN_LEN {
        return None;
    }
    // Validity and case are decided in one pass: the character scan has to
    // happen anyway, and it already knows whether any byte is uppercase.
    let mut has_uppercase = false;
    for c in candidate.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_') {
            return None;
        }
        has_uppercase |= c.is_ascii_uppercase();
    }
    if candidate.starts_with('-') || candidate.starts_with('.') || candidate.ends_with('-') {
        return None;
    }
    // `to_ascii_lowercase` allocates a `String` that `Arc::from` then copies
    // into a second allocation. Generated blocklists are entirely lowercase
    // (measured: 0 of 1,213,639 domains across oisd/StevenBlack/1Hosts had
    // any uppercase), so skipping the intermediate is the common case, not an
    // edge case. Hand-written lists still take the correct slow path.
    if has_uppercase {
        Some(Arc::from(candidate.to_ascii_lowercase()))
    } else {
        Some(Arc::from(candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_domain_and_lowercases() {
        assert_eq!(
            normalize_domain("Ads.Example.COM").as_deref(),
            Some("ads.example.com")
        );
    }

    #[test]
    fn strips_trailing_dot() {
        assert_eq!(
            normalize_domain("example.com.").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn rejects_wildcard() {
        assert_eq!(normalize_domain("*.example.com"), None);
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(normalize_domain(""), None);
        assert_eq!(normalize_domain("."), None);
    }

    #[test]
    fn rejects_leading_hyphen() {
        assert_eq!(normalize_domain("-example.com"), None);
    }

    /// `MatcherBuilder::add_rule` drops a longer name silently, so refusing it
    /// here is what keeps `rules_active_dns` honest.
    #[test]
    fn rejects_a_name_longer_than_dns_allows() {
        assert!(normalize_domain(&"a".repeat(MAX_DOMAIN_LEN)).is_some());
        assert_eq!(normalize_domain(&"a".repeat(MAX_DOMAIN_LEN + 1)), None);
        // The bound applies to the normalized form, not the raw line.
        assert!(normalize_domain(&format!("{}.", "a".repeat(MAX_DOMAIN_LEN))).is_some());
    }
}
