use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

use alaz_db::repos::{ContextTrackingRepo, LearningRunRepo, SearchQueryRepo};

use crate::error::ApiError;
use crate::state::AppState;

/// Server start time — set once at module load.
static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn init_start_time() {
    START_TIME.get_or_init(Instant::now);
}

pub fn router(state: AppState) -> Router {
    init_start_time();
    Router::new()
        .route("/system/status", get(system_status))
        .route("/system/metrics", get(system_metrics))
        .route("/system/retention", get(retention_preview))
        .route("/system/retention/run", post(retention_run))
        .route("/system/learning-analytics", get(learning_analytics))
        .route("/system/learning-runs", get(learning_runs))
        .route("/system/context-usage", get(context_usage))
        .route("/system/search-analytics", get(search_analytics))
        .with_state(state)
}

async fn system_metrics(State(state): State<AppState>) -> Json<Value> {
    let snap = state.metrics.snapshot();
    serde_json::to_value(snap)
        .unwrap_or(json!({"error": "serialization failed"}))
        .into()
}

async fn system_status(State(state): State<AppState>) -> Json<Value> {
    let config = &state.config;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    let ollama_url = format!("{}/", config.ollama_url);
    let tei_url = format!("{}/health", config.tei_url);
    let colbert_url = format!("{}/health", config.colbert_url);

    // Service checks + DB stats in parallel
    let (pg, qdrant, ollama, tei, colbert, stats) = tokio::join!(
        check_pg(&state.pool),
        check_qdrant(&state),
        check_http(&http, &ollama_url),
        check_http(&http, &tei_url),
        check_http(&http, &colbert_url),
        fetch_stats(&state.pool),
    );

    // Uptime
    let uptime = START_TIME
        .get()
        .map(|t| format_duration(t.elapsed()))
        .unwrap_or_else(|| "unknown".into());

    // Qdrant collections count
    let qdrant_collections = if qdrant.0 {
        state
            .qdrant
            .client()
            .list_collections()
            .await
            .map(|r| r.collections.len() as u64)
            .ok()
    } else {
        None
    };

    // PG active connections
    let pg_connections: Option<i64> = if pg.0 {
        sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database()",
        )
        .fetch_one(&state.pool)
        .await
        .ok()
    } else {
        None
    };

    let status_str = |up: bool| if up { "up" } else { "down" };

    let mut services = json!({
        "api": {
            "status": "up",
            "uptime": uptime,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "postgresql": { "status": status_str(pg.0) },
        "qdrant": { "status": status_str(qdrant.0) },
        "ollama": { "status": status_str(ollama.0) },
        "tei_reranker": { "status": status_str(tei.0) },
        "colbert": { "status": status_str(colbert.0) },
    });

    // Add extra info when available
    if let Some(conns) = pg_connections {
        services["postgresql"]["connections"] = json!(conns);
    }
    if let Some(cols) = qdrant_collections {
        services["qdrant"]["collections"] = json!(cols);
    }
    if !ollama.0 {
        services["ollama"]["error"] = json!("not configured");
    }

    Json(json!({
        "services": services,
        "stats": stats,
    }))
}

async fn check_pg(pool: &sqlx::PgPool) -> (bool, u64) {
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await;
    (
        matches!(result, Ok(Ok(_))),
        start.elapsed().as_millis() as u64,
    )
}

async fn check_qdrant(state: &AppState) -> (bool, u64) {
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        state.qdrant.client().collection_exists("alaz_text"),
    )
    .await;
    (
        matches!(result, Ok(Ok(_))),
        start.elapsed().as_millis() as u64,
    )
}

async fn check_http(client: &reqwest::Client, url: &str) -> (bool, u64) {
    let start = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(3), client.get(url).send()).await;
    let up = matches!(result, Ok(Ok(resp)) if resp.status().is_success());
    (up, start.elapsed().as_millis() as u64)
}

async fn fetch_stats(pool: &sqlx::PgPool) -> Value {
    let counts: Vec<(&str, &str)> = vec![
        ("knowledge", "SELECT count(*) FROM knowledge_items"),
        ("episodes", "SELECT count(*) FROM episodes"),
        ("procedures", "SELECT count(*) FROM procedures"),
        ("projects", "SELECT count(*) FROM projects"),
        ("sessions", "SELECT count(*) FROM session_logs"),
        ("core_memories", "SELECT count(*) FROM core_memories"),
    ];

    let mut stats = serde_json::Map::new();
    for (name, sql) in counts {
        let val: i64 = sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or(0);
        stats.insert(name.to_string(), json!(val));
    }
    Value::Object(stats)
}

/// Preview items eligible for retention cleanup (dry-run).
async fn retention_preview(State(state): State<AppState>) -> Json<Value> {
    let pool = &state.pool;
    let threshold = 0.1_f64;
    let min_age = "30 days";

    let (knowledge, episodes, procedures, stale_queries) = tokio::join!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM knowledge_items WHERE utility_score < $1 AND created_at < now() - $2::interval"
        ).bind(threshold).bind(min_age).fetch_one(pool),
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM episodes WHERE utility_score < $1 AND created_at < now() - $2::interval"
        ).bind(threshold).bind(min_age).fetch_one(pool),
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM procedures WHERE utility_score < $1 AND created_at < now() - $2::interval"
        ).bind(threshold).bind(min_age).fetch_one(pool),
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM search_queries WHERE created_at < now() - interval '30 days'"
        ).fetch_one(pool),
    );

    Json(json!({
        "threshold": threshold,
        "min_age_days": 30,
        "candidates": {
            "knowledge": knowledge.unwrap_or(0),
            "episodes": episodes.unwrap_or(0),
            "procedures": procedures.unwrap_or(0),
            "stale_search_queries": stale_queries.unwrap_or(0),
        },
        "note": "POST /api/v1/system/retention/run to execute cleanup"
    }))
}

/// Execute retention cleanup: prune low-score old items + stale search queries.
///
/// Also cleans up associated Qdrant vectors and orphaned graph edges.
async fn retention_run(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let pool = &state.pool;
    let threshold = 0.1_f64;
    let min_age = "30 days";

    // Collect IDs before deleting so we can clean up Qdrant vectors
    let ki_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM knowledge_items WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let ep_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM episodes WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let proc_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM procedures WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Clean up Qdrant vectors for items about to be deleted
    let qdrant = &state.qdrant;
    for (ids, entity_type) in [
        (&ki_ids, "knowledge_item"),
        (&ep_ids, "episode"),
        (&proc_ids, "procedure"),
    ] {
        for id in ids {
            for collection in ["alaz_text", "alaz_colbert"] {
                let _ = alaz_vector::DenseVectorOps::delete_point(
                    qdrant.client(),
                    collection,
                    entity_type,
                    id,
                )
                .await;
            }
        }
    }

    // Delete from DB
    let pruned_knowledge = sqlx::query(
        "DELETE FROM knowledge_items WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .execute(pool)
    .await?
    .rows_affected() as i64;

    let pruned_episodes = sqlx::query(
        "DELETE FROM episodes WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .execute(pool)
    .await?
    .rows_affected() as i64;

    let pruned_procedures = sqlx::query(
        "DELETE FROM procedures WHERE utility_score < $1 AND created_at < now() - $2::interval",
    )
    .bind(threshold)
    .bind(min_age)
    .execute(pool)
    .await?
    .rows_affected() as i64;

    // Clean up stale search queries (older than 30 days)
    let pruned_queries =
        sqlx::query("DELETE FROM search_queries WHERE created_at < now() - interval '30 days'")
            .execute(pool)
            .await?
            .rows_affected() as i64;

    // Clean orphaned graph edges (check both source_id and target_id)
    let orphaned_edges = sqlx::query(
        r#"DELETE FROM graph_edges
        WHERE (NOT EXISTS (SELECT 1 FROM knowledge_items WHERE id = graph_edges.source_id)
           AND NOT EXISTS (SELECT 1 FROM episodes WHERE id = graph_edges.source_id)
           AND NOT EXISTS (SELECT 1 FROM procedures WHERE id = graph_edges.source_id))
           OR (NOT EXISTS (SELECT 1 FROM knowledge_items WHERE id = graph_edges.target_id)
           AND NOT EXISTS (SELECT 1 FROM episodes WHERE id = graph_edges.target_id)
           AND NOT EXISTS (SELECT 1 FROM procedures WHERE id = graph_edges.target_id))"#,
    )
    .execute(pool)
    .await?
    .rows_affected() as i64;

    Ok((
        StatusCode::OK,
        Json(json!({
            "pruned": {
                "knowledge": pruned_knowledge,
                "episodes": pruned_episodes,
                "procedures": pruned_procedures,
                "search_queries": pruned_queries,
                "orphaned_edges": orphaned_edges,
                "vectors_cleaned": ki_ids.len() + ep_ids.len() + proc_ids.len(),
            },
            "total": pruned_knowledge + pruned_episodes + pruned_procedures + pruned_queries + orphaned_edges,
        })),
    ))
}

#[derive(serde::Deserialize)]
struct LearningAnalyticsQuery {
    days: Option<i32>,
}

async fn learning_analytics(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<LearningAnalyticsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let days = q.days.unwrap_or(30);
    let analytics = LearningRunRepo::analytics(&state.pool, days).await?;
    let v = serde_json::to_value(analytics)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(serde::Deserialize)]
struct LearningRunsQuery {
    limit: Option<i64>,
}

async fn learning_runs(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<LearningRunsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = q.limit.unwrap_or(10);
    let runs = LearningRunRepo::recent(&state.pool, limit).await?;
    let v = serde_json::to_value(runs)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(serde::Deserialize)]
struct ContextUsageQuery {
    days: Option<i32>,
}

async fn context_usage(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ContextUsageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let days = q.days.unwrap_or(7);
    let stats = ContextTrackingRepo::usage_stats(&state.pool, days).await?;
    let v = serde_json::to_value(stats)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(serde::Deserialize)]
struct SearchAnalyticsQuery {
    days: Option<i32>,
}

async fn search_analytics(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<SearchAnalyticsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let days = q.days.unwrap_or(7);
    let analytics = SearchQueryRepo::analytics(&state.pool, days).await?;
    let v = serde_json::to_value(analytics)?;
    Ok((StatusCode::OK, Json(v)))
}

fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_zero() {
        let d = Duration::from_secs(0);
        assert_eq!(format_duration(d), "0m");
    }

    #[test]
    fn format_duration_minutes_only() {
        let d = Duration::from_secs(25 * 60 + 30); // 25m 30s
        assert_eq!(format_duration(d), "25m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        let d = Duration::from_secs(2 * 3600 + 15 * 60); // 2h 15m
        assert_eq!(format_duration(d), "2h 15m");
    }

    #[test]
    fn format_duration_days_and_hours() {
        let d = Duration::from_secs(3 * 86400 + 5 * 3600); // 3d 5h
        assert_eq!(format_duration(d), "3d 5h");
    }

    #[test]
    fn format_duration_exact_hour() {
        let d = Duration::from_secs(3600); // exactly 1h
        assert_eq!(format_duration(d), "1h 0m");
    }

    #[test]
    fn format_duration_exact_day() {
        let d = Duration::from_secs(86400); // exactly 1d
        assert_eq!(format_duration(d), "1d 0h");
    }

    #[test]
    fn learning_analytics_query_default() {
        let json = serde_json::json!({});
        let q: LearningAnalyticsQuery = serde_json::from_value(json).unwrap();
        assert!(q.days.is_none());
    }

    #[test]
    fn learning_analytics_query_with_days() {
        let json = serde_json::json!({"days": 7});
        let q: LearningAnalyticsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.days, Some(7));
    }

    #[test]
    fn learning_runs_query_default() {
        let json = serde_json::json!({});
        let q: LearningRunsQuery = serde_json::from_value(json).unwrap();
        assert!(q.limit.is_none());
    }

    #[test]
    fn learning_runs_query_with_limit() {
        let json = serde_json::json!({"limit": 25});
        let q: LearningRunsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.limit, Some(25));
    }

    #[test]
    fn context_usage_query_default() {
        let json = serde_json::json!({});
        let q: ContextUsageQuery = serde_json::from_value(json).unwrap();
        assert!(q.days.is_none());
    }

    #[test]
    fn context_usage_query_with_days() {
        let json = serde_json::json!({"days": 14});
        let q: ContextUsageQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.days, Some(14));
    }

    #[test]
    fn search_analytics_query_default() {
        let json = serde_json::json!({});
        let q: SearchAnalyticsQuery = serde_json::from_value(json).unwrap();
        assert!(q.days.is_none());
    }

    #[test]
    fn search_analytics_query_with_days() {
        let json = serde_json::json!({"days": 30});
        let q: SearchAnalyticsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.days, Some(30));
    }
}
