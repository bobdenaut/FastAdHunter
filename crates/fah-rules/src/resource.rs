//! Resource-type options as a bitmask (RULE_ENGINE.md: HTTP matching).
//!
//! `$script,~image` restricts a rule to a set of request types. Storing that
//! set as a `u16` mask keeps a compiled URL rule's type test to one `AND` on
//! the hot path — no list walk, no allocation — and folds negation away at
//! compile time exactly as `$dnstype=~A` already does for DNS.

use fah_model::ResourceType;

/// Bit index per type. The table position *is* the bit, so the two directions
/// (option name -> bit, request type -> bit) cannot drift apart.
const TYPES: [(ResourceType, &[&str]); 12] = [
    (ResourceType::Document, &["document", "doc"]),
    (ResourceType::Subdocument, &["subdocument", "frame"]),
    (ResourceType::Script, &["script"]),
    (ResourceType::Stylesheet, &["stylesheet", "css"]),
    (ResourceType::Image, &["image", "img"]),
    (ResourceType::Font, &["font"]),
    (ResourceType::Media, &["media"]),
    (ResourceType::XmlHttpRequest, &["xmlhttprequest", "xhr"]),
    (ResourceType::WebSocket, &["websocket"]),
    (ResourceType::Ping, &["ping", "beacon"]),
    (ResourceType::Object, &["object"]),
    (ResourceType::Other, &["other"]),
];

/// Every bit [`bit_for_option`] can produce — what a purely negated set
/// (`~script,~image`) subtracts from.
pub(crate) const ALL_TYPES: u16 = (1 << 12) - 1;

/// The bit for one `$option` name, or `None` if the name is not a resource
/// type at all (the caller then decides whether it is another known option or
/// an unsupported one).
pub(crate) fn bit_for_option(name: &str) -> Option<u16> {
    TYPES.iter().enumerate().find_map(|(index, (_, names))| {
        names
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
            .then_some(1 << index)
    })
}

/// The bit for a request's actual type. [`ResourceType::Unknown`] carries no
/// bit: an undetermined type matches neither `$script` nor `~script`, so it
/// can never be over-blocked by a type-restricted rule.
pub(crate) fn bit_for_request(kind: ResourceType) -> u16 {
    if kind == ResourceType::Unknown {
        return 0;
    }
    TYPES
        .iter()
        .position(|(candidate, _)| *candidate == kind)
        .map_or(0, |index| 1 << index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_has_a_matching_option_name() {
        for (kind, names) in TYPES {
            assert_eq!(
                bit_for_option(names[0]),
                Some(bit_for_request(kind)),
                "{kind:?} option name and request bit disagree"
            );
        }
    }

    #[test]
    fn option_names_are_case_insensitive_and_aliased() {
        assert_eq!(bit_for_option("XHR"), bit_for_option("xmlhttprequest"));
        assert_eq!(bit_for_option("css"), bit_for_option("stylesheet"));
        assert_eq!(bit_for_option("Beacon"), bit_for_option("ping"));
    }

    #[test]
    fn an_unknown_option_is_not_a_resource_type() {
        assert_eq!(bit_for_option("third-party"), None);
        assert_eq!(bit_for_option("popup"), None);
    }

    #[test]
    fn an_undetermined_request_type_carries_no_bit() {
        assert_eq!(bit_for_request(ResourceType::Unknown), 0);
        assert_ne!(bit_for_request(ResourceType::Script), 0);
    }

    #[test]
    fn all_types_covers_every_bit_and_nothing_more() {
        let union = TYPES
            .iter()
            .map(|(kind, _)| bit_for_request(*kind))
            .fold(0, |acc, bit| acc | bit);
        assert_eq!(union, ALL_TYPES);
    }
}
