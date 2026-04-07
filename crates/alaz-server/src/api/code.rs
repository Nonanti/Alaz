use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use alaz_db::repos::{CodeSymbolRepo, GitActivityRepo, ProjectRepo};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/code/index", post(index_files))
        .route("/code/impact", post(impact_analysis))
        .route("/code/hot-files", get(hot_files))
        .route("/code/coupling", get(coupled_files))
        .with_state(state)
}

// --- Index ---

#[derive(Debug, Deserialize)]
struct IndexFile {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct IndexRequest {
    project: String,
    files: Vec<IndexFile>,
}

#[derive(Serialize)]
struct IndexResponse {
    files_indexed: usize,
    symbols_extracted: usize,
}

async fn index_files(
    State(state): State<AppState>,
    Json(body): Json<IndexRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let project = ProjectRepo::get_or_create(&state.pool, &body.project, None).await?;

    let files: Vec<(&str, &str)> = body
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.content.as_str()))
        .collect();

    let symbols =
        alaz_intel::code_index::index_files(&state.pool, Some(&project.id), &files).await?;

    Ok((
        StatusCode::OK,
        Json(IndexResponse {
            files_indexed: body.files.len(),
            symbols_extracted: symbols,
        }),
    ))
}

// --- Impact Analysis ---

#[derive(Debug, Deserialize)]
struct ImpactRequest {
    project: Option<String>,
    symbol_name: String,
}

#[derive(Serialize)]
struct ImpactResponse {
    symbol_name: String,
    definitions: Vec<SymbolInfo>,
    callers: Vec<SymbolInfo>,
}

#[derive(Serialize)]
struct SymbolInfo {
    file_path: String,
    symbol_name: String,
    symbol_type: String,
    signature: Option<String>,
    line_number: i32,
    parent: Option<String>,
}

async fn impact_analysis(
    State(state): State<AppState>,
    Json(body): Json<ImpactRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await?
            .map(|p| p.id)
    } else {
        None
    };

    let definitions =
        CodeSymbolRepo::get_by_name(&state.pool, project_id.as_deref(), &body.symbol_name).await?;
    let callers =
        CodeSymbolRepo::find_callers(&state.pool, project_id.as_deref(), &body.symbol_name).await?;

    let to_info = |s: alaz_core::models::CodeSymbol| SymbolInfo {
        file_path: s.file_path,
        symbol_name: s.symbol_name,
        symbol_type: s.symbol_type,
        signature: s.signature,
        line_number: s.line_number,
        parent: s.parent_symbol,
    };

    Ok((
        StatusCode::OK,
        Json(ImpactResponse {
            symbol_name: body.symbol_name,
            definitions: definitions.into_iter().map(to_info).collect(),
            callers: callers.into_iter().map(to_info).collect(),
        }),
    ))
}

// --- Hot Files ---

#[derive(Debug, Deserialize)]
struct HotFilesQuery {
    project: Option<String>,
    days: Option<i32>,
    limit: Option<i64>,
}

async fn hot_files(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HotFilesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await?
            .map(|p| p.id)
    } else {
        None
    };

    let results = GitActivityRepo::hot_files(
        &state.pool,
        project_id.as_deref(),
        q.days.unwrap_or(30),
        q.limit.unwrap_or(20),
    )
    .await?;

    Ok((StatusCode::OK, Json(results)))
}

// --- Coupling ---

#[derive(Debug, Deserialize)]
struct CouplingQuery {
    project: Option<String>,
    days: Option<i32>,
    min_co_changes: Option<i64>,
    limit: Option<i64>,
}

async fn coupled_files(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<CouplingQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = q.project {
        ProjectRepo::get_by_name(&state.pool, name)
            .await?
            .map(|p| p.id)
    } else {
        None
    };

    let results = GitActivityRepo::coupled_files(
        &state.pool,
        project_id.as_deref(),
        q.days.unwrap_or(30),
        q.min_co_changes.unwrap_or(3),
        q.limit.unwrap_or(20),
    )
    .await?;

    Ok((StatusCode::OK, Json(results)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_request_valid() {
        let json = json!({
            "project": "alaz",
            "files": [
                { "path": "src/main.rs", "content": "fn main() {}" },
                { "path": "src/lib.rs", "content": "pub mod api;" }
            ]
        });
        let req: IndexRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.project, "alaz");
        assert_eq!(req.files.len(), 2);
        assert_eq!(req.files[0].path, "src/main.rs");
        assert_eq!(req.files[1].content, "pub mod api;");
    }

    #[test]
    fn index_request_empty_files() {
        let json = json!({ "project": "test", "files": [] });
        let req: IndexRequest = serde_json::from_value(json).unwrap();
        assert!(req.files.is_empty());
    }

    #[test]
    fn impact_request_minimal() {
        let json = json!({ "symbol_name": "handle_request" });
        let req: ImpactRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.symbol_name, "handle_request");
        assert!(req.project.is_none());
    }

    #[test]
    fn hot_files_query_defaults() {
        let json = json!({});
        let q: HotFilesQuery = serde_json::from_value(json).unwrap();
        assert!(q.project.is_none());
        assert!(q.days.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn coupling_query_defaults() {
        let json = json!({});
        let q: CouplingQuery = serde_json::from_value(json).unwrap();
        assert!(q.project.is_none());
        assert!(q.days.is_none());
        assert!(q.min_co_changes.is_none());
        assert!(q.limit.is_none());
    }

    #[test]
    fn index_response_serialization() {
        let resp = IndexResponse {
            files_indexed: 10,
            symbols_extracted: 42,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["files_indexed"], 10);
        assert_eq!(v["symbols_extracted"], 42);
    }

    #[test]
    fn symbol_info_serialization() {
        let info = SymbolInfo {
            file_path: "src/api/code.rs".to_string(),
            symbol_name: "index_files".to_string(),
            symbol_type: "function".to_string(),
            signature: Some("async fn index_files(...)".to_string()),
            line_number: 42,
            parent: None,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["file_path"], "src/api/code.rs");
        assert_eq!(v["symbol_name"], "index_files");
        assert_eq!(v["line_number"], 42);
        assert!(v["signature"].is_string());
        assert!(v["parent"].is_null());
    }

    #[test]
    fn impact_response_serialization() {
        let resp = ImpactResponse {
            symbol_name: "foo".to_string(),
            definitions: vec![],
            callers: vec![SymbolInfo {
                file_path: "bar.rs".to_string(),
                symbol_name: "call_foo".to_string(),
                symbol_type: "function".to_string(),
                signature: None,
                line_number: 10,
                parent: Some("mod_bar".to_string()),
            }],
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["symbol_name"], "foo");
        assert!(v["definitions"].as_array().unwrap().is_empty());
        assert_eq!(v["callers"].as_array().unwrap().len(), 1);
        assert_eq!(v["callers"][0]["parent"], "mod_bar");
    }
}
