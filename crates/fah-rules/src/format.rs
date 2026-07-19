//! Rule-list format detection (RULE_ENGINE.md: Supported formats).
//!
//! Format is detected once per list, from its first non-comment line — not
//! per line.

/// One of the four rule-list formats RULE_ENGINE.md commits to supporting.
/// EasyList, uBlock Origin and AdGuard share one syntax family and therefore
/// one parser (`Adblock`); AdGuard's DNS-specific options (`$dnstype`,
/// `$dnsrewrite`, `$client`) are recognized within it, not a separate format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFormat {
    Hosts,
    PlainDomainList,
    Adblock,
}

/// Detects a rule list's format from its first content line (comments and
/// blank lines skipped). An all-comment/empty list defaults to
/// `PlainDomainList` — it parses to zero rules either way.
pub fn detect_format(text: &str) -> RuleFormat {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with('!')
            || line.starts_with('[')
        {
            continue;
        }
        return classify_line(line);
    }
    RuleFormat::PlainDomainList
}

fn classify_line(line: &str) -> RuleFormat {
    let mut tokens = line.split_whitespace();
    if let Some(first) = tokens.next() {
        if tokens.next().is_some() && looks_like_ip(first) {
            return RuleFormat::Hosts;
        }
    }
    if line.starts_with("||")
        || line.starts_with("@@")
        || line.contains("##")
        || line.contains("#@#")
        || line.contains("#?#")
        || line.contains("#$#")
    {
        return RuleFormat::Adblock;
    }
    RuleFormat::PlainDomainList
}

/// Loose IPv4/IPv6 shape check for hosts-line detection — not full validation.
pub(crate) fn looks_like_ip(token: &str) -> bool {
    if token.contains('.') && !token.contains(':') {
        let parts: Vec<&str> = token.split('.').collect();
        parts.len() >= 2
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    } else if token.contains(':') {
        token.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hosts_from_ip_prefixed_line() {
        assert_eq!(
            detect_format("# comment\n0.0.0.0 ads.example.com\n"),
            RuleFormat::Hosts
        );
    }

    #[test]
    fn detects_plain_domain_list() {
        assert_eq!(
            detect_format("! not a hosts comment\nads.example.com\n"),
            RuleFormat::PlainDomainList
        );
    }

    #[test]
    fn detects_adblock_from_domain_anchor() {
        assert_eq!(
            detect_format("! Title: x\n||ads.example.com^\n"),
            RuleFormat::Adblock
        );
    }

    #[test]
    fn detects_adblock_from_cosmetic_marker() {
        assert_eq!(
            detect_format("example.com##.ad-banner\n"),
            RuleFormat::Adblock
        );
    }

    #[test]
    fn all_comments_defaults_to_plain_domain_list() {
        assert_eq!(
            detect_format("# only comments\n! and more\n"),
            RuleFormat::PlainDomainList
        );
    }
}
