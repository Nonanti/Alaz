//! Git commit ingestion: reads local git log and creates structured episodes.
//!
//! Runs `git log --since=<time> --format=... --stat` and parses each commit
//! into an Episode record. Used by hook stop to capture "what actually happened"
//! alongside the subjective transcript.

use std::path::Path;
use std::process::Command;

use alaz_core::Result;
use alaz_core::models::{CreateEpisode, Episode};
use alaz_db::repos::{EpisodeRepo, FileChange, GitActivityRepo};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// A parsed git commit with stat information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub timestamp: i64, // unix epoch seconds
    pub subject: String,
    pub body: String,
    pub files_changed: Vec<GitFileChange>,
    pub total_insertions: u32,
    pub total_deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileChange {
    pub path: String,
    pub change_type: String, // "add", "modify", "delete", "rename"
    pub insertions: u32,
    pub deletions: u32,
}

/// Walk up from `start` looking for a `.git` directory.
/// Returns the repo root if found.
pub fn find_repo_root(start: &Path) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Detect the project name from the repo root (uses directory name).
pub fn project_name_from_repo(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Run `git log --since=<timestamp>` and parse output into commits.
///
/// Only commits newer than `since_epoch` are returned.
/// Limited to `max_commits` to prevent ingesting entire history on first run.
pub fn read_commits_since(
    repo_root: &Path,
    since_epoch: i64,
    max_commits: usize,
) -> Result<Vec<GitCommit>> {
    // Use a unique record separator that won't appear in commit data
    let format = "%H%x1f%h%x1f%an%x1f%at%x1f%s%x1f%b%x1e";

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("log")
        .arg(format!("--since=@{since_epoch}"))
        .arg(format!("--max-count={max_commits}"))
        .arg(format!("--format={format}"))
        .arg("--name-status")
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            tracing::warn!(
                status = ?o.status,
                stderr = %String::from_utf8_lossy(&o.stderr),
                "git log command failed"
            );
            return Ok(vec![]);
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to execute git");
            return Ok(vec![]);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits = parse_git_log(&stdout);

    // Get shortstat for each commit in a second pass to get insertions/deletions
    let enriched = enrich_with_stats(repo_root, commits);

    Ok(enriched)
}

fn parse_git_log(text: &str) -> Vec<GitCommit> {
    let mut commits = Vec::new();

    for record in text.split('\x1e') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }

        // Split header and name-status section
        let (header, name_status) = match record.find('\n') {
            Some(pos) => (&record[..pos], &record[pos + 1..]),
            None => (record, ""),
        };

        let fields: Vec<&str> = header.split('\x1f').collect();
        if fields.len() < 5 {
            continue;
        }

        let hash = fields[0].to_string();
        let short_hash = fields[1].to_string();
        let author = fields[2].to_string();
        let timestamp: i64 = fields[3].parse().unwrap_or(0);
        let subject = fields[4].to_string();
        // Body may span until the name-status starts; it was field 5 in our format
        let body = if fields.len() > 5 {
            fields[5].trim().to_string()
        } else {
            String::new()
        };

        let files_changed = parse_name_status(name_status);

        commits.push(GitCommit {
            hash,
            short_hash,
            author,
            timestamp,
            subject,
            body,
            files_changed,
            total_insertions: 0,
            total_deletions: 0,
        });
    }

    commits
}

fn parse_name_status(text: &str) -> Vec<GitFileChange> {
    let mut files = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let status_char = parts[0].chars().next().unwrap_or('?');
        let change_type = match status_char {
            'A' => "add",
            'M' => "modify",
            'D' => "delete",
            'R' => "rename",
            'C' => "copy",
            _ => "modify",
        }
        .to_string();

        // For rename/copy, the path is the third field (new name)
        let path = if change_type == "rename" || change_type == "copy" {
            parts.get(2).map(|s| s.to_string()).unwrap_or_default()
        } else {
            parts[1].to_string()
        };

        if !path.is_empty() {
            files.push(GitFileChange {
                path,
                change_type,
                insertions: 0,
                deletions: 0,
            });
        }
    }
    files
}

/// Enrich commits with per-file insertion/deletion counts using `git show --numstat`.
fn enrich_with_stats(repo_root: &Path, mut commits: Vec<GitCommit>) -> Vec<GitCommit> {
    for commit in &mut commits {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("show")
            .arg("--numstat")
            .arg("--format=")
            .arg(&commit.hash)
            .output();

        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut total_ins = 0u32;
        let mut total_del = 0u32;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let ins: u32 = parts[0].parse().unwrap_or(0);
            let del: u32 = parts[1].parse().unwrap_or(0);
            let path = parts[2];

            total_ins += ins;
            total_del += del;

            // Update the matching file change
            if let Some(file) = commit.files_changed.iter_mut().find(|f| f.path == path) {
                file.insertions = ins;
                file.deletions = del;
            }
        }

        commit.total_insertions = total_ins;
        commit.total_deletions = total_del;
    }

    commits
}

/// Format a commit as episode content (title + structured body).
pub fn format_commit_content(commit: &GitCommit) -> String {
    let files_summary = commit
        .files_changed
        .iter()
        .map(|f| {
            if f.insertions > 0 || f.deletions > 0 {
                format!(
                    "- `{}` ({}) +{} -{}",
                    f.path, f.change_type, f.insertions, f.deletions
                )
            } else {
                format!("- `{}` ({})", f.path, f.change_type)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let body = if commit.body.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", commit.body)
    };

    format!(
        "**Commit {}** by {}{}\n\n**Files** ({} changed, +{} -{}):\n{}",
        commit.short_hash,
        commit.author,
        body,
        commit.files_changed.len(),
        commit.total_insertions,
        commit.total_deletions,
        files_summary,
    )
}

/// Classify commit severity from the subject line keywords.
pub fn classify_severity(subject: &str) -> &'static str {
    let lower = subject.to_lowercase();
    let is_meaningful = lower.starts_with("fix")
        || lower.contains("bug")
        || lower.contains("critical")
        || lower.starts_with("feat")
        || lower.starts_with("refactor");
    if is_meaningful { "medium" } else { "low" }
}

/// Summary of git ingestion results.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GitIngestSummary {
    pub commits_read: usize,
    pub episodes_created: usize,
    pub duplicates_skipped: usize,
    pub files_recorded: usize,
    pub errors: Vec<String>,
}

/// Ingest commits from a repo since `since_epoch` into the database.
///
/// - Reads git log via `read_commits_since`
/// - Deduplicates: skips commits whose hash already exists in episodes source_metadata
/// - Creates an Episode per commit with `kind: "git_commit"`, action = hash,
///   content = formatted commit summary, files in related_files array
/// - Also records the commit in git_activity table (for hot_files/coupled_files analysis)
pub async fn ingest_commits_for_session(
    pool: &PgPool,
    repo_root: &Path,
    since_epoch: i64,
    project_id: Option<&str>,
    session_id: Option<&str>,
    max_commits: usize,
) -> Result<GitIngestSummary> {
    let mut summary = GitIngestSummary::default();

    let commits = match read_commits_since(repo_root, since_epoch, max_commits) {
        Ok(c) => c,
        Err(e) => {
            summary.errors.push(format!("git log failed: {e}"));
            return Ok(summary);
        }
    };

    summary.commits_read = commits.len();
    if commits.is_empty() {
        return Ok(summary);
    }

    // Fetch existing commit hashes to dedup
    let existing_hashes = fetch_existing_commit_hashes(pool, &commits).await;

    for commit in &commits {
        if existing_hashes.contains(&commit.hash) {
            summary.duplicates_skipped += 1;
            continue;
        }

        // Record file changes in git_activity table
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

        match GitActivityRepo::record_commit(
            pool,
            project_id,
            &commit.hash,
            &commit.subject,
            &file_changes,
        )
        .await
        {
            Ok(n) => summary.files_recorded += n,
            Err(e) => summary.errors.push(format!("git_activity record: {e}")),
        }

        // Create Episode
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

        let input = CreateEpisode {
            title: commit.subject.clone(),
            content: format_commit_content(commit),
            kind: Some("git_commit".into()),
            severity: Some(classify_severity(&commit.subject).into()),
            action: Some(commit.short_hash.clone()),
            outcome: Some("committed".into()),
            outcome_score: None,
            resolved: Some(true),
            who_cues: Some(vec![commit.author.clone()]),
            what_cues: Some(vec![commit.subject.clone()]),
            where_cues: Some(related_files.clone()),
            when_cues: Some(vec![format_unix_ts(commit.timestamp)]),
            why_cues: None,
            related_files: Some(related_files),
            project: None,
            source: Some("git".into()),
            source_metadata: Some(source_meta),
        };
        // session_id is not in CreateEpisode — link via graph edge below if needed
        let _ = session_id;

        match EpisodeRepo::create(pool, &input, project_id).await {
            Ok(_) => summary.episodes_created += 1,
            Err(e) => summary.errors.push(format!("episode create: {e}")),
        }
    }

    Ok(summary)
}

/// Fetch commit hashes already stored as episodes (via source_metadata->>'git_commit_hash').
async fn fetch_existing_commit_hashes(
    pool: &PgPool,
    commits: &[GitCommit],
) -> std::collections::HashSet<String> {
    let hashes: Vec<String> = commits.iter().map(|c| c.hash.clone()).collect();
    let rows: Result<Vec<(String,)>> = sqlx::query_as(
        "SELECT source_metadata->>'git_commit_hash' FROM episodes \
         WHERE source = 'git' \
         AND source_metadata->>'git_commit_hash' = ANY($1)",
    )
    .bind(&hashes)
    .fetch_all(pool)
    .await
    .map_err(Into::into);

    match rows {
        Ok(rows) => rows.into_iter().map(|(h,)| h).collect(),
        Err(_) => std::collections::HashSet::new(),
    }
}

fn format_unix_ts(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Suppress unused warning for Episode (re-exported type).
#[allow(dead_code)]
fn _unused_episode_marker(_: Episode) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_status_add() {
        let text = "A\tsrc/new.rs";
        let files = parse_name_status(text);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/new.rs");
        assert_eq!(files[0].change_type, "add");
    }

    #[test]
    fn parse_name_status_rename() {
        let text = "R100\told.rs\tnew.rs";
        let files = parse_name_status(text);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.rs");
        assert_eq!(files[0].change_type, "rename");
    }

    #[test]
    fn parse_name_status_multiple() {
        let text = "A\tsrc/a.rs\nM\tsrc/b.rs\nD\tsrc/c.rs";
        let files = parse_name_status(text);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].change_type, "add");
        assert_eq!(files[1].change_type, "modify");
        assert_eq!(files[2].change_type, "delete");
    }

    #[test]
    fn classify_severity_fix() {
        assert_eq!(classify_severity("fix: auth bug"), "medium");
        assert_eq!(classify_severity("Fix critical panic"), "medium");
    }

    #[test]
    fn classify_severity_docs() {
        assert_eq!(classify_severity("docs: update README"), "low");
        assert_eq!(classify_severity("chore: bump deps"), "low");
    }

    #[test]
    fn format_commit_content_basic() {
        let commit = GitCommit {
            hash: "abc123def".to_string(),
            short_hash: "abc123d".to_string(),
            author: "nonantiy".to_string(),
            timestamp: 1_700_000_000,
            subject: "fix: auth".to_string(),
            body: "Body details".to_string(),
            files_changed: vec![GitFileChange {
                path: "src/auth.rs".to_string(),
                change_type: "modify".to_string(),
                insertions: 10,
                deletions: 3,
            }],
            total_insertions: 10,
            total_deletions: 3,
        };
        let content = format_commit_content(&commit);
        assert!(content.contains("abc123d"));
        assert!(content.contains("src/auth.rs"));
        assert!(content.contains("+10"));
        assert!(content.contains("-3"));
    }
}
