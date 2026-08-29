//! The single error shape every non-2xx response carries (API.md §Error
//! format): `{ "error": { "code", "message" } }`, where `code` is a stable
//! machine-readable slug.

use axum::http::header::{CACHE_CONTROL, RETRY_AFTER};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// Missing or invalid API key (API.md §Authentication).
    Unauthorized,
    NotFound(String),
    /// A well-formed request whose contents fail validation — the `422` cases
    /// (bad rule lines, a config patch the schema rejects).
    ValidationFailed(String),
    /// A syntactically bad request: unparseable query string, malformed JSON
    /// body, an unknown enum value. `400`, distinct from a `422` whose shape
    /// was fine but whose values were not.
    BadRequest(String),
    Conflict(String),
    RateLimited {
        message: String,
        retry_after: u64,
    },
    Unavailable {
        message: String,
        retry_after: Option<u64>,
    },
    Internal(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "missing or invalid credentials".to_string(),
            ),
            Self::NotFound(what) => (StatusCode::NOT_FOUND, "not_found", what.clone()),
            Self::ValidationFailed(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                message.clone(),
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message.clone()),
            Self::Conflict(message) => (StatusCode::CONFLICT, "conflict", message.clone()),
            Self::RateLimited { message, .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                message.clone(),
            ),
            Self::Unavailable { message, .. } => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                message.clone(),
            ),
            Self::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                message.clone(),
            ),
        }
    }

    fn no_store(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized | Self::RateLimited { .. } | Self::Unavailable { .. }
        )
    }

    fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after, .. } => Some(*retry_after),
            Self::Unavailable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let no_store = self.no_store();
        let retry_after = self.retry_after();
        let (status, code, message) = self.parts();
        let mut response = (
            status,
            Json(ErrorBody {
                error: ErrorDetail { code, message },
            }),
        )
            .into_response();
        if no_store {
            response.headers_mut().insert(CACHE_CONTROL, NO_STORE);
        }
        if let Some(seconds) = retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(RETRY_AFTER, value);
            }
        }
        response
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_maps_to_its_documented_status_and_code() {
        let cases = [
            (
                ApiError::Unauthorized,
                StatusCode::UNAUTHORIZED,
                "unauthorized",
            ),
            (
                ApiError::NotFound("list x".into()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                ApiError::ValidationFailed("line 1: bad".into()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
            ),
            (
                ApiError::BadRequest("bad cursor".into()),
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                ApiError::Conflict("exists".into()),
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                ApiError::RateLimited {
                    message: "too many".into(),
                    retry_after: 42,
                },
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
            (
                ApiError::Unavailable {
                    message: "busy".into(),
                    retry_after: Some(1),
                },
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
            ),
            (
                ApiError::Internal("boom".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
        ];
        for (error, status, code) in cases {
            let (got_status, got_code, _) = error.parts();
            assert_eq!(got_status, status);
            assert_eq!(got_code, code);
        }
    }

    #[test]
    fn the_envelope_carries_no_store_and_retry_after_where_the_contract_says() {
        let cases = [
            (ApiError::Unauthorized, true, None),
            (
                ApiError::RateLimited {
                    message: "too many".into(),
                    retry_after: 42,
                },
                true,
                Some("42"),
            ),
            (
                ApiError::Unavailable {
                    message: "busy".into(),
                    retry_after: Some(1),
                },
                true,
                Some("1"),
            ),
            (
                ApiError::Unavailable {
                    message: "tls off".into(),
                    retry_after: None,
                },
                true,
                None,
            ),
            (ApiError::NotFound("x".into()), false, None),
        ];
        for (error, no_store, retry_after) in cases {
            let response = error.into_response();
            let headers = response.headers();
            assert_eq!(
                headers.get(CACHE_CONTROL).map(|v| v.to_str().unwrap()),
                no_store.then_some("no-store")
            );
            assert_eq!(
                headers.get(RETRY_AFTER).map(|v| v.to_str().unwrap()),
                retry_after
            );
        }
    }
}
