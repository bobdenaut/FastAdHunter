//! Bridges wire-level `hickory_proto` types to `fah-model`'s own lightweight
//! [`QueryType`] (ARCHITECTURE.md: "DNS wire format comes from `hickory-proto`;
//! `fah-model` holds our own domain types only" — this module is the seam).

use std::fmt::Write;

use fah_model::QueryType;
use hickory_proto::rr::{Name, RecordType};

/// Maps a wire record type to the Rule Engine's own [`QueryType`] — the type
/// `fah_rules::Matcher::lookup` expects. `A`/`AAAA` get their own variants
/// (the only types with a defined blocked-response synthesis, CONTEXT.md:
/// Blocked Response); everything else is `Other`, matched by the matcher's
/// allocation-free `$dnstype` table walk.
pub(crate) fn to_fah_query_type(record_type: RecordType) -> QueryType {
    match record_type {
        RecordType::A => QueryType::A,
        RecordType::AAAA => QueryType::Aaaa,
        other => QueryType::Other(other.to_string()),
    }
}

/// The domain text the matcher expects: `Name`'s UTF-8 form, trailing dot and
/// all — `fah_rules::Matcher::lookup` strips it internally, so no need to
/// duplicate that here.
pub(crate) fn domain_of(name: &Name) -> String {
    let mut domain = String::with_capacity(name.len());
    let _ = write!(domain, "{name}");
    domain
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn a_and_aaaa_map_to_their_own_variants() {
        assert_eq!(to_fah_query_type(RecordType::A), QueryType::A);
        assert_eq!(to_fah_query_type(RecordType::AAAA), QueryType::Aaaa);
    }

    #[test]
    fn other_types_carry_their_wire_name() {
        assert_eq!(
            to_fah_query_type(RecordType::HTTPS),
            QueryType::Other("HTTPS".to_string())
        );
    }

    #[test]
    fn domain_of_keeps_the_trailing_dot() {
        let name = Name::from_str("ads.example.com.").unwrap();
        assert_eq!(domain_of(&name), "ads.example.com.");
    }

    #[test]
    fn domain_of_matches_to_utf8_byte_for_byte() {
        for text in [
            "ads.example.com.",
            "a-very-long-subdomain-label-here.blocked.example.com.",
            "xn--nxasmq6b.example.",
            "example",
            ".",
        ] {
            let name = Name::from_str(text).unwrap();
            assert_eq!(domain_of(&name), name.to_utf8(), "{text}");
        }
    }

    #[test]
    fn domain_of_sizes_the_string_once_for_ascii_names() {
        for text in [
            "ads.example.com.",
            "a-very-long-subdomain-label-here.blocked.example.com.",
        ] {
            let name = Name::from_str(text).unwrap();
            let domain = domain_of(&name);
            assert_eq!(domain.capacity(), name.len(), "{text}");
            assert_eq!(domain.len(), name.len(), "{text}");
        }
    }
}
