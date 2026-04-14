//! Comprehensive project health calculator.
//!
//! Checks 6 dimensions: knowledge freshness, procedure health, episode coverage,
//! core memory completeness, search effectiveness, and learning pipeline health.

use alaz_core::Result;
use sqlx::PgPool;
use tracing::debug;

/// Full project health report with per-dimension scores and recommendations.
#[derive(Debug, serde::Serialize)]
pub struct ProjectHealthReport {
    pub project_id: Option<String>,
    /// Overall health score (0.0 - 1.0), average of all dimensions.
    pub overall_score: f64,
    pub dimensions: Vec<HealthDimension>,
    pub recommendations: Vec<String>,
}

/// A single health dimension with score and status.
#[derive(Debug, serde::Serialize)]
pub struct HealthDimension {
    pub name: String,
    /// Score from 0.0 to 1.0.
    pub score: f64,
    /// One of "healthy", "warning", "critical".
    pub status: String,
    pub detail: String,
}

/// Compute project health across 6 dimensions.
pub async fn compute_project_health(
    pool: &PgPool,
    project_id: Option<&str>,
) -> Result<ProjectHealthReport> {
    let mut dimensions = Vec::new();
    let mut recommendations = Vec::new();

    // 1. Knowledge Freshness: % of items accessed in last 30 days
    let freshness = knowledge_freshness(pool, project_id).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Knowledge Freshness",
        freshness.score,
        &freshness.detail,
        "Review and access stale knowledge items to keep them relevant",
        "Many knowledge items are stale — consider archiving unused items and adding fresh content",
    );

    // 2. Procedure Health: avg success_rate of well-used procedures
    let procedures = procedure_health(pool, project_id).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Procedure Health",
        procedures.score,
        &procedures.detail,
        "Some procedures have low success rates — review and update their steps",
        "Procedures are failing frequently — audit and rewrite underperforming procedures",
    );

    // 3. Episode Coverage: recent episodes (target: >= 3 per week)
    let episodes = episode_coverage(pool, project_id).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Episode Coverage",
        episodes.score,
        &episodes.detail,
        "Few recent episodes — ensure notable events are being captured",
        "No recent episodes — the system may not be capturing important events",
    );

    // 4. Core Memory Completeness: count of core memories (target: >= 5)
    let memories = core_memory_completeness(pool, project_id).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Core Memory Completeness",
        memories.score,
        &memories.detail,
        "Add more core memories to capture preferences, facts, and conventions",
        "Very few core memories — add essential preferences, constraints, and facts",
    );

    // 5. Search Effectiveness: click-through rate from recent searches
    let search = search_effectiveness(pool, project_id).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Search Effectiveness",
        search.score,
        &search.detail,
        "Search results are not being clicked often — review content quality and relevance",
        "Very low search CTR — knowledge base may have gaps or poor content alignment",
    );

    // 6. Learning Pipeline Health: successful runs in last 7 days
    let learning = learning_pipeline_health(pool).await?;
    push_dimension(
        &mut dimensions,
        &mut recommendations,
        "Learning Pipeline",
        learning.score,
        &learning.detail,
        "Learning pipeline has not run recently — ensure sessions trigger learning",
        "Learning pipeline appears inactive — check pipeline configuration and session integration",
    );

    let overall_score = if dimensions.is_empty() {
        0.0
    } else {
        dimensions.iter().map(|d| d.score).sum::<f64>() / dimensions.len() as f64
    };

    let pid = project_id.map(String::from);
    debug!(
        project_id = ?pid,
        overall_score,
        dimensions = dimensions.len(),
        recommendations = recommendations.len(),
        "project health computed"
    );

    Ok(ProjectHealthReport {
        project_id: pid,
        overall_score,
        dimensions,
        recommendations,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct DimensionResult {
    score: f64,
    detail: String,
}

fn push_dimension(
    dimensions: &mut Vec<HealthDimension>,
    recommendations: &mut Vec<String>,
    name: &str,
    score: f64,
    detail: &str,
    warning_rec: &str,
    critical_rec: &str,
) {
    let status = if score >= 0.7 {
        "healthy"
    } else if score >= 0.4 {
        recommendations.push(warning_rec.to_string());
        "warning"
    } else {
        recommendations.push(critical_rec.to_string());
        "critical"
    };

    dimensions.push(HealthDimension {
        name: name.to_string(),
        score,
        status: status.to_string(),
        detail: detail.to_string(),
    });
}

/// 1. Knowledge Freshness: fraction of items accessed in last 30 days.
async fn knowledge_freshness(pool: &PgPool, project_id: Option<&str>) -> Result<DimensionResult> {
    let row: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*)::BIGINT AS total,
            COUNT(*) FILTER (WHERE last_accessed_at > now() - interval '30 days')::BIGINT AS fresh
        FROM knowledge_items
        WHERE ($1::TEXT IS NULL OR project_id = $1)
          AND superseded_by IS NULL
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    let (total, fresh) = row;
    let score = if total == 0 {
        0.0
    } else {
        fresh as f64 / total as f64
    };

    Ok(DimensionResult {
        score,
        detail: format!("{fresh}/{total} knowledge items accessed in the last 30 days"),
    })
}

/// 2. Procedure Health: average success_rate of procedures used >= 3 times.
async fn procedure_health(pool: &PgPool, project_id: Option<&str>) -> Result<DimensionResult> {
    let avg: Option<f64> = sqlx::query_scalar(
        r#"
        SELECT AVG(success_rate)::FLOAT8
        FROM procedures
        WHERE times_used >= 3
          AND ($1::TEXT IS NULL OR project_id = $1)
          AND superseded_by IS NULL
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    let score = avg.unwrap_or(1.0).clamp(0.0, 1.0);
    let detail = match avg {
        Some(v) => format!("Average procedure success rate: {:.0}%", v * 100.0),
        None => "No procedures with 3+ uses to evaluate".to_string(),
    };

    Ok(DimensionResult { score, detail })
}

/// 3. Episode Coverage: episodes in last 7 days (target: >= 3).
async fn episode_coverage(pool: &PgPool, project_id: Option<&str>) -> Result<DimensionResult> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM episodes
        WHERE created_at > now() - interval '7 days'
          AND ($1::TEXT IS NULL OR project_id = $1)
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    let target = 3.0_f64;
    let score = (count as f64 / target).clamp(0.0, 1.0);

    Ok(DimensionResult {
        score,
        detail: format!("{count} episodes in the last 7 days (target: {target:.0}+)"),
    })
}

/// 4. Core Memory Completeness: count of core memories (target: >= 5).
async fn core_memory_completeness(
    pool: &PgPool,
    project_id: Option<&str>,
) -> Result<DimensionResult> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM core_memories
        WHERE ($1::TEXT IS NULL OR project_id = $1)
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    let target = 5.0_f64;
    let score = (count as f64 / target).clamp(0.0, 1.0);

    Ok(DimensionResult {
        score,
        detail: format!("{count} core memories (target: {target:.0}+)"),
    })
}

/// 5. Search Effectiveness: click-through rate from searches in last 7 days.
async fn search_effectiveness(pool: &PgPool, project_id: Option<&str>) -> Result<DimensionResult> {
    let row: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*)::BIGINT AS total,
            COUNT(*) FILTER (WHERE array_length(clicked_ids, 1) > 0)::BIGINT AS clicked
        FROM search_queries
        WHERE created_at > now() - interval '7 days'
          AND ($1::TEXT IS NULL OR project_id = $1)
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    let (total, clicked) = row;
    let score = if total == 0 {
        1.0 // No searches = no problem; don't penalize
    } else {
        (clicked as f64 / total as f64).clamp(0.0, 1.0)
    };

    let detail = if total == 0 {
        "No searches in the last 7 days".to_string()
    } else {
        format!("{clicked}/{total} searches had click-through in the last 7 days")
    };

    Ok(DimensionResult { score, detail })
}

/// 6. Learning Pipeline Health: runs in last 7 days (target: >= 2).
async fn learning_pipeline_health(pool: &PgPool) -> Result<DimensionResult> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM learning_runs
        WHERE created_at > now() - interval '7 days'
        "#,
    )
    .fetch_one(pool)
    .await?;

    let target = 2.0_f64;
    let score = (count as f64 / target).clamp(0.0, 1.0);

    Ok(DimensionResult {
        score,
        detail: format!("{count} learning runs in the last 7 days (target: {target:.0}+)"),
    })
}
