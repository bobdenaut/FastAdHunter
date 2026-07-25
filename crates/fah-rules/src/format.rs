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

/// How many content lines [`detect_format`] weighs before deciding. Detection
/// runs once per refresh over lists that reach tens of MB, so it must not scan
/// the whole file; a couple of hundred lines is far more than enough to tell
/// three formats apart, and scanning stops as soon as the sample is full.
const SAMPLE_LINES: usize = 200;

/// Detects a rule list's format by **sampling** its content lines (comments and
/// blank lines skipped) and taking the format most of them look like. An
/// all-comment/empty list defaults to `PlainDomainList` — it parses to zero
/// rules either way.
///
/// Sampling rather than reading line one is not defensive coding, it is the
/// only thing that works on real lists. EasyList's first content line is
/// `&rb=&uuid=$third-party` and EasyPrivacy's is
/// `&&sub19=undefined&sub20=undefined` — URL-substring patterns carrying none
/// of the `||`/`@@`/`##` markers a first-line check looks for. Both used to
/// fall through to `PlainDomainList`, which handed the whole of EasyList to the
/// bare-domain parser: 83 junk rules and 69,514 parse errors.
///
/// A tie resolves to `PlainDomainList`. Misreading a domain list as `Adblock`
/// is the worse failure of the two — every bare-domain line becomes an inactive
/// URL pattern, so the list silently contributes nothing, with no parse errors
/// to notice.
pub fn detect_format(text: &str) -> RuleFormat {
    let (mut hosts, mut adblock, mut domains) = (0usize, 0usize, 0usize);
    let mut sampled = 0usize;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with('!')
            || line.starts_with('[')
        {
            continue;
        }
        match classify_line(line) {
            Some(RuleFormat::Hosts) => hosts += 1,
            Some(RuleFormat::Adblock) => adblock += 1,
            Some(RuleFormat::PlainDomainList) => domains += 1,
            // Recognized by no format — counted in the sample so a file of
            // noise cannot be decided by a single stray line, but voting for
            // nothing.
            None => {}
        }
        sampled += 1;
        if sampled >= SAMPLE_LINES {
            break;
        }
    }

    if hosts > adblock && hosts > domains {
        RuleFormat::Hosts
    } else if adblock > domains && adblock > hosts {
        RuleFormat::Adblock
    } else {
        RuleFormat::PlainDomainList
    }
}

/// Classifies one content line, or `None` when it fits no format.
///
/// The adblock markers are chosen to be ones a hosts entry or a bare domain
/// can never contain — `^`, `/`, a `$option` suffix, an `|` address anchor —
/// so a domain list is never mistaken for an adblock list. `*` is deliberately
/// **not** a marker: plain domain lists in the wild carry `*.example.com`
/// entries, and treating those as adblock syntax would silently deactivate the
/// list.
fn classify_line(line: &str) -> Option<RuleFormat> {
    let mut tokens = line.split_whitespace();
    if let Some(first) = tokens.next() {
        if tokens.next().is_some() && looks_like_ip(first) {
            return Some(RuleFormat::Hosts);
        }
    }
    if looks_like_adblock(line) {
        return Some(RuleFormat::Adblock);
    }
    if looks_like_bare_domain(line) {
        return Some(RuleFormat::PlainDomainList);
    }
    None
}

fn looks_like_adblock(line: &str) -> bool {
    if line.starts_with("||")
        || line.starts_with("@@")
        || line.starts_with('|')
        || line.ends_with('|')
        || line.contains("##")
        || line.contains("#@#")
        || line.contains("#?#")
        || line.contains("#$#")
        || line.contains('^')
        || line.contains('/')
    {
        return true;
    }
    // A `$` introducing an option name (`$third-party`, `$~stylesheet`) — the
    // signal that carries EasyList's leading URL-substring rules.
    line.split('$')
        .skip(1)
        .any(|option| option.starts_with(|c: char| c.is_ascii_alphabetic() || c == '~'))
}

/// One token made only of characters a bare domain line may contain. Kept
/// deliberately loose — this decides a *format*, not a rule's validity; the
/// plain-domain parser still applies `normalize_domain` per line.
fn looks_like_bare_domain(line: &str) -> bool {
    !line.is_empty()
        && !line.contains(char::is_whitespace)
        && line
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
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

    /// The shape of real EasyList: the leading content lines are URL-substring
    /// patterns carrying none of `||`, `@@` or `##`. Deciding from line one
    /// classified the whole list as bare domains.
    #[test]
    fn url_substring_patterns_are_adblock_even_without_an_anchor() {
        let text = "! Title: EasyList\n\
                    &rb=&uuid=$third-party\n\
                    &subaffid=%$subdocument,third-party\n\
                    -ad-manager/$~stylesheet\n\
                    -ad.jpg.pagespeed.$image\n";
        assert_eq!(detect_format(text), RuleFormat::Adblock);
    }

    /// The failure that matters in the other direction: one stray adblock line
    /// must not condemn a domain list to the adblock parser, where every bare
    /// domain would become an inactive URL pattern and the list would silently
    /// contribute nothing.
    #[test]
    fn a_single_stray_adblock_line_does_not_flip_a_domain_list() {
        let mut text = String::from("||stray.example.com^\n");
        for i in 0..50 {
            text.push_str(&format!("ads{i}.example.com\n"));
        }
        assert_eq!(detect_format(&text), RuleFormat::PlainDomainList);
    }

    /// Wildcard entries appear in real plain domain lists, so `*` is not an
    /// adblock marker.
    #[test]
    fn wildcard_domain_entries_stay_a_plain_domain_list() {
        let text = "*.ads.example.com\ntracker.example.net\nads.example.org\n";
        assert_eq!(detect_format(text), RuleFormat::PlainDomainList);
    }

    /// A hosts file keeps winning even though its host tokens would each pass
    /// as a bare domain on their own.
    #[test]
    fn hosts_entries_outvote_their_own_domain_tokens() {
        let text = "0.0.0.0 a.example.com\n0.0.0.0 b.example.com\n127.0.0.1 c.example.com\n";
        assert_eq!(detect_format(text), RuleFormat::Hosts);
    }

    /// Detection must not read the whole file: a decisive sample followed by
    /// megabytes of anything else still decides on the sample.
    #[test]
    fn detection_stops_after_the_sample() {
        let mut text = String::new();
        for i in 0..SAMPLE_LINES {
            text.push_str(&format!("||ads{i}.example.com^\n"));
        }
        for i in 0..10_000 {
            text.push_str(&format!("plain{i}.example.com\n"));
        }
        assert_eq!(detect_format(&text), RuleFormat::Adblock);
    }
}
