pub mod code;
pub mod context;
pub mod episodic;
pub mod git;
pub mod graph;
pub mod ingest;
pub mod knowledge;
pub mod learn;
pub mod project;
pub mod search;
pub mod session;
pub mod system;
pub mod tags;
pub mod vault;

use crate::middleware::JwtSecret;
use crate::state::AppState;
use alaz_db::repos::{AuditRepo, DeviceRepo};
use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use tracing::{debug, warn};

/// Middleware that enforces authentication + device trust on all API routes.
///
/// Auth flow:
/// 1. Authenticate via `Authorization: Bearer <jwt>` or `X-API-Key: <key>`
/// 2. If `X-Device-Fingerprint` header is present, verify the device is trusted
/// 3. If fingerprint is unknown, auto-register it (untrusted) and return 403
/// 4. Log the request to audit_logs
async fn require_auth(request: Request, next: Next) -> Response {
    let headers = request.headers();

    let client_ip = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        });

    let device_fingerprint = headers
        .get("x-device-fingerprint")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let pool = match request.extensions().get::<sqlx::PgPool>().cloned() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error: no database pool",
            )
                .into_response();
        }
    };

    // --- Step 1: Authenticate (JWT or API key) ---
    let owner_id: String;

    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(value) = auth_header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                let jwt_secret = request
                    .extensions()
                    .get::<JwtSecret>()
                    .map(|s| s.0.clone())
                    .unwrap_or_default();

                match alaz_auth::verify_token(token, &jwt_secret) {
                    Ok(claims) => {
                        owner_id = claims.sub;
                    }
                    Err(_) => {
                        return (StatusCode::UNAUTHORIZED, "invalid JWT token").into_response();
                    }
                }
            } else {
                return (StatusCode::UNAUTHORIZED, "malformed Authorization header")
                    .into_response();
            }
        } else {
            return (
                StatusCode::UNAUTHORIZED,
                "invalid Authorization header encoding",
            )
                .into_response();
        }
    } else if let Some(api_key_header) = headers.get("x-api-key") {
        if let Ok(key) = api_key_header.to_str() {
            match alaz_auth::verify_key(&pool, key).await {
                Ok(oid) => {
                    owner_id = oid;
                }
                Err(_) => {
                    return (StatusCode::UNAUTHORIZED, "invalid API key").into_response();
                }
            }
        } else {
            return (
                StatusCode::UNAUTHORIZED,
                "invalid X-API-Key header encoding",
            )
                .into_response();
        }
    } else {
        return (
            StatusCode::UNAUTHORIZED,
            "missing authentication: provide Authorization Bearer token or X-API-Key header",
        )
            .into_response();
    }

    // --- Step 2: Device fingerprint verification ---
    if let Some(ref fingerprint) = device_fingerprint {
        match DeviceRepo::get_by_fingerprint(&pool, fingerprint).await {
            Ok(Some(device)) => {
                if !device.trusted {
                    warn!(
                        fingerprint = %fingerprint,
                        device_id = %device.id,
                        "untrusted device attempted access"
                    );
                    // Audit log: untrusted device attempt
                    let _ = AuditRepo::log(
                        &pool,
                        Some(&owner_id),
                        "device.untrusted_access",
                        serde_json::json!({"fingerprint": fingerprint, "device_id": device.id}),
                        client_ip.as_deref(),
                    )
                    .await;
                    return (
                        StatusCode::FORBIDDEN,
                        "device not trusted — approve via: alaz device approve <device-id>",
                    )
                        .into_response();
                }
                // Trusted device — update last_seen_at
                let _ = DeviceRepo::touch(&pool, fingerprint).await;
                debug!(fingerprint = %fingerprint, "trusted device verified");
            }
            Ok(None) => {
                // Unknown device — auto-register as untrusted
                let _ = DeviceRepo::register(&pool, &owner_id, fingerprint, None).await;
                warn!(
                    fingerprint = %fingerprint,
                    "new device auto-registered as untrusted"
                );
                let _ = AuditRepo::log(
                    &pool,
                    Some(&owner_id),
                    "device.auto_registered",
                    serde_json::json!({"fingerprint": fingerprint}),
                    client_ip.as_deref(),
                )
                .await;
                return (
                    StatusCode::FORBIDDEN,
                    "new device registered but not yet trusted — approve via: alaz device approve <device-id>",
                )
                    .into_response();
            }
            Err(e) => {
                warn!(error = %e, "device lookup failed, allowing request");
                // Graceful degradation: if DB lookup fails, allow the request
            }
        }
    }

    // --- Step 3: Audit log ---
    let _ = AuditRepo::log(
        &pool,
        Some(&owner_id),
        "api.request",
        serde_json::json!({
            "device_fingerprint": device_fingerprint,
        }),
        client_ip.as_deref(),
    )
    .await;

    next.run(request).await
}

/// Build the REST API router with all sub-routes, protected by auth middleware.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(knowledge::router(state.clone()))
        .merge(graph::router(state.clone()))
        .merge(episodic::router(state.clone()))
        .merge(search::router(state.clone()))
        .merge(session::router(state.clone()))
        .merge(context::router(state.clone()))
        .merge(learn::router(state.clone()))
        .merge(vault::router(state.clone()))
        .merge(project::router(state.clone()))
        .merge(tags::router(state.clone()))
        .merge(system::router(state.clone()))
        .merge(ingest::router(state.clone()))
        .merge(git::router(state.clone()))
        .merge(code::router(state))
        .layer(middleware::from_fn(require_auth))
}
