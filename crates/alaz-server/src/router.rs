use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::warn;

use crate::api;
use crate::mcp::AlazMcpServer;
use crate::middleware::JwtSecret;
use crate::rate_limit::{RateLimiter, rate_limit_middleware};
use crate::state::AppState;

/// Build the combined router with REST API and MCP endpoints.
pub fn build_router(state: AppState) -> Router {
    let api = api::router(state.clone());

    // Create the MCP StreamableHTTP service
    let mcp_state = state.clone();
    let session_manager = Arc::new(LocalSessionManager::default());
    let mcp_config = StreamableHttpServerConfig::default();

    let mcp_service = StreamableHttpService::new(
        move || Ok(AlazMcpServer::new(mcp_state.clone())),
        session_manager,
        mcp_config,
    );

    // Auth extensions: inject JwtSecret and PgPool so AuthUser extractor can access them
    let jwt_secret = JwtSecret(state.config.jwt_secret.clone());
    let pool = state.pool.clone();

    // Rate limiter: 60 requests per 60 seconds per IP
    let limiter = Arc::new(RateLimiter::new(60, 60));

    // Spawn cleanup job (every 5 minutes, remove stale IP entries)
    let cleanup_limiter = limiter.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            cleanup_limiter.cleanup();
        }
    });

    // MCP route with API key auth middleware
    let mcp_router = Router::new()
        .route_service("/mcp", mcp_service)
        .layer(middleware::from_fn(require_mcp_auth));

    // Proactive context state (session cooldown + dedup tracking)
    let proactive_state = ProactiveState {
        app: state.clone(),
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    let proactive_router = Router::new()
        .route(
            "/api/proactive-context",
            axum::routing::post(proactive_context_handler),
        )
        .layer(middleware::from_fn(require_mcp_auth))
        .with_state(proactive_state);

    // Build CORS origins before state is consumed by health_router
    let cors_origins: Vec<HeaderValue> = state
        .config
        .cors_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();

    let health_router = Router::new()
        .route("/health", axum::routing::get(health_check))
        .with_state(state);

    Router::new()
        .nest("/api/v1", api)
        .merge(mcp_router)
        .merge(proactive_router)
        .merge(health_router)
        .layer(axum::Extension(jwt_secret))
        .layer(axum::Extension(pool))
        .layer(axum::Extension(limiter))
        .layer(middleware::from_fn(rate_limit_middleware))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(cors_origins)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers(tower_http::cors::Any),
        )
}

/// Lightweight auth middleware for the MCP endpoint.
///
/// Validates the `X-API-Key` header against the database.
/// MCP clients (like Claude Code) send the API key in headers,
/// so JWT is not needed here — API key is sufficient.
async fn require_mcp_auth(request: Request, next: Next) -> Response {
    let headers = request.headers();

    // Check X-API-Key header
    let api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());

    let Some(key) = api_key else {
        warn!("MCP request rejected: missing X-API-Key header");
        return (StatusCode::UNAUTHORIZED, "missing X-API-Key header").into_response();
    };

    let pool = match request.extensions().get::<sqlx::PgPool>().cloned() {
        Some(p) => p,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    match alaz_auth::verify_key(&pool, key).await {
        Ok(_owner_id) => next.run(request).await,
        Err(_) => {
            warn!("MCP request rejected: invalid API key");
            (StatusCode::UNAUTHORIZED, "invalid API key").into_response()
        }
    }
}

// === Proactive Context Injection ===

/// Per-session tracking for rate limiting and dedup.
struct SessionTracker {
    last_call: Instant,
    injected_ids: Vec<String>,
}

#[derive(Clone)]
struct ProactiveState {
    app: AppState,
    sessions: Arc<Mutex<HashMap<String, SessionTracker>>>,
}

#[derive(Deserialize)]
struct ProactiveRequest {
    tool: String,
    context: String,
    session_id: Option<String>,
}

#[derive(Serialize)]
struct ProactiveResponse {
    results: Vec<alaz_search::ProactiveResult>,
}

/// Session-based cooldown (30 seconds).
const PROACTIVE_COOLDOWN_SECS: u64 = 30;

async fn proactive_context_handler(
    State(state): State<ProactiveState>,
    Json(req): Json<ProactiveRequest>,
) -> impl IntoResponse {
    let session_id = req.session_id.as_deref().unwrap_or("default").to_string();

    // Extract keywords from tool + context
    let keywords = match alaz_search::extract_keywords(&req.tool, &req.context) {
        Some(kw) => kw,
        None => {
            return (StatusCode::OK, Json(ProactiveResponse { results: vec![] })).into_response();
        }
    };

    // Check rate limit and collect injected IDs — release lock before DB query
    let injected_ids = {
        let mut sessions = state.sessions.lock().await;

        // Periodic cleanup: remove sessions idle for more than 10 minutes
        if sessions.len() > 50 {
            sessions.retain(|_, t| t.last_call.elapsed().as_secs() < 600);
        }

        if let Some(tracker) = sessions.get(session_id.as_str()) {
            if tracker.last_call.elapsed().as_secs() < PROACTIVE_COOLDOWN_SECS {
                return (StatusCode::OK, Json(ProactiveResponse { results: vec![] }))
                    .into_response();
            }
            tracker.injected_ids.clone()
        } else {
            Vec::new()
        }
    }; // lock released here

    // Resolve project from the context path if possible
    let project_id = None::<&str>; // Proactive search is project-agnostic for now

    // Run the lightweight FTS search (no lock held)
    let results = match alaz_search::proactive_search(&state.app.pool, &keywords, project_id).await
    {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::OK, Json(ProactiveResponse { results: vec![] })).into_response();
        }
    };

    // Filter out already-injected entities
    let new_results: Vec<alaz_search::ProactiveResult> = results
        .into_iter()
        .filter(|r| !injected_ids.contains(&r.entity_id))
        .collect();

    // Re-acquire lock to update session tracker
    {
        let mut sessions = state.sessions.lock().await;
        let new_ids: Vec<String> = new_results.iter().map(|r| r.entity_id.clone()).collect();
        let tracker = sessions
            .entry(session_id)
            .or_insert_with(|| SessionTracker {
                last_call: Instant::now(),
                injected_ids: Vec::new(),
            });
        tracker.last_call = Instant::now();
        tracker.injected_ids.extend(new_ids);

        // Cap injected_ids to prevent unbounded growth
        if tracker.injected_ids.len() > 100 {
            tracker.injected_ids = tracker
                .injected_ids
                .split_off(tracker.injected_ids.len() - 50);
        }
    }

    (
        StatusCode::OK,
        Json(ProactiveResponse {
            results: new_results,
        }),
    )
        .into_response()
}

/// Check a single HTTP service by GETting a URL.
async fn check_http_service(client: &reqwest::Client, url: &str) -> (bool, u64) {
    let start = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await;
    let ms = start.elapsed().as_millis() as u64;
    let up = matches!(result, Ok(Ok(resp)) if resp.status().is_success());
    (up, ms)
}

/// Shared HTTP client for health checks — avoids creating a new client per request.
static HEALTH_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

async fn health_check(State(state): State<AppState>) -> axum::Json<Value> {
    let http_client = HEALTH_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default()
    });

    let config = &state.config;

    // Pre-build URLs so they live long enough for the async block borrows
    let ollama_health_url = format!("{}/", config.ollama_url);
    let tei_health_url = format!("{}/health", config.tei_url);
    let colbert_health_url = format!("{}/health", config.colbert_url);

    // Check all services in parallel
    let (pg_result, qdrant_result, ollama_result, tei_result, colbert_result) = tokio::join!(
        async {
            let start = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                sqlx::query("SELECT 1").execute(&state.pool),
            )
            .await;
            let ms = start.elapsed().as_millis() as u64;
            (matches!(result, Ok(Ok(_))), ms)
        },
        async {
            let start = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                state.qdrant.client().collection_exists("alaz_text"),
            )
            .await;
            let ms = start.elapsed().as_millis() as u64;
            (matches!(result, Ok(Ok(_))), ms)
        },
        check_http_service(http_client, &ollama_health_url),
        check_http_service(http_client, &tei_health_url),
        check_http_service(http_client, &colbert_health_url),
    );

    // Determine overall status
    let all_up =
        pg_result.0 && qdrant_result.0 && ollama_result.0 && tei_result.0 && colbert_result.0;
    let db_up = pg_result.0;
    let overall = if all_up {
        "healthy"
    } else if db_up {
        "degraded"
    } else {
        "unhealthy"
    };

    // Only expose service status (up/down), no ports or internal URLs
    let status_str = |up: bool| if up { "up" } else { "down" };

    let services: serde_json::Map<String, Value> = [
        (
            "postgresql".to_string(),
            json!({ "status": status_str(pg_result.0) }),
        ),
        (
            "qdrant".to_string(),
            json!({ "status": status_str(qdrant_result.0) }),
        ),
        (
            "ollama".to_string(),
            json!({ "status": status_str(ollama_result.0) }),
        ),
        (
            "tei_embeddings".to_string(),
            json!({ "status": status_str(tei_result.0) }),
        ),
        (
            "colbert".to_string(),
            json!({ "status": status_str(colbert_result.0) }),
        ),
    ]
    .into_iter()
    .collect();

    axum::Json(json!({
        "status": overall,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "services": services,
    }))
}
