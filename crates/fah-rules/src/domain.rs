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
    let valid_chars = candidate
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_');
    if !valid_chars
        || candidate.starts_with('-')
        || candidate.starts_with('.')
        || candidate.ends_with('-')
    {
        return None;
    }
    Some(Arc::from(candidate.to_ascii_lowercase()))
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
