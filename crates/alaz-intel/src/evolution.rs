//! Knowledge evolution tracking and spaced repetition.
//!
//! Follows supersede chains to show how knowledge evolved over time,
//! and implements SM-2 spaced repetition for important items.

use std::collections::HashSet;

use alaz_core::Result;
use alaz_core::models::KnowledgeItem;
use alaz_db::repos::KnowledgeRepo;
use sqlx::PgPool;
use tracing::{debug, warn};

/// A single entry in the evolution chain.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvolutionEntry {
    pub id: String,
    pub title: String,
    pub version: usize,
    pub reason: Option<String>,
    pub created_at: String,
    pub is_current: bool,
}

/// The full evolution chain of a knowledge item.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvolutionChain {
    pub current_id: String,
    pub current_title: String,
    pub total_versions: usize,
    pub entries: Vec<EvolutionEntry>,
}

/// Follow the supersede chain backward to find all previous versions.
///
/// Starting from `id`, finds items whose `superseded_by` points to this item,
/// then recurses backward. Returns the full chain oldest→newest.
pub async fn get_evolution_chain(pool: &PgPool, id: &str) -> Result<EvolutionChain> {
    let current = KnowledgeRepo::get_readonly(pool, id).await?;

    let mut predecessors: Vec<PredecessorRow> = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(id.to_string());

    // Walk backward: find items that were superseded by the current one.
    // Uses a visited set to prevent infinite loops from cyclic supersede chains.
    let mut target_id = id.to_string();
    for _ in 0..50 {
        let predecessor = find_predecessor(pool, &target_id).await?;
        match predecessor {
            Some(pred) => {
                if !visited.insert(pred.id.clone()) {
                    warn!(id = %pred.id, "evolution: cyclic supersede chain detected, stopping");
                    break;
                }
                target_id = pred.id.clone();
                predecessors.push(pred);
            }
            None => break,
        }
    }

    // Reverse so oldest is first
    predecessors.reverse();

    // Walk forward from current to find any successors.
    // Uses lightweight PredecessorRow query to avoid fetching full KnowledgeItem.
    let mut successor_id = current.superseded_by.clone();
    let mut successors: Vec<PredecessorRow> = Vec::new();
    for _ in 0..50 {
        match successor_id {
            Some(ref sid) => {
                if !visited.insert(sid.clone()) {
                    warn!(id = %sid, "evolution: cyclic supersede chain detected (forward), stopping");
                    break;
                }
                // Use lightweight query for forward walk too
                match find_by_id(pool, sid).await? {
                    Some(row) => {
                        // Check if this item has a successor
                        successor_id = get_superseded_by(pool, sid).await?;
                        successors.push(row);
                    }
                    None => break,
                }
            }
            None => break,
        }
    }

    // Build full chain: predecessors + current + successors
    let total = predecessors.len() + 1 + successors.len();
    let mut entries: Vec<EvolutionEntry> = Vec::with_capacity(total);

    for (i, pred) in predecessors.iter().enumerate() {
        entries.push(EvolutionEntry {
            id: pred.id.clone(),
            title: pred.title.clone(),
            version: i + 1,
            reason: pred.invalidation_reason.clone(),
            created_at: pred.created_at.format("%Y-%m-%d").to_string(),
            is_current: false,
        });
    }

    let current_version = predecessors.len() + 1;
    let current_is_latest = successors.is_empty() && current.superseded_by.is_none();
    entries.push(EvolutionEntry {
        id: current.id.clone(),
        title: current.title.clone(),
        version: current_version,
        reason: current.invalidation_reason.clone(),
        created_at: current.created_at.format("%Y-%m-%d").to_string(),
        is_current: current_is_latest,
    });

    for (i, succ) in successors.iter().enumerate() {
        let is_last = i == successors.len() - 1;
        entries.push(EvolutionEntry {
            id: succ.id.clone(),
            title: succ.title.clone(),
            version: current_version + i + 1,
            reason: succ.invalidation_reason.clone(),
            created_at: succ.created_at.format("%Y-%m-%d").to_string(),
            is_current: is_last,
        });
    }

    let latest = entries.last().cloned().unwrap_or(EvolutionEntry {
        id: id.to_string(),
        title: current.title.clone(),
        version: 1,
        reason: None,
        created_at: current.created_at.format("%Y-%m-%d").to_string(),
        is_current: true,
    });

    debug!(id, versions = entries.len(), "evolution chain resolved");

    Ok(EvolutionChain {
        current_id: latest.id,
        current_title: latest.title,
        total_versions: entries.len(),
        entries,
    })
}

/// Fetch lightweight info by ID (for forward walk).
async fn find_by_id(pool: &PgPool, id: &str) -> Result<Option<PredecessorRow>> {
    let row = sqlx::query_as::<_, PredecessorRow>(
        "SELECT id, title, invalidation_reason, created_at FROM knowledge_items WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Fetch only the `superseded_by` field for an item.
async fn get_superseded_by(pool: &PgPool, id: &str) -> Result<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT superseded_by FROM knowledge_items WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    Ok(row.and_then(|r| r.0))
}

/// Lightweight predecessor info (avoids fetching all 25+ columns).
#[derive(Debug, sqlx::FromRow)]
struct PredecessorRow {
    id: String,
    title: String,
    invalidation_reason: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Find the item that was superseded by `target_id`.
///
/// Only fetches columns needed for the evolution chain display,
/// avoiding the overhead of a full `KnowledgeItem` load per hop.
async fn find_predecessor(pool: &PgPool, target_id: &str) -> Result<Option<PredecessorRow>> {
    let row = sqlx::query_as::<_, PredecessorRow>(
        r#"
        SELECT id, title, invalidation_reason, created_at
        FROM knowledge_items
        WHERE superseded_by = $1
        LIMIT 1
        "#,
    )
    .bind(target_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

// ---------------------------------------------------------------------------
// Spaced Repetition (SM-2 algorithm)
// ---------------------------------------------------------------------------

/// SM-2 review result quality (0-5 scale).
#[derive(Debug, Clone, Copy)]
pub enum ReviewQuality {
    /// Complete blackout (0).
    Blackout = 0,
    /// Incorrect, but remembered upon seeing answer (1).
    Incorrect = 1,
    /// Incorrect, but easy to recall once shown (2).
    DifficultRecall = 2,
    /// Correct with serious difficulty (3).
    Correct = 3,
    /// Correct after hesitation (4).
    CorrectHesitation = 4,
    /// Perfect response (5).
    Perfect = 5,
}

impl ReviewQuality {
    pub fn from_score(score: i32) -> Self {
        match score {
            0 => Self::Blackout,
            1 => Self::Incorrect,
            2 => Self::DifficultRecall,
            3 => Self::Correct,
            4 => Self::CorrectHesitation,
            _ => Self::Perfect,
        }
    }
}

/// SM-2 algorithm output.
#[derive(Debug)]
pub struct Sm2Result {
    pub interval_days: i32,
    pub easiness: f32,
    pub repetitions: i32,
}

/// Compute the next review interval using the SM-2 algorithm.
///
/// Based on: <https://en.wikipedia.org/wiki/SuperMemo#SM-2_algorithm>
pub fn sm2_next_review(
    quality: ReviewQuality,
    current_easiness: f32,
    current_repetitions: i32,
    current_interval: i32,
) -> Sm2Result {
    let q = quality as i32;

    if q < 3 {
        // Failed review: reset repetitions, short interval
        return Sm2Result {
            interval_days: 1,
            easiness: current_easiness,
            repetitions: 0,
        };
    }

    // Update easiness factor
    let new_easiness = current_easiness + (0.1 - (5 - q) as f32 * (0.08 + (5 - q) as f32 * 0.02));
    let new_easiness = new_easiness.max(1.3); // minimum 1.3

    let new_repetitions = current_repetitions + 1;
    let new_interval = match new_repetitions {
        1 => 1,
        2 => 6,
        _ => (current_interval as f32 * new_easiness).round() as i32,
    };

    Sm2Result {
        interval_days: new_interval,
        easiness: new_easiness,
        repetitions: new_repetitions,
    }
}

/// Record a review for a knowledge item and update its SR schedule.
pub async fn record_review(pool: &PgPool, id: &str, quality: i32) -> Result<()> {
    // Fetch current SR state
    let row = sqlx::query_as::<_, SrState>(
        "SELECT sr_interval_days, sr_easiness, sr_repetitions FROM knowledge_items WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(state) = row else {
        return Err(alaz_core::AlazError::NotFound(format!(
            "knowledge item {id}"
        )));
    };

    let result = sm2_next_review(
        ReviewQuality::from_score(quality),
        state.sr_easiness,
        state.sr_repetitions,
        state.sr_interval_days,
    );

    sqlx::query(
        r#"
        UPDATE knowledge_items
        SET sr_interval_days = $2,
            sr_easiness = $3,
            sr_repetitions = $4,
            sr_next_review = now() + make_interval(days => $2),
            access_count = access_count + 1,
            last_accessed_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(result.interval_days)
    .bind(result.easiness)
    .bind(result.repetitions)
    .execute(pool)
    .await?;

    debug!(
        id,
        quality,
        next_interval = result.interval_days,
        easiness = result.easiness,
        "review recorded"
    );

    Ok(())
}

/// Get items due for review.
pub async fn items_due_for_review(
    pool: &PgPool,
    project_id: Option<&str>,
    limit: i64,
) -> Result<Vec<KnowledgeItem>> {
    let rows = sqlx::query_as::<_, KnowledgeItem>(
        r#"
        SELECT id, title, content, description, type AS kind, language, file_path, project_id,
               tags, utility_score, access_count, last_accessed_at, needs_embedding, feedback_boost,
               valid_from, valid_until, superseded_by, invalidation_reason, source, source_metadata,
               times_used, times_success, pattern_score, created_at, updated_at
        FROM knowledge_items
        WHERE sr_next_review IS NOT NULL
          AND sr_next_review <= now()
          AND superseded_by IS NULL
          AND ($1::TEXT IS NULL OR project_id = $1)
        ORDER BY sr_next_review ASC
        LIMIT $2
        "#,
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Schedule a knowledge item for spaced repetition review.
///
/// Sets `sr_next_review` to tomorrow if not already set.
pub async fn schedule_for_review(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE knowledge_items
        SET sr_next_review = COALESCE(sr_next_review, now() + interval '1 day')
        WHERE id = $1 AND superseded_by IS NULL
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
struct SrState {
    sr_interval_days: i32,
    sr_easiness: f32,
    sr_repetitions: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_perfect_response_increases_interval() {
        let result = sm2_next_review(ReviewQuality::Perfect, 2.5, 2, 6);
        assert!(result.interval_days > 6);
        assert!(result.easiness >= 2.5);
        assert_eq!(result.repetitions, 3);
    }

    #[test]
    fn sm2_failed_response_resets() {
        let result = sm2_next_review(ReviewQuality::Blackout, 2.5, 5, 30);
        assert_eq!(result.interval_days, 1);
        assert_eq!(result.repetitions, 0);
    }

    #[test]
    fn sm2_first_review() {
        let result = sm2_next_review(ReviewQuality::Correct, 2.5, 0, 1);
        assert_eq!(result.interval_days, 1);
        assert_eq!(result.repetitions, 1);
    }

    #[test]
    fn sm2_second_review() {
        let result = sm2_next_review(ReviewQuality::Perfect, 2.5, 1, 1);
        assert_eq!(result.interval_days, 6);
        assert_eq!(result.repetitions, 2);
    }

    #[test]
    fn sm2_easiness_never_below_minimum() {
        // Repeated difficulty should lower easiness but never below 1.3
        let result = sm2_next_review(ReviewQuality::Correct, 1.3, 3, 10);
        assert!(result.easiness >= 1.3);
    }

    #[test]
    fn sm2_quality_from_score() {
        assert!(matches!(
            ReviewQuality::from_score(0),
            ReviewQuality::Blackout
        ));
        assert!(matches!(
            ReviewQuality::from_score(3),
            ReviewQuality::Correct
        ));
        assert!(matches!(
            ReviewQuality::from_score(5),
            ReviewQuality::Perfect
        ));
        assert!(matches!(
            ReviewQuality::from_score(99),
            ReviewQuality::Perfect
        ));
    }
}
