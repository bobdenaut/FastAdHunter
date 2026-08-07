//! Bearer-key authentication (API.md §Authentication, SECURITY.md §API
//! access): required for everything under `/api/v1/`, with `GET /health`
//! exempt.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::state::AppState;

/// Liveness only — status, version, uptime. Every other path requires the key.
const PUBLIC_PATHS: [&str; 1] = ["/health"];

pub async fn require_api_key(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if PUBLIC_PATHS.contains(&path) {
        return next.run(request).await;
    }

    let presented = bearer_token(&request).or_else(|| query_token(&request));
    match presented {
        Some(token) if state.keys.matches(&token) => next.run(request).await,
        _ => ApiError::Unauthorized.into_response(),
    }
}

/// `Authorization: Bearer <key>`. The scheme is compared case-insensitively
/// (RFC 7235 makes it case-insensitive); the key itself is not.
fn bearer_token(request: &Request) -> Option<String> {
    let header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = header.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim().to_string())
}

/// `?token=` on the WebSocket upgrade (API.md §Events: browsers cannot set
/// headers on a `WebSocket` handshake). Restricted to the events route so it
/// never becomes a way to put the key in a REST URL — and therefore in proxy
/// logs and browser history.
fn query_token(request: &Request) -> Option<String> {
    if request.uri().path() != "/api/v1/events" {
        return None;
    }
    let query = request.uri().query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token").then(|| percent_decode(value))
    })
}

/// Minimal percent-decoding for the token parameter — an API key is hex, so
/// this only has to survive a client that encodes anyway.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;

    use super::*;

    fn request(uri: &str, authorization: Option<&str>) -> Request {
        let mut builder = Request::builder().uri(uri);
        if let Some(value) = authorization {
            builder = builder.header(axum::http::header::AUTHORIZATION, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn bearer_scheme_is_case_insensitive_but_the_key_is_not() {
        assert_eq!(
            bearer_token(&request("/api/v1/stats", Some("Bearer abc123"))).as_deref(),
            Some("abc123")
        );
        assert_eq!(
            bearer_token(&request("/api/v1/stats", Some("bearer abc123"))).as_deref(),
            Some("abc123")
        );
        assert_eq!(
            bearer_token(&request("/api/v1/stats", Some("BEARER ABC123"))).as_deref(),
            Some("ABC123")
        );
    }

    #[test]
    fn a_non_bearer_or_missing_header_yields_nothing() {
        assert_eq!(bearer_token(&request("/api/v1/stats", None)), None);
        assert_eq!(
            bearer_token(&request("/api/v1/stats", Some("Basic abc123"))),
            None
        );
        assert_eq!(
            bearer_token(&request("/api/v1/stats", Some("abc123"))),
            None
        );
    }

    #[test]
    fn query_token_is_accepted_only_on_the_events_route() {
        assert_eq!(
            query_token(&request("/api/v1/events?token=abc123", None)).as_deref(),
            Some("abc123")
        );
        assert_eq!(
            query_token(&request("/api/v1/stats?token=abc123", None)),
            None,
            "a REST URL must never carry the key"
        );
        assert_eq!(query_token(&request("/api/v1/events", None)), None);
    }

    #[test]
    fn query_token_survives_other_parameters_and_encoding() {
        assert_eq!(
            query_token(&request("/api/v1/events?x=1&token=abc123&y=2", None)).as_deref(),
            Some("abc123")
        );
        assert_eq!(percent_decode("a%2Bb"), "a+b");
        assert_eq!(percent_decode("plain"), "plain");
    }
}
