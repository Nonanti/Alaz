use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};

use alaz_db::repos::{FileChange, GitActivityRepo, ProjectRepo};
use alaz_intel::{SessionLearner, detect_domain};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/git/ingest", post(ingest_git))
        .route("/git/ingest-commits", post(ingest_commits_bulk))
        .with_state(state)
}

/// Bulk commit ingestion from a pre-parsed list (CLI hook reads git log locally
/// and pushes commits here, since the server doesn't have access to the client's
/// filesystem).
#[derive(Debug, Deserialize)]
struct BulkCommitsRequest {
    /// Project name (resolved to project_id)
    project: Option<String>,
    /// Optional session_id to link commits to
    session_id: Option<String>,
    /// Pre-parsed commits from the client
    commits: Vec<alaz_intel::git_ingest::GitCommit>,
}

#[derive(Serialize)]
struct BulkCommitsResponse {
    commits_received: usize,
    episodes_created: usize,
    duplicates_skipped: usize,
    files_recorded: usize,
}

async fn ingest_commits_bulk(
    State(state): State<AppState>,
    Json(body): Json<BulkCommitsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let project_id = if let Some(ref name) = body.project {
        ProjectRepo::get_or_create(&state.pool, name, None)
            .await
            .ok()
            .map(|p| p.id)
    } else {
        None
    };

    let mut summary = alaz_intel::git_ingest::GitIngestSummary {
        commits_read: body.commits.len(),
        ..Default::default()
    };

    // Dedup: skip commits we already have
    let existing = fetch_existing_hashes(&state.pool, &body.commits).await;

    for commit in &body.commits {
        if existing.contains(&commit.hash) {
            summary.duplicates_skipped += 1;
            continue;
        }

        // Record in git_activity
        let file_changes: Vec<FileChange> = commit
            .files_changed
            .iter()
            .map(|f| FileChange {
                path: f.path.clone(),
                change_type: f.change_type.clone(),
                lines_added: f.insertions as i32,
                lines_removed: f.deletions as i32,
            })
            .collect();

        if let Ok(n) = alaz_db::repos::GitActivityRepo::record_commit(
            &state.pool,
            project_id.as_deref(),
            &commit.hash,
            &commit.subject,
            &file_changes,
        )
        .await
        {
            summary.files_recorded += n;
        }

        // Create episode
        let related_files: Vec<String> = commit
            .files_changed
            .iter()
            .map(|f| f.path.clone())
            .collect();
        let source_meta = serde_json::json!({
            "git_commit_hash": commit.hash,
            "short_hash": commit.short_hash,
            "author": commit.author,
            "timestamp": commit.timestamp,
            "insertions": commit.total_insertions,
            "deletions": commit.total_deletions,
            "file_count": commit.files_changed.len(),
        });
        let when_ts = chrono::DateTime::<chrono::Utc>::from_timestamp(commit.timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_default();

        let input = alaz_core::models::CreateEpisode {
            title: commit.subject.clone(),
            content: alaz_intel::git_ingest::format_commit_content(commit),
            kind: Some("git_commit".into()),
            severity: Some(alaz_intel::git_ingest::classify_severity(&commit.subject).into()),
            action: Some(commit.short_hash.clone()),
            outcome: Some("committed".into()),
            outcome_score: None,
            resolved: Some(true),
            who_cues: Some(vec![commit.author.clone()]),
            what_cues: Some(vec![commit.subject.clone()]),
            where_cues: Some(related_files.clone()),
            when_cues: Some(vec![when_ts]),
            why_cues: None,
            related_files: Some(related_files),
            project: None,
            source: Some("git".into()),
            source_metadata: Some(source_meta),
        };

        if alaz_db::repos::EpisodeRepo::create(&state.pool, &input, project_id.as_deref())
            .await
            .is_ok()
        {
            summary.episodes_created += 1;
        }
    }

    let _ = body.session_id; // reserved for future graph edge linking

    Ok((
        StatusCode::OK,
        Json(BulkCommitsResponse {
            commits_received: summary.commits_read,
            episodes_created: summary.episodes_created,
            duplicates_skipped: summary.duplicates_skipped,
            files_recorded: summary.files_recorded,
        }),
    ))
}

async fn fetch_existing_hashes(
    pool: &sqlx::PgPool,
    commits: &[alaz_intel::git_ingest::GitCommit],
) -> std::collections::HashSet<String> {
    let hashes: Vec<String> = commits.iter().map(|c| c.hash.clone()).collect();
    let rows: std::result::Result<Vec<(String,)>, _> = sqlx::query_as(
        "SELECT source_metadata->>'git_commit_hash' FROM episodes \
         WHERE source = 'git' \
         AND source_metadata->>'git_commit_hash' = ANY($1)",
    )
    .bind(&hashes)
    .fetch_all(pool)
    .await;
    rows.map(|r| r.into_iter().map(|(h,)| h).collect())
        .unwrap_or_default()
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
