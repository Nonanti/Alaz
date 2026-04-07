//! Unified API error type for all REST endpoints.
//!
//! Provides consistent JSON error responses:
//! ```json
//! { "error": { "code": "not_found", "message": "episode xyz not found" } }
//! ```

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// API error type that converts to consistent JSON responses.
#[derive(Debug)]
pub enum ApiError {
    /// 404 — entity not found
    NotFound(String),
    /// 400 — invalid input
    BadRequest(String),
    /// 401 — authentication failed
    Unauthorized(String),
    /// 403 — forbidden (e.g. untrusted device)
    Forbidden(String),
    /// 500 — internal server error
    Internal(String),
}

impl ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::NotFound(m)
            | Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::Internal(m) => m,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code(),
                "message": self.message(),
            }
        });
        (self.status_code(), Json(body)).into_response()
    }
}

/// Convert `alaz_core::AlazError` into `ApiError` with proper status codes.
impl From<alaz_core::AlazError> for ApiError {
    fn from(err: alaz_core::AlazError) -> Self {
        match &err {
            alaz_core::AlazError::NotFound(msg) => ApiError::NotFound(msg.clone()),
            alaz_core::AlazError::Validation(msg) => ApiError::BadRequest(msg.clone()),
            alaz_core::AlazError::Duplicate(msg) => ApiError::BadRequest(msg.clone()),
            alaz_core::AlazError::Auth(msg) => ApiError::Unauthorized(msg.clone()),
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

/// Convert `serde_json::Error` into `ApiError`.
impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::Internal(format!("serialization error: {err}"))
    }
}

/// Convert `sqlx::Error` into `ApiError`.
impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::Internal(format!("database error: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn not_found_returns_404() {
        let err = ApiError::NotFound("gone".into());
        assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn bad_request_returns_400() {
        let err = ApiError::BadRequest("invalid".into());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn unauthorized_returns_401() {
        let err = ApiError::Unauthorized("no token".into());
        assert_eq!(err.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn forbidden_returns_403() {
        let err = ApiError::Forbidden("denied".into());
        assert_eq!(err.status_code(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn internal_returns_500() {
        let err = ApiError::Internal("kaboom".into());
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn from_alaz_not_found() {
        let err: ApiError = alaz_core::AlazError::NotFound("x".into()).into();
        assert!(matches!(err, ApiError::NotFound(m) if m == "x"));
    }

    #[test]
    fn from_alaz_validation() {
        let err: ApiError = alaz_core::AlazError::Validation("bad".into()).into();
        assert!(matches!(err, ApiError::BadRequest(m) if m == "bad"));
    }

    #[test]
    fn from_alaz_auth() {
        let err: ApiError = alaz_core::AlazError::Auth("nope".into()).into();
        assert!(matches!(err, ApiError::Unauthorized(m) if m == "nope"));
    }

    #[test]
    fn from_alaz_qdrant() {
        let err: ApiError = alaz_core::AlazError::Qdrant("timeout".into()).into();
        assert!(matches!(err, ApiError::Internal(m) if m.contains("qdrant")));
    }

    #[tokio::test]
    async fn json_response_contains_code_and_message() {
        let err = ApiError::NotFound("item 42 not found".into());
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "not_found");
        assert_eq!(json["error"]["message"], "item 42 not found");
    }
}
