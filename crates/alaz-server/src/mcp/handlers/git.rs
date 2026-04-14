use alaz_core::models::ListEpisodesFilter;
use alaz_db::repos::GitActivityRepo;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

/// Timeline of recent git commits as episodes (from git ingestion).
pub(crate) async fn git_timeline(
    state: &AppState,
    params: GitTimelineParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let days = params.days.unwrap_or(7);
    let limit = params.limit.unwrap_or(20);

    // Query episodes with kind=git_commit from last N days
    let filter = ListEpisodesFilter {
        project: project_id.clone(),
        kind: Some("git_commit".into()),
        resolved: None,
        limit: Some(limit),
        offset: None,
    };

    let episodes = alaz_db::repos::EpisodeRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("git timeline failed: {e}"))?;

    // Filter to last N days
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let recent: Vec<_> = episodes
        .into_iter()
        .filter(|e| e.created_at >= cutoff)
        .collect();

    if recent.is_empty() {
        return Ok(format!("No git commits in the last {days} days."));
    }

    let mut output = format!(
        "## Git Timeline — last {} days ({} commits)\n\n",
        days,
        recent.len()
    );
    for ep in &recent {
        let hash = ep.action.as_deref().unwrap_or("-");
        let author = ep.who_cues.first().map(String::as_str).unwrap_or("?");
        let files_count = ep.related_files.len();
        let stats = ep
            .source_metadata
            .as_ref()
            .and_then(|m| {
                let ins = m.get("insertions")?.as_i64()?;
                let del = m.get("deletions")?.as_i64()?;
                Some(format!("+{ins} -{del}"))
            })
            .unwrap_or_default();
        output.push_str(&format!(
            "- **{}** `{}` {} ({} files, {})\n  _{}_\n",
            ep.created_at.format("%m-%d %H:%M"),
            hash,
            ep.title,
            files_count,
            stats,
            author,
        ));
    }

    Ok(output)
}

/// Most frequently changed files (hot files).
pub(crate) async fn git_hot_files(
    state: &AppState,
    params: GitHotFilesParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let days = params.days.unwrap_or(30);
    let limit = params.limit.unwrap_or(20);

    let files = GitActivityRepo::hot_files(&state.pool, project_id.as_deref(), days, limit)
        .await
        .map_err(|e| format!("hot files failed: {e}"))?;

    if files.is_empty() {
        return Ok(format!("No git activity in the last {days} days."));
    }

    let mut output = format!(
        "## Hot Files — last {} days ({} files)\n\n",
        days,
        files.len()
    );
    output.push_str("| File | Commits | +Lines | -Lines | Churn |\n|------|---------|--------|--------|-------|\n");
    for f in &files {
        output.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            f.file_path, f.commit_count, f.total_lines_added, f.total_lines_removed, f.total_churn,
        ));
    }

    Ok(output)
}

/// Files that tend to change together (temporal coupling).
pub(crate) async fn git_coupled_files(
    state: &AppState,
    params: GitCoupledFilesParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let days = params.days.unwrap_or(30);
    let min_co = params.min_co_changes.unwrap_or(3);
    let limit = params.limit.unwrap_or(20);

    let pairs =
        GitActivityRepo::coupled_files(&state.pool, project_id.as_deref(), days, min_co, limit)
            .await
            .map_err(|e| format!("coupled files failed: {e}"))?;

    if pairs.is_empty() {
        return Ok(format!(
            "No coupled file pairs found (last {days} days, min {min_co} co-changes)."
        ));
    }

    let mut output = format!(
        "## Coupled Files — last {} days ({} pairs)\n\n",
        days,
        pairs.len()
    );
    output.push_str(
        "| File A | File B | Co-changes | Coupling |\n|--------|--------|-----------|----------|\n",
    );
    for p in &pairs {
        output.push_str(&format!(
            "| `{}` | `{}` | {} | {:.0}% |\n",
            p.file_a,
            p.file_b,
            p.co_change_count,
            p.coupling_ratio * 100.0,
        ));
    }

    Ok(output)
}
