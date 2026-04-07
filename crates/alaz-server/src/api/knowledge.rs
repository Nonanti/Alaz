use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};

use alaz_core::models::*;
use alaz_db::repos::{KnowledgeRepo, ProjectRepo};

use crate::error::ApiError;
use crate::state::AppState;

const BULK_MAX_ITEMS: usize = 100;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/knowledge", post(create_knowledge))
        .route("/knowledge/bulk", post(bulk_create_knowledge))
        .route("/knowledge/bulk", delete(bulk_delete_knowledge))
        .route("/knowledge/{id}", get(get_knowledge))
        .route("/knowledge/{id}", put(update_knowledge))
        .route("/knowledge/{id}", delete(delete_knowledge))
        .route("/knowledge/{id}/usage", post(record_usage))
        .route("/knowledge", get(list_knowledge))
        .with_state(state)
}

#[derive(Deserialize)]
struct CreateBody {
    title: String,
    content: String,
    description: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    language: Option<String>,
    file_path: Option<String>,
    project: Option<String>,
    tags: Option<Vec<String>>,
}

async fn create_knowledge(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let input = CreateKnowledge {
        title: body.title,
        content: body.content,
        description: body.description,
        kind: body.kind,
        language: body.language,
        file_path: body.file_path,
        project: body.project,
        tags: body.tags,
        valid_from: None,
        valid_until: None,
        source: None,
        source_metadata: None,
    };

    let item = KnowledgeRepo::create(&state.pool, &input, project_id.as_deref()).await?;
    let v = serde_json::to_value(item)?;
    Ok((StatusCode::CREATED, Json(v)))
}

// --- Bulk operations ---

#[derive(Deserialize)]
struct BulkCreateBody {
    items: Vec<CreateBody>,
}

#[derive(Serialize)]
struct BulkCreateResponse {
    ids: Vec<String>,
}

async fn bulk_create_knowledge(
    State(state): State<AppState>,
    Json(body): Json<BulkCreateBody>,
) -> Result<impl IntoResponse, ApiError> {
    if body.items.is_empty() {
        return Err(ApiError::BadRequest("items array must not be empty".into()));
    }
    if body.items.len() > BULK_MAX_ITEMS {
        return Err(ApiError::BadRequest(format!(
            "too many items: max {BULK_MAX_ITEMS} per request"
        )));
    }

    let mut tx = state.pool.begin().await?;
    let mut ids = Vec::with_capacity(body.items.len());

    for item in &body.items {
        // NOTE: Project resolution runs outside the transaction because
        // `ProjectRepo::get_or_create` takes `&PgPool`. Projects are idempotent
        // (ON CONFLICT DO UPDATE), so an orphaned project from a rolled-back
        // transaction is harmless.
        // TODO: Refactor repos to accept `sqlx::Executor` for full transactional consistency.
        let project_id = if let Some(ref name) = item.project {
            ProjectRepo::get_or_create(&state.pool, name, None)
                .await
                .ok()
                .map(|p| p.id)
        } else {
            None
        };

        let id = cuid2::create_id();
        let kind = item.kind.as_deref().unwrap_or("artifact");
        let tags: &[String] = item.tags.as_deref().unwrap_or(&[]);

        sqlx::query(
            r#"
            INSERT INTO knowledge_items (id, title, content, description, type, language, file_path, project_id, tags)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&id)
        .bind(&item.title)
        .bind(&item.content)
        .bind(&item.description)
        .bind(kind)
        .bind(&item.language)
        .bind(&item.file_path)
        .bind(project_id.as_deref())
        .bind(tags)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert failed: {e}")))?;

        ids.push(id);
    }

    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(BulkCreateResponse { ids })))
}

#[derive(Deserialize)]
struct BulkDeleteBody {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct BulkDeleteResponse {
    deleted: u64,
}

async fn bulk_delete_knowledge(
    State(state): State<AppState>,
    Json(body): Json<BulkDeleteBody>,
) -> Result<impl IntoResponse, ApiError> {
    if body.ids.is_empty() {
        return Err(ApiError::BadRequest("ids array must not be empty".into()));
    }
    if body.ids.len() > BULK_MAX_ITEMS {
        return Err(ApiError::BadRequest(format!(
            "too many ids: max {BULK_MAX_ITEMS} per request"
        )));
    }

    let count = KnowledgeRepo::bulk_delete(&state.pool, &body.ids).await?;
    Ok((StatusCode::OK, Json(BulkDeleteResponse { deleted: count })))
}

async fn get_knowledge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let item = KnowledgeRepo::get(&state.pool, &id).await?;
    let v = serde_json::to_value(item)?;
    Ok((StatusCode::OK, Json(v)))
}

async fn update_knowledge(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateKnowledge>,
) -> Result<impl IntoResponse, ApiError> {
    let item = KnowledgeRepo::update(&state.pool, &id, &body).await?;
    let v = serde_json::to_value(item)?;
    Ok((StatusCode::OK, Json(v)))
}

async fn delete_knowledge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    KnowledgeRepo::delete(&state.pool, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct UsageBody {
    success: bool,
}

async fn record_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UsageBody>,
) -> Result<impl IntoResponse, ApiError> {
    KnowledgeRepo::record_usage(&state.pool, &id, body.success).await?;
    let item = KnowledgeRepo::get(&state.pool, &id).await?;
    let v = serde_json::to_value(item)?;
    Ok((StatusCode::OK, Json(v)))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(rename = "type")]
    kind: Option<String>,
    language: Option<String>,
    project: Option<String>,
    tag: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_knowledge(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
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

    let filter = ListKnowledgeFilter {
        project: project_id,
        kind: q.kind,
        language: q.language,
        tag: q.tag,
        limit: q.limit,
        offset: q.offset,
    };

    let items = KnowledgeRepo::list(&state.pool, &filter).await?;
    let v = serde_json::to_value(items)?;
    Ok((StatusCode::OK, Json(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_body_minimal() {
        let body: CreateBody = serde_json::from_value(json!({
            "title": "Hello",
            "content": "World"
        }))
        .unwrap();
        assert_eq!(body.title, "Hello");
        assert_eq!(body.content, "World");
        assert!(body.description.is_none());
        assert!(body.kind.is_none());
        assert!(body.language.is_none());
        assert!(body.file_path.is_none());
        assert!(body.project.is_none());
        assert!(body.tags.is_none());
    }

    #[test]
    fn create_body_full() {
        let body: CreateBody = serde_json::from_value(json!({
            "title": "Pattern",
            "content": "Use builder pattern",
            "description": "A useful pattern",
            "type": "pattern",
            "language": "rust",
            "file_path": "src/lib.rs",
            "project": "alaz",
            "tags": ["rust", "pattern"]
        }))
        .unwrap();
        assert_eq!(body.title, "Pattern");
        assert_eq!(body.content, "Use builder pattern");
        assert_eq!(body.description.as_deref(), Some("A useful pattern"));
        assert_eq!(body.kind.as_deref(), Some("pattern"));
        assert_eq!(body.language.as_deref(), Some("rust"));
        assert_eq!(body.file_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(body.project.as_deref(), Some("alaz"));
        assert_eq!(
            body.tags.as_deref(),
            Some(&["rust".to_string(), "pattern".to_string()][..])
        );
    }

    #[test]
    fn bulk_create_body_deserialize() {
        let body: BulkCreateBody = serde_json::from_value(json!({
            "items": [
                { "title": "A", "content": "a" },
                { "title": "B", "content": "b", "type": "snippet" }
            ]
        }))
        .unwrap();
        assert_eq!(body.items.len(), 2);
        assert_eq!(body.items[0].title, "A");
        assert_eq!(body.items[1].kind.as_deref(), Some("snippet"));
    }

    #[test]
    fn bulk_delete_body_deserialize() {
        let body: BulkDeleteBody = serde_json::from_value(json!({
            "ids": ["id1", "id2", "id3"]
        }))
        .unwrap();
        assert_eq!(body.ids, vec!["id1", "id2", "id3"]);
    }

    #[test]
    fn usage_body_deserialize() {
        let body: UsageBody = serde_json::from_value(json!({ "success": true })).unwrap();
        assert!(body.success);

        let body: UsageBody = serde_json::from_value(json!({ "success": false })).unwrap();
        assert!(!body.success);
    }

    #[test]
    fn list_query_defaults() {
        let q: ListQuery = serde_json::from_value(json!({})).unwrap();
        assert!(q.kind.is_none());
        assert!(q.language.is_none());
        assert!(q.project.is_none());
        assert!(q.tag.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn list_query_all_fields() {
        let q: ListQuery = serde_json::from_value(json!({
            "type": "pattern",
            "language": "rust",
            "project": "alaz",
            "tag": "cache",
            "limit": 10,
            "offset": 20
        }))
        .unwrap();
        assert_eq!(q.kind.as_deref(), Some("pattern"));
        assert_eq!(q.language.as_deref(), Some("rust"));
        assert_eq!(q.project.as_deref(), Some("alaz"));
        assert_eq!(q.tag.as_deref(), Some("cache"));
        assert_eq!(q.limit, Some(10));
        assert_eq!(q.offset, Some(20));
    }

    #[test]
    fn bulk_create_response_serialize() {
        let resp = BulkCreateResponse {
            ids: vec!["a1".into(), "b2".into()],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v, json!({ "ids": ["a1", "b2"] }));
    }

    #[test]
    fn bulk_delete_response_serialize() {
        let resp = BulkDeleteResponse { deleted: 5 };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v, json!({ "deleted": 5 }));
    }

    #[test]
    fn bulk_max_items_value() {
        assert_eq!(BULK_MAX_ITEMS, 100);
    }
}
