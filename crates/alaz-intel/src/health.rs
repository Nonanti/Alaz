//! Knowledge health scoring and gap detection.
//!
//! Computes per-topic health metrics (freshness, confidence, coverage, usefulness)
//! and detects knowledge gaps from low-CTR search queries.

use std::collections::HashMap;

use alaz_core::Result;
use alaz_core::models::{KnowledgeItem, ListKnowledgeFilter};
use alaz_db::repos::{KnowledgeRepo, ProjectRepo};
use sqlx::PgPool;
use tracing::debug;

/// Health score for a topic cluster.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TopicHealth {
    /// Topic label (derived from most common tag or kind).
    pub topic: String,
    /// Number of items in this topic.
    pub item_count: usize,
    /// Average freshness: 1.0 = accessed today, 0.0 = 30+ days stale.
    pub freshness: f64,
    /// Average confidence: based on access count and feedback boost.
    pub confidence: f64,
    /// Items with low utility score (< 0.3).
    pub stale_count: usize,
    /// Overall health: weighted average of metrics.
    pub overall: f64,
    /// Status emoji.
    pub status: String,
}

/// A detected knowledge gap.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KnowledgeGap {
    /// The query or topic with insufficient coverage.
    pub topic: String,
    /// How many times this was searched.
    pub search_count: i64,
    /// How many results were clicked (low = gap).
    pub click_count: i64,
    /// Suggestion for filling the gap.
    pub suggestion: String,
}

/// Full health report for a project.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthReport {
    pub project: String,
    pub total_items: usize,
    pub topics: Vec<TopicHealth>,
    pub stale_items: usize,
    pub gaps: Vec<KnowledgeGap>,
}

/// Compute the health report for a project.
pub async fn compute_health(pool: &PgPool, project_name: Option<&str>) -> Result<HealthReport> {
    let project_label = project_name.unwrap_or("global");

    let project_id = if let Some(name) = project_name {
        ProjectRepo::get_by_name(pool, name).await?.map(|p| p.id)
    } else {
        None
    };

    // Fetch all active items
    let filter = ListKnowledgeFilter {
        project: project_id.clone(),
        limit: Some(1000),
        ..Default::default()
    };
    let items: Vec<KnowledgeItem> = KnowledgeRepo::list(pool, &filter)
        .await?
        .into_iter()
        .filter(|i| i.superseded_by.is_none())
        .collect();

    let total_items = items.len();

    // Group by kind (or first tag)
    let mut topic_groups: HashMap<String, Vec<&KnowledgeItem>> = HashMap::new();
    for item in &items {
        let topic = if !item.tags.is_empty() {
            item.tags[0].clone()
        } else {
            item.kind.clone()
        };
        topic_groups.entry(topic).or_default().push(item);
    }

    let now = chrono::Utc::now();
    let mut topics: Vec<TopicHealth> = topic_groups
        .iter()
        .map(|(topic, group)| {
            let item_count = group.len();

            // Freshness: days since last access, mapped to 0..1
            let freshness: f64 = group
                .iter()
                .map(|i| {
                    let last = i.last_accessed_at.unwrap_or(i.created_at);
                    let days = (now - last).num_days() as f64;
                    (1.0 - days / 30.0).clamp(0.0, 1.0)
                })
                .sum::<f64>()
                / item_count as f64;

            // Confidence: based on access count and feedback
            let confidence: f64 = group
                .iter()
                .map(|i| {
                    let access_factor = (i.access_count as f64).ln_1p() / 5.0; // ln(1+count)/5
                    let feedback = f64::from(i.feedback_boost);
                    (access_factor + feedback).clamp(0.0, 1.0)
                })
                .sum::<f64>()
                / item_count as f64;

            let stale_count = group.iter().filter(|i| i.utility_score < 0.3).count();

            let overall = freshness * 0.4
                + confidence * 0.4
                + (1.0 - stale_count as f64 / item_count.max(1) as f64) * 0.2;

            let status = match overall {
                x if x >= 0.7 => "🟢".to_string(),
                x if x >= 0.4 => "🟡".to_string(),
                _ => "🔴".to_string(),
            };

            TopicHealth {
                topic: topic.clone(),
                item_count,
                freshness,
                confidence,
                stale_count,
                overall,
                status,
            }
        })
        .collect();

    // Sort by overall ascending (worst first)
    topics.sort_by(|a, b| {
        a.overall
            .partial_cmp(&b.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let stale_items = items.iter().filter(|i| i.utility_score < 0.3).count();

    // Detect gaps: queries with low CTR from the last 14 days
    let gaps = detect_gaps(pool, project_id.as_deref()).await?;

    debug!(
        project = project_label,
        total_items,
        topics = topics.len(),
        stale_items,
        gaps = gaps.len(),
        "health report computed"
    );

    Ok(HealthReport {
        project: project_label.to_string(),
        total_items,
        topics,
        stale_items,
        gaps,
    })
}

/// Detect knowledge gaps from search queries with low or zero click-through.
async fn detect_gaps(pool: &PgPool, project_id: Option<&str>) -> Result<Vec<KnowledgeGap>> {
    // Queries searched 2+ times in the last 14 days with 0 clicks
    let rows = sqlx::query_as::<_, GapRow>(
        r#"
        SELECT query,
               COUNT(*) AS search_count,
               SUM(array_length(clicked_ids, 1))::BIGINT AS click_count
        FROM search_queries
        WHERE created_at > now() - interval '14 days'
          AND ($1::TEXT IS NULL OR project_id = $1)
        GROUP BY query
        HAVING COUNT(*) >= 2
           AND COALESCE(SUM(array_length(clicked_ids, 1)), 0) = 0
        ORDER BY COUNT(*) DESC
        LIMIT 10
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| KnowledgeGap {
            suggestion: format!(
                "\"{}\" was searched {} times with no results clicked — consider adding knowledge about this topic",
                r.query, r.search_count
            ),
            topic: r.query,
            search_count: r.search_count,
            click_count: r.click_count.unwrap_or(0),
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct GapRow {
    query: String,
    search_count: i64,
    click_count: Option<i64>,
}
