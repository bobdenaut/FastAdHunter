use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::session;
use crate::state::AppState;

const PUBLIC_PATHS: [&str; 2] = ["/health", "/api/v1/auth/login"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Bearer,
    Session,
}

pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if PUBLIC_PATHS.contains(&path) {
        return next.run(request).await;
    }

    let presented = bearer_token(&request).or_else(|| query_token(&request));
    if let Some(token) = presented {
        if state.keys.matches(&token) {
            request.extensions_mut().insert(AuthMethod::Bearer);
            return next.run(request).await;
        }
    }

    if let Some(token) = session_token(&request) {
        if state.auth.verify_session(&token).await {
            request.extensions_mut().insert(AuthMethod::Session);
            return next.run(request).await;
        }
    }

    ApiError::Unauthorized.into_response()
}

fn session_token(request: &Request) -> Option<String> {
    let header = request
        .headers()
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?;
    session::token_from_cookies(header).map(str::to_string)
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

    #[test]
    fn the_session_cookie_is_read_from_the_cookie_header_only() {
        let with_cookie = Request::builder()
            .uri("/api/v1/stats")
            .header(axum::http::header::COOKIE, "a=1; __Host-fah_session=tok")
            .body(Body::empty())
            .unwrap();
        assert_eq!(session_token(&with_cookie).as_deref(), Some("tok"));
        assert_eq!(session_token(&request("/api/v1/stats", None)), None);
    }

    #[test]
    fn login_is_the_only_path_added_to_the_public_set() {
        assert_eq!(PUBLIC_PATHS, ["/health", "/api/v1/auth/login"]);
    }
}
