use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alaz_db::repos::ProjectRepo;
use alaz_intel::{ContentIngester, SessionLearner, detect_domain};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ingest", post(ingest))
        .route("/ingest/url", post(ingest_url))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct IngestRequest {
    /// The main content (text, markdown, transcript, etc.)
    content: String,
    /// Source identifier: "claude_code", "mobile_note", "web_clip", "voice_memo", "photo", "manual"
    source: String,
    /// Content type: "text", "markdown", "transcript", "url", "image_description"
    content_type: Option<String>,
    /// Optional title
    title: Option<String>,
    /// Project name
    project: Option<String>,
    /// Tags
    tags: Option<Vec<String>>,
    /// Source-specific metadata (url, device_id, etc.)
    metadata: Option<Value>,
}

#[derive(Serialize)]
struct IngestResponse {
    id: String,
    items_extracted: usize,
    source: String,
    domain: String,
}

async fn ingest(
    State(state): State<AppState>,
    Json(body): Json<IngestRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Resolve project
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    // Detect content domain
    let domain = detect_domain(&body.content);

    // Build the learner
    let learner = SessionLearner::new(
        state.pool.clone(),
        state.llm.clone(),
        state.embedding.clone(),
        state.qdrant.clone(),
    );

    // If there's a title, prepend it to content for better extraction
    let content = if let Some(ref title) = body.title {
        format!("# {title}\n\n{}", body.content)
    } else {
        body.content.clone()
    };

    // Run the learning pipeline
    let summary = learner
        .learn_from_content(&content, &body.source, domain, project_id.as_deref())
        .await?;

    let total = summary.patterns_saved
        + summary.episodes_saved
        + summary.procedures_saved
        + summary.memories_saved;

    let response = IngestResponse {
        id: cuid2::create_id(),
        items_extracted: total,
        source: body.source,
        domain: domain.to_string(),
    };
    Ok((StatusCode::OK, Json(response)))
}

// ---------------------------------------------------------------------------
// URL ingestion endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IngestUrlRequest {
    url: String,
    project: Option<String>,
    tags: Option<Vec<String>>,
    title: Option<String>,
}

#[derive(Serialize)]
struct IngestUrlResponse {
    knowledge_id: String,
    title: String,
    content_length: usize,
    source_url: String,
}

async fn ingest_url(
    State(state): State<AppState>,
    Json(body): Json<IngestUrlRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let ingester = ContentIngester::new(state.pool.clone());

    let req = alaz_intel::ingest::IngestRequest {
        url: body.url,
        project: body.project,
        tags: body.tags,
        title_override: body.title,
    };

    let result = ingester.ingest_url(req).await?;

    let response = IngestUrlResponse {
        knowledge_id: result.knowledge_id,
        title: result.title,
        content_length: result.content_length,
        source_url: result.source_url,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ingest_request_minimal() {
        let json = json!({
            "content": "hello world",
            "source": "manual"
        });
        let req: IngestRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.content, "hello world");
        assert_eq!(req.source, "manual");
        assert!(req.content_type.is_none());
        assert!(req.title.is_none());
        assert!(req.project.is_none());
        assert!(req.tags.is_none());
        assert!(req.metadata.is_none());
    }

    #[test]
    fn ingest_request_full() {
        let json = json!({
            "content": "# Rust patterns",
            "source": "web_clip",
            "content_type": "markdown",
            "title": "Rust Tips",
            "project": "alaz",
            "tags": ["rust", "patterns"],
            "metadata": {"url": "https://example.com"}
        });
        let req: IngestRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.source, "web_clip");
        assert_eq!(req.content_type.as_deref(), Some("markdown"));
        assert_eq!(req.tags.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn ingest_url_request_minimal() {
        let json = json!({ "url": "https://example.com" });
        let req: IngestUrlRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.url, "https://example.com");
        assert!(req.project.is_none());
        assert!(req.tags.is_none());
        assert!(req.title.is_none());
    }

    #[test]
    fn ingest_response_serialization() {
        let resp = IngestResponse {
            id: "abc123".to_string(),
            items_extracted: 5,
            source: "manual".to_string(),
            domain: "coding".to_string(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], "abc123");
        assert_eq!(v["items_extracted"], 5);
        assert_eq!(v["source"], "manual");
        assert_eq!(v["domain"], "coding");
    }

    #[test]
    fn ingest_url_response_serialization() {
        let resp = IngestUrlResponse {
            knowledge_id: "kid_001".to_string(),
            title: "My Article".to_string(),
            content_length: 1024,
            source_url: "https://example.com/article".to_string(),
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["knowledge_id"], "kid_001");
        assert_eq!(v["title"], "My Article");
        assert_eq!(v["content_length"], 1024);
        assert_eq!(v["source_url"], "https://example.com/article");
    }
}
