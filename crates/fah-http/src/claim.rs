//! The client's claim about where it was going.
//!
//! RouterOS exposes no `SO_ORIGINAL_DST`, so after the router's dst-nat the
//! only statement of the original destination is the `Host` header. This module
//! turns that header into a `(host, port)` pair or rejects it — and nothing
//! more. **It does not decide whether we may connect there**; that is
//! [`fah_common::egress::DestinationPolicy`], which judges the *resolved*
//! address and is shared with Phase 3.
//!
//! The split is deliberate. Parsing a claim is protocol-specific — HTTPS's
//! equivalent reads SNI out of a ClientHello and has nothing in common with
//! this code — while the address decision is identical for both. Forcing them
//! into one function to look shared would put "what a `Host` header is" into
//! L1.

use std::fmt;

use hyper::header::HOST;
use std::fmt::Write;

use hyper::http::uri::{Authority, PathAndQuery};
use hyper::{Request, Uri};

const SOCKET_ADDR_TEXT_MAX: usize = 58;

/// Why a request's stated destination was rejected, before any resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    /// No `Host` at all. HTTP/1.1 requires one; HTTP/1.0 without one leaves us
    /// with no destination to forward to, since there is no `SO_ORIGINAL_DST`
    /// to fall back on.
    Missing,
    /// More than one `Host`. Two headers means two possible destinations, and
    /// picking either is how request smuggling gets its foothold.
    Duplicated,
    /// Present but not a parseable authority.
    Malformed,
    /// A bare IP where policy expects a name. An IP literal cannot be the
    /// result of a normal DNS lookup by a browser, so it is a client naming an
    /// address directly — the shape of a probe rather than of web traffic.
    IpLiteral,
}

impl ClaimError {
    /// Stable, low-cardinality metric label.
    pub fn reason(self) -> &'static str {
        match self {
            ClaimError::Missing => "host_missing",
            ClaimError::Duplicated => "host_duplicated",
            ClaimError::Malformed => "host_malformed",
            ClaimError::IpLiteral => "host_ip_literal",
        }
    }
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

/// A validated destination claim: a hostname and the port it named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub host: String,
    pub port: u16,
}

/// Reads the destination a request claims, without trusting it.
///
/// `default_port` is the port this proxy intercepts — used when `Host` carries
/// no explicit one, which is the ordinary case.
pub fn destination_of<B>(
    request: &Request<B>,
    default_port: u16,
    allow_ip_literals: bool,
) -> Result<Destination, ClaimError> {
    let authority = authority_of(request, default_port)?;
    let host = authority.host();
    if host.is_empty() {
        return Err(ClaimError::Malformed);
    }
    // A bracketed IPv6 literal arrives as "[::1]"; `Authority::host` keeps the
    // brackets, and both the resolver and the policy want it bare.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if !allow_ip_literals && host.parse::<std::net::IpAddr>().is_ok() {
        return Err(ClaimError::IpLiteral);
    }
    Ok(Destination {
        host: host.to_string(),
        port: authority.port_u16().unwrap_or(default_port),
    })
}

/// Prefers the request target's authority when it is absolute (the shape a
/// client sends to an explicit proxy), else the `Host` header (the transparent
/// case this deployment actually sees).
///
/// When both are present they must agree: a request line naming one host and a
/// `Host` header naming another is the classic desync, where we and the origin
/// disagree about which site the response belongs to.
fn authority_of<B>(request: &Request<B>, default_port: u16) -> Result<Authority, ClaimError> {
    let mut headers = request.headers().get_all(HOST).iter();
    let header = headers.next();
    if headers.next().is_some() {
        return Err(ClaimError::Duplicated);
    }
    let header = header
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|text| text.parse::<Authority>().ok())
                .ok_or(ClaimError::Malformed)
        })
        .transpose()?;

    match (request.uri().authority().cloned(), header) {
        (Some(from_uri), Some(from_header)) => {
            if !same_destination(&from_uri, &from_header, default_port) {
                return Err(ClaimError::Malformed);
            }
            Ok(from_uri)
        }
        (Some(from_uri), None) => Ok(from_uri),
        (None, Some(from_header)) => Ok(from_header),
        (None, None) => Err(ClaimError::Missing),
    }
}

/// Do two authorities name the same place?
///
/// Compared by what they mean, not how they are spelled: `example.com` and
/// `example.com:80` are one destination, and hostnames are case-insensitive.
/// A raw string comparison rejected both of those as a desync attempt, which
/// turned a legal absolute-form request into a 400.
fn same_destination(a: &Authority, b: &Authority, default_port: u16) -> bool {
    a.host().eq_ignore_ascii_case(b.host())
        && a.port_u16().unwrap_or(default_port) == b.port_u16().unwrap_or(default_port)
}

/// Rewrites the request target to an origin-form URI aimed at one **already
/// approved literal address**.
///
/// This is what closes the gap between checking and connecting. The connector
/// downstream does no name resolution at all, so there is no window in which a
/// second lookup could return a different address than the one the policy
/// approved — a DNS rebind has nothing left to race. The `Host` header is left
/// untouched, so the origin still sees the name the client asked for
/// (RFC 9110 §7.2).
pub fn retarget(uri: &Uri, address: std::net::SocketAddr) -> Result<Uri, hyper::http::Error> {
    let path_and_query = uri
        .path_and_query()
        .cloned()
        .unwrap_or_else(|| PathAndQuery::from_static("/"));
    let mut authority = String::with_capacity(SOCKET_ADDR_TEXT_MAX);
    let _ = write!(authority, "{address}");
    Uri::builder()
        .scheme("http")
        .authority(authority)
        .path_and_query(path_and_query)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTTP: u16 = 80;

    fn with_host(host: &str) -> Request<()> {
        Request::builder()
            .uri("/index.html")
            .header(HOST, host)
            .body(())
            .unwrap()
    }

    #[test]
    fn a_plain_host_header_names_the_destination() {
        let claim = destination_of(&with_host("example.com"), HTTP, false).unwrap();
        assert_eq!(claim.host, "example.com");
        assert_eq!(claim.port, HTTP);
    }

    #[test]
    fn an_explicit_port_in_host_is_kept() {
        // Kept rather than ignored so the egress policy can refuse it — the
        // proxy must not silently rewrite a client's :8443 into :80.
        let claim = destination_of(&with_host("example.com:8443"), HTTP, false).unwrap();
        assert_eq!(claim.port, 8443);
    }

    #[test]
    fn a_missing_host_is_rejected() {
        let request = Request::builder().uri("/").body(()).unwrap();
        assert_eq!(
            destination_of(&request, HTTP, false),
            Err(ClaimError::Missing)
        );
    }

    /// Two `Host` headers mean two destinations; choosing either is where
    /// request smuggling starts.
    #[test]
    fn duplicate_host_headers_are_rejected() {
        let request = Request::builder()
            .uri("/")
            .header(HOST, "example.com")
            .header(HOST, "evil.example")
            .body(())
            .unwrap();
        assert_eq!(
            destination_of(&request, HTTP, false),
            Err(ClaimError::Duplicated)
        );
    }

    #[test]
    fn an_ip_literal_host_is_rejected_unless_allowed() {
        assert_eq!(
            destination_of(&with_host("192.168.10.1"), HTTP, false),
            Err(ClaimError::IpLiteral)
        );
        // With literals allowed it parses — the address policy still judges it.
        let claim = destination_of(&with_host("192.168.10.1"), HTTP, true).unwrap();
        assert_eq!(claim.host, "192.168.10.1");
    }

    #[test]
    fn a_bracketed_ipv6_literal_loses_its_brackets() {
        let claim = destination_of(&with_host("[::1]:80"), HTTP, true).unwrap();
        assert_eq!(claim.host, "::1", "the resolver and policy want it bare");
        assert_eq!(claim.port, 80);
    }

    /// An absolute request target disagreeing with `Host` is a desync attempt.
    #[test]
    fn an_absolute_target_disagreeing_with_host_is_rejected() {
        let request = Request::builder()
            .uri("http://origin.example/index.html")
            .header(HOST, "other.example")
            .body(())
            .unwrap();
        assert_eq!(
            destination_of(&request, HTTP, false),
            Err(ClaimError::Malformed)
        );
    }

    /// The same destination spelled two ways is not a desync. A raw string
    /// comparison rejected this pair, turning a legal request into a 400.
    #[test]
    fn an_absolute_target_agreeing_with_host_but_spelled_differently_is_accepted() {
        for (target, host) in [
            ("http://origin.example/i", "origin.example:80"),
            ("http://origin.example:80/i", "origin.example"),
            ("http://Origin.Example/i", "origin.example"),
        ] {
            let request = Request::builder()
                .uri(target)
                .header(HOST, host)
                .body(())
                .unwrap();
            let claim = destination_of(&request, HTTP, false)
                .unwrap_or_else(|err| panic!("{target} + {host} rejected as {err}"));
            assert_eq!(claim.port, HTTP);
        }
    }

    /// …but a genuinely different port or host still is.
    #[test]
    fn an_absolute_target_on_another_port_is_still_rejected() {
        let request = Request::builder()
            .uri("http://origin.example:8080/i")
            .header(HOST, "origin.example")
            .body(())
            .unwrap();
        assert_eq!(
            destination_of(&request, HTTP, false),
            Err(ClaimError::Malformed)
        );
    }

    #[test]
    fn an_absolute_target_alone_is_accepted() {
        let request = Request::builder()
            .uri("http://origin.example/index.html")
            .body(())
            .unwrap();
        let claim = destination_of(&request, HTTP, false).unwrap();
        assert_eq!(claim.host, "origin.example");
    }

    #[test]
    fn retarget_points_at_the_literal_address_and_keeps_the_path() {
        let uri: Uri = "/a/b?c=d".parse().unwrap();
        let retargeted = retarget(&uri, "93.184.216.34:80".parse().unwrap()).unwrap();
        assert_eq!(retargeted.authority().unwrap().as_str(), "93.184.216.34:80");
        assert_eq!(retargeted.path_and_query().unwrap().as_str(), "/a/b?c=d");
    }

    #[test]
    fn retarget_brackets_ipv6_and_defaults_an_empty_path() {
        let uri: Uri = "/".parse().unwrap();
        let retargeted = retarget(&uri, "[2606:2800::1]:80".parse().unwrap()).unwrap();
        assert_eq!(
            retargeted.authority().unwrap().as_str(),
            "[2606:2800::1]:80"
        );
        assert_eq!(retargeted.path_and_query().unwrap().as_str(), "/");
    }

    #[test]
    fn every_claim_error_has_a_distinct_stable_label() {
        let all = [
            ClaimError::Missing,
            ClaimError::Duplicated,
            ClaimError::Malformed,
            ClaimError::IpLiteral,
        ];
        let mut labels: Vec<&str> = all.iter().map(|err| err.reason()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }
}
