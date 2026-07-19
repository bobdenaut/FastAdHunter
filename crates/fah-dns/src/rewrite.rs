//! Interprets a matched rule's raw `$dnsrewrite` payload. `fah-rules` parses
//! and stores this value verbatim without interpreting it (see
//! `fah_rules::DomainRule::dns_rewrite`'s doc comment and
//! `fah_rules::Matcher::rewrite`'s: "used by fah-dns for answer synthesis" —
//! this module is that synthesis step.
//!
//! Supported AdGuard `$dnsrewrite` forms (the ones the existing rule-parser
//! test corpus exercises and the ones expressible without an upstream
//! resolve, which doesn't exist until p1-06):
//!
//! - A bare IP literal (`$dnsrewrite=0.0.0.0`) — answer with that address,
//!   for the matching query type only (an IPv4 literal answers `A`, an IPv6
//!   literal answers `AAAA`; the other query type falls through to the
//!   standard blocked-response synthesis, matching AdGuard's own behavior of
//!   not fabricating an address in the family the rule didn't specify).
//! - A bare RCODE keyword (`NOERROR` | `NXDOMAIN` | `REFUSED`, case
//!   insensitive) — that response code, no answer records.
//! - The full form `RCODE;TYPE;VALUE` (e.g. `NOERROR;A;1.2.3.4`) where
//!   `TYPE` is `A` or `AAAA` and `VALUE` is a matching IP literal.
//!
//! Anything else (CNAME targets, TXT payloads, unparseable text) falls
//! through to the standard block synthesis — never a rejected rule, never a
//! panic, consistent with RULE_ENGINE.md's "never reject" philosophy.

use std::net::{Ipv4Addr, Ipv6Addr};

use hickory_proto::op::ResponseCode;
use hickory_proto::rr::rdata::{A, AAAA};

/// What a `$dnsrewrite` payload resolves to for one query.
pub(crate) enum RewriteOutcome {
    /// Answer with this literal address (already checked against the
    /// query's type).
    Address(RewriteAddress),
    /// Respond with this code and no answer records.
    Code(ResponseCode),
    /// Not a form this module understands (or it doesn't apply to the
    /// query's type) — caller falls back to standard block synthesis.
    Unhandled,
}

pub(crate) enum RewriteAddress {
    V4(A),
    V6(AAAA),
}

/// Interprets `raw` for a query of type `qtype_is_a`/`qtype_is_aaaa`
/// (mutually exclusive with "neither" for non-address query types, which
/// always fall through).
pub(crate) fn interpret(raw: &str, qtype_is_a: bool, qtype_is_aaaa: bool) -> RewriteOutcome {
    let raw = raw.trim();

    if let Some(outcome) = parse_bare_code(raw) {
        return outcome;
    }
    if let Some(outcome) = parse_address(raw, qtype_is_a, qtype_is_aaaa) {
        return outcome;
    }
    if let Some((rcode, value)) = raw.split_once(';').and_then(|(rcode, rest)| {
        rest.split_once(';')
            .map(|(rtype, value)| (rcode, rtype, value))
            .filter(|(_, rtype, _)| {
                rtype.eq_ignore_ascii_case("a") || rtype.eq_ignore_ascii_case("aaaa")
            })
            .map(|(rcode, _, value)| (rcode, value))
    }) {
        let code = if rcode.trim().is_empty() {
            Some(ResponseCode::NoError)
        } else {
            bare_code(rcode.trim())
        };
        if code == Some(ResponseCode::NoError) {
            if let Some(outcome) = parse_address(value.trim(), qtype_is_a, qtype_is_aaaa) {
                return outcome;
            }
        }
    }

    RewriteOutcome::Unhandled
}

fn parse_bare_code(raw: &str) -> Option<RewriteOutcome> {
    bare_code(raw).map(RewriteOutcome::Code)
}

fn bare_code(raw: &str) -> Option<ResponseCode> {
    match raw.to_ascii_uppercase().as_str() {
        "NOERROR" => Some(ResponseCode::NoError),
        "NXDOMAIN" => Some(ResponseCode::NXDomain),
        "REFUSED" => Some(ResponseCode::Refused),
        _ => None,
    }
}

fn parse_address(raw: &str, qtype_is_a: bool, qtype_is_aaaa: bool) -> Option<RewriteOutcome> {
    if qtype_is_a {
        if let Ok(addr) = raw.parse::<Ipv4Addr>() {
            return Some(RewriteOutcome::Address(RewriteAddress::V4(A(addr))));
        }
    }
    if qtype_is_aaaa {
        if let Ok(addr) = raw.parse::<Ipv6Addr>() {
            return Some(RewriteOutcome::Address(RewriteAddress::V6(AAAA(addr))));
        }
    }
    // A literal of the *other* family for this query type is a defined,
    // deliberate "no answer for this type" outcome, not an unhandled form.
    if raw.parse::<Ipv4Addr>().is_ok() || raw.parse::<Ipv6Addr>().is_ok() {
        return Some(RewriteOutcome::Unhandled);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ipv4_answers_a_query() {
        let outcome = interpret("0.0.0.0", true, false);
        assert!(matches!(
            outcome,
            RewriteOutcome::Address(RewriteAddress::V4(A(addr))) if addr == Ipv4Addr::new(0, 0, 0, 0)
        ));
    }

    #[test]
    fn bare_ipv4_does_not_answer_aaaa_query() {
        let outcome = interpret("0.0.0.0", false, true);
        assert!(matches!(outcome, RewriteOutcome::Unhandled));
    }

    #[test]
    fn bare_nxdomain_carries_no_address() {
        let outcome = interpret("NXDOMAIN", true, false);
        assert!(matches!(
            outcome,
            RewriteOutcome::Code(ResponseCode::NXDomain)
        ));
    }

    #[test]
    fn full_form_noerror_a_answers_with_the_address() {
        let outcome = interpret("NOERROR;A;1.2.3.4", true, false);
        assert!(matches!(
            outcome,
            RewriteOutcome::Address(RewriteAddress::V4(A(addr))) if addr == Ipv4Addr::new(1, 2, 3, 4)
        ));
    }

    #[test]
    fn unparseable_payload_is_unhandled() {
        let outcome = interpret("sinkhole.example.com", true, false);
        assert!(matches!(outcome, RewriteOutcome::Unhandled));
    }
}
