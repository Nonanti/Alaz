use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};

use alaz_db::repos::{FileChange, GitActivityRepo, ProjectRepo};
use alaz_intel::{SessionLearner, detect_domain};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/git/ingest", post(ingest_git))
        .with_state(state)
}

/// A single file change from a git diff.
#[derive(Debug, Deserialize)]
struct GitFileChange {
    path: String,
    /// "add", "modify", "delete", "rename"
    change_type: String,
    #[serde(default)]
    lines_added: i32,
    #[serde(default)]
    lines_removed: i32,
}

#[derive(Debug, Deserialize)]
struct GitIngestRequest {
    /// The project/repo name (e.g., "Alaz").
    project: String,
    /// Full commit hash.
    commit_hash: String,
    /// Commit message.
    commit_message: String,
    /// List of files changed in this commit.
    files: Vec<GitFileChange>,
    /// Optional: full diff content for LLM knowledge extraction.
    diff: Option<String>,
}

#[derive(Serialize)]
struct GitIngestResponse {
    /// Number of file changes recorded.
    files_recorded: usize,
    /// Number of knowledge items extracted from the diff (if provided).
    items_extracted: usize,
}

async fn ingest_git(
    State(state): State<AppState>,
    Json(body): Json<GitIngestRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Resolve project
    let project = ProjectRepo::get_or_create(&state.pool, &body.project, None).await?;
    let project_id = project.id;

    // Record file changes
    let file_changes: Vec<FileChange> = body
        .files
        .iter()
        .map(|f| FileChange {
            path: f.path.clone(),
            change_type: f.change_type.clone(),
            lines_added: f.lines_added,
            lines_removed: f.lines_removed,
        })
        .collect();

    let files_recorded = GitActivityRepo::record_commit(
        &state.pool,
        Some(&project_id),
        &body.commit_hash,
        &body.commit_message,
        &file_changes,
    )
    .await?;

    // If a diff is provided, run knowledge extraction
    let mut items_extracted = 0;
    if let Some(ref diff) = body.diff {
        // Build context: commit message + file list + diff
        let file_list: String = body
            .files
            .iter()
            .map(|f| format!("  {} {}", f.change_type, f.path))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
            "# Git Commit: {}\n\n## Message\n{}\n\n## Changed Files\n{}\n\n## Diff\n```\n{}\n```",
            &body.commit_hash[..8.min(body.commit_hash.len())],
            body.commit_message,
            file_list,
            truncate_diff(diff, 24_000),
        );

        let domain = detect_domain(&content);
        let learner = SessionLearner::new(
            state.pool.clone(),
            state.llm.clone(),
            state.embedding.clone(),
            state.qdrant.clone(),
        );

        match learner
            .learn_from_content(&content, "git", domain, Some(&project_id))
            .await
        {
            Ok(summary) => {
                items_extracted = summary.patterns_saved
                    + summary.episodes_saved
                    + summary.procedures_saved
                    + summary.memories_saved;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    commit = %body.commit_hash,
                    "git ingest: knowledge extraction failed (file changes still recorded)"
                );
            }
        }
    }

    Ok((
        StatusCode::OK,
        Json(GitIngestResponse {
            files_recorded,
            items_extracted,
        }),
    ))
}

/// Truncate a diff to approximately `max_bytes` at a line boundary.
fn truncate_diff(diff: &str, max_bytes: usize) -> &str {
    if diff.len() <= max_bytes {
        return diff;
    }
    // Find the last newline before max_bytes
    match diff[..max_bytes].rfind('\n') {
        Some(pos) => &diff[..pos],
        None => &diff[..max_bytes],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_diff_short() {
        assert_eq!(truncate_diff("short", 100), "short");
    }

    #[test]
    fn truncate_diff_at_line_boundary() {
        let diff = "line1\nline2\nline3\nline4";
        let result = truncate_diff(diff, 12);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn truncate_diff_no_newline() {
        let diff = "a".repeat(100);
        let result = truncate_diff(&diff, 50);
        assert_eq!(result.len(), 50);
    }
}
