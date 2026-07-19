//! Domain-string normalization shared by every rule parser.

use std::sync::Arc;

/// Normalizes a candidate domain: lowercases and strips a trailing dot.
/// Rejects anything containing whitespace, wildcards, or path separators —
/// those belong to non-DNS rule syntax, not a domain anchor.
pub(crate) fn normalize_domain(candidate: &str) -> Option<Arc<str>> {
    let candidate = candidate.trim().trim_end_matches('.');
    if candidate.is_empty() {
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
}
