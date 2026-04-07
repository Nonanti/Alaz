use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use sqlx::PgPool;
use tracing::debug;

use crate::{apikey, jwt};

/// Authenticated user extracted from the request.
///
/// Checks in order:
/// 1. `Authorization: Bearer <jwt>` header
/// 2. `X-API-Key: <key>` header
/// 3. If neither, returns 401.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub owner_id: String,
}

/// Rejection type for auth failures.
pub struct AuthRejection {
    message: String,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, self.message).into_response()
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Try Bearer token first
        if let Some(auth_header) = parts.headers.get("authorization")
            && let Ok(value) = auth_header.to_str()
            && let Some(token) = value.strip_prefix("Bearer ")
        {
            let jwt_secret = match parts.extensions.get::<JwtSecret>() {
                Some(s) if !s.0.is_empty() => s.0.clone(),
                _ => {
                    return Err(AuthRejection {
                        message: "internal error: JWT secret not configured".to_string(),
                    });
                }
            };

            match jwt::verify_token(token, &jwt_secret) {
                Ok(claims) => {
                    debug!(owner_id = %claims.sub, "authenticated via JWT");
                    return Ok(AuthUser {
                        owner_id: claims.sub,
                    });
                }
                Err(e) => {
                    return Err(AuthRejection {
                        message: format!("invalid JWT: {e}"),
                    });
                }
            }
        }

        // Try X-API-Key header
        if let Some(api_key_header) = parts.headers.get("x-api-key")
            && let Ok(key) = api_key_header.to_str()
        {
            let pool = parts
                .extensions
                .get::<PgPool>()
                .cloned()
                .ok_or_else(|| AuthRejection {
                    message: "internal error: no database pool".to_string(),
                })?;

            match apikey::verify_key(&pool, key).await {
                Ok(owner_id) => {
                    debug!(owner_id = %owner_id, "authenticated via API key");
                    return Ok(AuthUser { owner_id });
                }
                Err(e) => {
                    return Err(AuthRejection {
                        message: format!("invalid API key: {e}"),
                    });
                }
            }
        }

        Err(AuthRejection {
            message:
                "missing authentication: provide Authorization Bearer token or X-API-Key header"
                    .to_string(),
        })
    }
}

/// Extension type to pass the JWT secret to the auth middleware.
#[derive(Clone)]
pub struct JwtSecret(pub String);
