//! The single error shape every non-2xx response carries (API.md §Error
//! format): `{ "error": { "code", "message" } }`, where `code` is a stable
//! machine-readable slug.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

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
    Internal(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "missing or invalid API key".to_string(),
            ),
            Self::NotFound(what) => (StatusCode::NOT_FOUND, "not_found", what.clone()),
            Self::ValidationFailed(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                message.clone(),
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message.clone()),
            Self::Conflict(message) => (StatusCode::CONFLICT, "conflict", message.clone()),
            Self::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                message.clone(),
            ),
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
        let (status, code, message) = self.parts();
        (
            status,
            Json(ErrorBody {
                error: ErrorDetail { code, message },
            }),
        )
            .into_response()
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
}
