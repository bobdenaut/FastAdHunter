use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

use crate::state::AppState;

pub const DNS_MESSAGE: &str = "application/dns-message";

pub const MAX_MESSAGE_BYTES: usize = u16::MAX as usize;

#[derive(Deserialize)]
pub struct GetParams {
    dns: String,
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(params): Query<GetParams>,
) -> Response {
    let Ok(message) = URL_SAFE_NO_PAD.decode(params.dns.as_bytes()) else {
        return reject(
            StatusCode::BAD_REQUEST,
            "the dns parameter must be unpadded base64url (RFC 8484 section 4.1)",
        );
    };
    answer(&state, message, peer).await
}

pub async fn post(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !carries_dns_message(&headers) {
        return reject(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/dns-message",
        );
    }
    answer(&state, Vec::from(body), peer).await
}

async fn answer(state: &AppState, message: Vec<u8>, peer: SocketAddr) -> Response {
    let Some(doh) = &state.doh else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if message.is_empty() {
        return reject(StatusCode::BAD_REQUEST, "empty DNS message");
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return reject(
            StatusCode::PAYLOAD_TOO_LARGE,
            "a DNS message is at most 65535 bytes",
        );
    }
    match doh.resolve(message, peer.ip()).await {
        Some(reply) => (
            StatusCode::OK,
            [(CONTENT_TYPE, DNS_MESSAGE), (CACHE_CONTROL, "no-store")],
            reply,
        )
            .into_response(),
        None => reject(StatusCode::BAD_REQUEST, "not a DNS query"),
    }
}

fn carries_dns_message(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(DNS_MESSAGE))
}

fn reject(status: StatusCode, reason: &'static str) -> Response {
    (status, [(CACHE_CONTROL, "no-store")], reason).into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(content_type: Option<&'static str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn only_the_dns_message_media_type_is_accepted() {
        assert!(carries_dns_message(&headers(Some(
            "application/dns-message"
        ))));
        assert!(carries_dns_message(&headers(Some(
            "Application/DNS-Message; charset=utf-8"
        ))));
        assert!(!carries_dns_message(&headers(Some("application/json"))));
        assert!(!carries_dns_message(&headers(Some("application/dns-json"))));
        assert!(!carries_dns_message(&headers(None)));
    }

    #[test]
    fn padded_base64url_is_rejected_as_the_rfc_requires() {
        let unpadded = URL_SAFE_NO_PAD.encode([0xAB, 0xCD, 0x12, 0x34, 0x00]);
        assert!(URL_SAFE_NO_PAD.decode(unpadded.as_bytes()).is_ok());
        assert!(URL_SAFE_NO_PAD
            .decode(format!("{unpadded}=").as_bytes())
            .is_err());
        assert!(URL_SAFE_NO_PAD.decode(b"not base64!").is_err());
    }
}
