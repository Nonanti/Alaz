use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use alaz_db::repos::ProjectRepo;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/tags", get(list_tags))
        .route("/tags/{old_name}", put(rename_tag))
        .route("/tags/merge", post(merge_tags))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// List all unique tags with usage counts
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListTagsQuery {
    project: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct TagCount {
    tag: String,
    count: i64,
}

async fn list_tags(
    State(state): State<AppState>,
    Query(q): Query<ListTagsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id)
    } else {
        None
    };

    let tags = sqlx::query_as::<_, TagCount>(
        r#"
        SELECT tag, SUM(cnt)::BIGINT AS count
        FROM (
            SELECT unnest(tags) AS tag, 1 AS cnt
            FROM knowledge_items
            WHERE ($1::TEXT IS NULL OR project_id = $1)

            UNION ALL

            SELECT type AS tag, 1 AS cnt
            FROM episodes
            WHERE ($1::TEXT IS NULL OR project_id = $1)

            UNION ALL

            SELECT unnest(tags) AS tag, 1 AS cnt
            FROM procedures
            WHERE ($1::TEXT IS NULL OR project_id = $1)
        ) AS all_tags
        GROUP BY tag
        ORDER BY count DESC, tag ASC
        "#,
    )
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;

    let v = serde_json::to_value(tags)?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Rename a tag
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RenameTagBody {
    new_name: String,
}

#[derive(Serialize)]
struct UpdatedResponse {
    updated: u64,
}

async fn rename_tag(
    State(state): State<AppState>,
    Path(old_name): Path<String>,
    Json(body): Json<RenameTagBody>,
) -> Result<impl IntoResponse, ApiError> {
    let new_name = body.new_name.trim().to_string();

    if new_name.is_empty() {
        return Err(ApiError::BadRequest("new_name must not be empty".into()));
    }

    if old_name == new_name {
        return Ok((StatusCode::OK, Json(serde_json::json!({"updated": 0}))));
    }

    let mut total: u64 = 0;

    // Update knowledge_items tags
    let r = sqlx::query(
        r#"
        UPDATE knowledge_items
        SET tags = array_replace(tags, $1, $2),
            updated_at = now()
        WHERE $1 = ANY(tags)
        "#,
    )
    .bind(&old_name)
    .bind(&new_name)
    .execute(&state.pool)
    .await?;
    total += r.rows_affected();

    // Update procedures tags
    let r = sqlx::query(
        r#"
        UPDATE procedures
        SET tags = array_replace(tags, $1, $2),
            updated_at = now()
        WHERE $1 = ANY(tags)
        "#,
    )
    .bind(&old_name)
    .bind(&new_name)
    .execute(&state.pool)
    .await?;
    total += r.rows_affected();

    let v = serde_json::to_value(UpdatedResponse { updated: total })?;
    Ok((StatusCode::OK, Json(v)))
}

// ---------------------------------------------------------------------------
// Merge multiple tags into one
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct MergeTagsBody {
    source_tags: Vec<String>,
    target_tag: String,
}

async fn merge_tags(
    State(state): State<AppState>,
    Json(body): Json<MergeTagsBody>,
) -> Result<impl IntoResponse, ApiError> {
    let target = body.target_tag.trim().to_string();

    if target.is_empty() {
        return Err(ApiError::BadRequest("target_tag must not be empty".into()));
    }

    if body.source_tags.is_empty() {
        return Ok((StatusCode::OK, Json(serde_json::json!({"updated": 0}))));
    }

    let mut total: u64 = 0;

    for source in &body.source_tags {
        if source == &target {
            continue;
        }

        // Rename source -> target in knowledge_items
        let r = sqlx::query(
            r#"
            UPDATE knowledge_items
            SET tags = array_replace(tags, $1, $2),
                updated_at = now()
            WHERE $1 = ANY(tags)
            "#,
        )
        .bind(source)
        .bind(&target)
        .execute(&state.pool)
        .await?;
        total += r.rows_affected();

        // Rename source -> target in procedures
        let r = sqlx::query(
            r#"
            UPDATE procedures
            SET tags = array_replace(tags, $1, $2),
                updated_at = now()
            WHERE $1 = ANY(tags)
            "#,
        )
        .bind(source)
        .bind(&target)
        .execute(&state.pool)
        .await?;
        total += r.rows_affected();
    }

    // Deduplicate: remove duplicate target tags from arrays
    sqlx::query(
        r#"
        UPDATE knowledge_items
        SET tags = (SELECT ARRAY(SELECT DISTINCT unnest(tags)))
        WHERE $1 = ANY(tags)
          AND array_length(tags, 1) <> (SELECT count(DISTINCT t) FROM unnest(tags) AS t)
        "#,
    )
    .bind(&target)
    .execute(&state.pool)
    .await?;

    sqlx::query(
        r#"
        UPDATE procedures
        SET tags = (SELECT ARRAY(SELECT DISTINCT unnest(tags)))
        WHERE $1 = ANY(tags)
          AND array_length(tags, 1) <> (SELECT count(DISTINCT t) FROM unnest(tags) AS t)
        "#,
    )
    .bind(&target)
    .execute(&state.pool)
    .await?;

    let v = serde_json::to_value(UpdatedResponse { updated: total })?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tags_query_empty_defaults() {
        let q: ListTagsQuery = serde_json::from_str("{}").unwrap();
        assert!(q.project.is_none());
    }

    #[test]
    fn list_tags_query_with_project() {
        let q: ListTagsQuery = serde_json::from_str(r#"{"project": "my-project"}"#).unwrap();
        assert_eq!(q.project.as_deref(), Some("my-project"));
    }

    #[test]
    fn rename_tag_body_valid() {
        let body: RenameTagBody = serde_json::from_str(r#"{"new_name": "rust"}"#).unwrap();
        assert_eq!(body.new_name, "rust");
    }

    #[test]
    fn rename_tag_body_missing_field() {
        let result = serde_json::from_str::<RenameTagBody>("{}");
        assert!(result.is_err());
    }

    #[test]
    fn merge_tags_body_valid() {
        let body: MergeTagsBody =
            serde_json::from_str(r#"{"source_tags": ["a", "b"], "target_tag": "c"}"#).unwrap();
        assert_eq!(body.source_tags, vec!["a", "b"]);
        assert_eq!(body.target_tag, "c");
    }

    #[test]
    fn merge_tags_body_empty_source_tags() {
        let body: MergeTagsBody =
            serde_json::from_str(r#"{"source_tags": [], "target_tag": "c"}"#).unwrap();
        assert!(body.source_tags.is_empty());
        assert_eq!(body.target_tag, "c");
    }

    #[test]
    fn tag_count_serialization() {
        let tc = TagCount {
            tag: "rust".into(),
            count: 42,
        };
        let json = serde_json::to_value(&tc).unwrap();
        assert_eq!(json["tag"], "rust");
        assert_eq!(json["count"], 42);
    }

    #[test]
    fn updated_response_serialization() {
        let resp = UpdatedResponse { updated: 7 };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["updated"], 7);
    }
}
