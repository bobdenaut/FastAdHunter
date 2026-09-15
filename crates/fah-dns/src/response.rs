//! Builds the synthesized DNS answers the pipeline sends: blocked responses
//! (CONTEXT.md: Blocked Response), error responses for malformed/unsupported
//! requests, and the UDP truncation fallback (ARCHITECTURE.md §Listeners).

use std::net::{Ipv4Addr, Ipv6Addr};

use hickory_proto::op::{Edns, Message, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA};
use hickory_proto::rr::{RData, Record};
use hickory_proto::ProtoError;

use crate::cache::CachedAnswer;
use crate::rewrite::{RewriteAddress, RewriteOutcome};

/// Default UDP payload size when a request carries no EDNS OPT record
/// ([RFC 1035] historical default, mirrored by every resolver).
const NO_EDNS_UDP_PAYLOAD: u16 = 512;

pub(crate) struct Encoded {
    pub(crate) bytes: Vec<u8>,
    pub(crate) failure: Option<ProtoError>,
}

/// Empty skeleton response: id/op_code mirrored, `RD` echoed, `RA` set (we
/// answer authoritatively for blocked queries and relay for the rest), EDNS
/// echoed with the DO bit passed through untouched (ARCHITECTURE.md: DNSSEC
/// pass-through) but no `AD` bit — we're not vouching for authenticity of a
/// locally synthesized answer.
fn skeleton(request: &Message, response_code: ResponseCode) -> Message {
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.response_code = response_code;
    response.metadata.recursion_desired = request.metadata.recursion_desired;
    response.metadata.recursion_available = true;
    response.metadata.authentic_data = false;

    if let Some(request_edns) = &request.edns {
        let mut edns = Edns::new();
        edns.set_max_payload(request_edns.max_payload().max(NO_EDNS_UDP_PAYLOAD));
        edns.set_dnssec_ok(request_edns.flags().dnssec_ok);
        response.set_edns(edns);
    }

    response
}

/// A request with no question, or an op-code we don't handle — answered with
/// `FORMERR`/`NOTIMP` and no question section (we have none to echo).
pub(crate) fn error(request: &Message, response_code: ResponseCode) -> Message {
    skeleton(request, response_code)
}

/// The blocked-query answer. `A`/`AAAA` get the null-IP synthesis
/// (CONTEXT.md); any other query type under a block verdict gets an empty
/// `NOERROR` (no data for this type at this name) rather than `NXDOMAIN` —
/// answering "no such domain" would be a stronger, unjustified claim for a
/// type we were never asked to reason about (mirrors the matcher's own
/// conservative-unknown-type stance).
///
/// A `$dnsrewrite` payload, if present, is tried first via [`crate::rewrite`]
/// and takes priority; an unhandled/inapplicable payload falls back to this
/// same null-IP/empty synthesis.
pub(crate) fn blocked(
    request: &Message,
    query: &Query,
    ttl: u32,
    dns_rewrite: Option<&str>,
) -> Message {
    let is_a = query.query_type() == hickory_proto::rr::RecordType::A;
    let is_aaaa = query.query_type() == hickory_proto::rr::RecordType::AAAA;

    if let Some(raw) = dns_rewrite {
        match crate::rewrite::interpret(raw, is_a, is_aaaa) {
            RewriteOutcome::Code(code) => {
                let mut response = skeleton(request, code);
                response.add_query(query.clone());
                return response;
            }
            RewriteOutcome::Address(address) => {
                let mut response = skeleton(request, ResponseCode::NoError);
                response.add_query(query.clone());
                let record = match address {
                    RewriteAddress::V4(a) => {
                        Record::from_rdata(query.name().clone(), ttl, RData::A(a))
                    }
                    RewriteAddress::V6(a) => {
                        Record::from_rdata(query.name().clone(), ttl, RData::AAAA(a))
                    }
                };
                response.add_answer(record);
                return response;
            }
            RewriteOutcome::Unhandled => {} // fall through to null-IP/empty synthesis below
        }
    }

    let mut response = skeleton(request, ResponseCode::NoError);
    response.add_query(query.clone());
    if is_a {
        response.add_answer(Record::from_rdata(
            query.name().clone(),
            ttl,
            RData::A(A(Ipv4Addr::UNSPECIFIED)),
        ));
    } else if is_aaaa {
        response.add_answer(Record::from_rdata(
            query.name().clone(),
            ttl,
            RData::AAAA(AAAA(Ipv6Addr::UNSPECIFIED)),
        ));
    }
    response
}

/// Replays a cached answer (p1-05): same records and response code the
/// upstream sent, `ttl` stamped on every record — the caller has already
/// computed either the remaining freshness window or, for a stale-served
/// reply (RFC 8767), the short retry TTL. A negative answer's stored SOA
/// comes back in the authority section (RFC 2308: that's what lets a
/// downstream cacher negative-cache our reply).
pub(crate) fn from_cache(
    request: &Message,
    query: &Query,
    answer: &CachedAnswer,
    ttl: u32,
) -> Message {
    let mut response = skeleton(request, answer.response_code);
    response.add_query(query.clone());
    for record in &answer.records {
        let mut record = record.clone();
        record.ttl = ttl;
        response.add_answer(record);
    }
    for record in &answer.authorities {
        let mut record = record.clone();
        record.ttl = ttl;
        response.add_authority(record);
    }
    response
}

/// The UDP payload budget for `request`: its EDNS max payload, or the
/// historical 512-byte default when it carries no OPT record. An advertised
/// value below 512 is clamped up per RFC 6891 §6.2.3 ("values lower than 512
/// MUST be treated as equal to 512") — otherwise a client advertising a tiny
/// payload would force needless truncation of every answer.
pub(crate) fn max_udp_payload(request: &Message) -> u16 {
    request.edns.as_ref().map_or(NO_EDNS_UDP_PAYLOAD, |edns| {
        edns.max_payload().max(NO_EDNS_UDP_PAYLOAD)
    })
}

/// Encodes `message`, truncating it (RFC 1035 §4.1.1 `TC` bit: header +
/// question only, answers dropped) if it would exceed `budget` over UDP.
/// TCP callers pass `u16::MAX` so truncation never triggers.
pub(crate) fn encode_for_transport(message: &Message, budget: u16) -> Encoded {
    let encoded = encode(message);
    if encoded.bytes.len() <= budget as usize {
        return encoded;
    }
    let truncated = encode(&message.truncate());
    Encoded {
        bytes: truncated.bytes,
        failure: encoded.failure.or(truncated.failure),
    }
}

/// Encodes a message to wire bytes. Falls back to a bare `SERVFAIL` (which
/// cannot itself fail to encode: no question, no records) on the
/// near-impossible chance the real message doesn't fit its own encoding
/// constraints — callers must never propagate a panic from a malformed
/// upstream answer or oversized synthesized name.
fn encode(message: &Message) -> Encoded {
    match message.to_vec() {
        Ok(bytes) => Encoded {
            bytes,
            failure: None,
        },
        Err(err) => Encoded {
            bytes: Message::error_msg(
                message.metadata.id,
                message.metadata.op_code,
                ResponseCode::ServFail,
            )
            .to_vec()
            .unwrap_or_default(),
            failure: Some(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hickory_proto::rr::{Name, RecordType};

    use super::*;

    fn request_with_query(query_type: RecordType) -> (Message, Query) {
        let mut request = Message::query();
        let query = Query::query(Name::from_str("ads.example.com.").unwrap(), query_type);
        request.add_query(query.clone());
        (request, query)
    }

    #[test]
    fn blocked_a_query_answers_null_ipv4() {
        let (request, query) = request_with_query(RecordType::A);
        let response = blocked(&request, &query, 10, None);
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert_eq!(response.answers.len(), 1);
        assert_eq!(response.answers[0].ttl, 10);
        assert!(
            matches!(response.answers[0].data, RData::A(A(addr)) if addr == Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn blocked_aaaa_query_answers_null_ipv6() {
        let (request, query) = request_with_query(RecordType::AAAA);
        let response = blocked(&request, &query, 10, None);
        assert!(
            matches!(response.answers[0].data, RData::AAAA(AAAA(addr)) if addr == Ipv6Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn blocked_other_query_type_answers_empty_noerror() {
        let (request, query) = request_with_query(RecordType::TXT);
        let response = blocked(&request, &query, 10, None);
        assert_eq!(response.metadata.response_code, ResponseCode::NoError);
        assert!(response.answers.is_empty());
    }

    #[test]
    fn dnsrewrite_address_overrides_null_ip() {
        let (request, query) = request_with_query(RecordType::A);
        let response = blocked(&request, &query, 10, Some("1.2.3.4"));
        assert!(
            matches!(response.answers[0].data, RData::A(A(addr)) if addr == Ipv4Addr::new(1, 2, 3, 4))
        );
    }

    #[test]
    fn dnsrewrite_nxdomain_carries_response_code_and_no_answers() {
        let (request, query) = request_with_query(RecordType::A);
        let response = blocked(&request, &query, 10, Some("NXDOMAIN"));
        assert_eq!(response.metadata.response_code, ResponseCode::NXDomain);
        assert!(response.answers.is_empty());
    }

    #[test]
    fn unhandled_dnsrewrite_falls_back_to_null_ip() {
        let (request, query) = request_with_query(RecordType::A);
        let response = blocked(&request, &query, 10, Some("sinkhole.example.com"));
        assert!(
            matches!(response.answers[0].data, RData::A(A(addr)) if addr == Ipv4Addr::UNSPECIFIED)
        );
    }

    #[test]
    fn edns_do_bit_is_echoed_without_setting_authentic_data() {
        let (mut request, query) = request_with_query(RecordType::A);
        let mut edns = Edns::new();
        edns.set_dnssec_ok(true);
        edns.set_max_payload(4096);
        request.set_edns(edns);

        let response = blocked(&request, &query, 10, None);
        assert!(response.edns.unwrap().flags().dnssec_ok);
        assert!(!response.metadata.authentic_data);
    }

    #[test]
    fn oversized_response_is_truncated_under_budget() {
        let (request, query) = request_with_query(RecordType::A);
        let mut response = blocked(&request, &query, 10, None);
        for i in 0..100 {
            response.add_answer(Record::from_rdata(
                Name::from_str(&format!("padding{i}.ads.example.com.")).unwrap(),
                10,
                RData::A(A(Ipv4Addr::UNSPECIFIED)),
            ));
        }
        let bytes = encode_for_transport(&response, 64).bytes;
        assert!(bytes.len() <= 64 || bytes.len() < encode(&response).bytes.len());
        let decoded = Message::from_vec(&bytes).unwrap();
        assert!(decoded.metadata.truncation);
        assert!(decoded.answers.is_empty());
    }

    #[test]
    fn a_sub_512_edns_payload_is_clamped_up_per_rfc_6891() {
        let (mut request, _query) = request_with_query(RecordType::A);
        let mut edns = Edns::new();
        edns.set_max_payload(100);
        request.set_edns(edns);
        assert_eq!(max_udp_payload(&request), 512);
    }

    #[test]
    fn cached_negative_replay_keeps_its_soa_with_the_stamped_ttl() {
        use hickory_proto::rr::rdata::SOA;

        let (request, query) = request_with_query(RecordType::A);
        let answer = CachedAnswer {
            records: Vec::new(),
            authorities: vec![Record::from_rdata(
                Name::from_str("example.com.").unwrap(),
                3600,
                RData::SOA(SOA::new(
                    Name::from_str("ns1.example.com.").unwrap(),
                    Name::from_str("admin.example.com.").unwrap(),
                    1,
                    7200,
                    3600,
                    1209600,
                    45,
                )),
            )],
            response_code: ResponseCode::NXDomain,
        };

        let response = from_cache(&request, &query, &answer, 45);
        assert_eq!(response.metadata.response_code, ResponseCode::NXDomain);
        assert!(response.answers.is_empty());
        assert_eq!(response.authorities.len(), 1);
        assert_eq!(response.authorities[0].ttl, 45);
    }

    #[test]
    fn tcp_budget_never_truncates() {
        let (request, query) = request_with_query(RecordType::A);
        let response = blocked(&request, &query, 10, None);
        let bytes = encode_for_transport(&response, u16::MAX).bytes;
        let decoded = Message::from_vec(&bytes).unwrap();
        assert!(!decoded.metadata.truncation);
        assert_eq!(decoded.answers.len(), 1);
    }
}
