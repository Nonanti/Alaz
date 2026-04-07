use chrono::{NaiveDate, TimeZone, Utc};

use alaz_core::models::*;
use alaz_db::repos::*;
use alaz_graph::{follow_causal_chain, follow_causal_chain_reverse};

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn episodes(state: &AppState, params: EpisodesParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = ListEpisodesFilter {
        project: project_id,
        kind: params.kind,
        resolved: params.resolved,
        limit: params.limit,
        offset: None,
    };
    let episodes = EpisodeRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("episodes failed: {e}"))?;
    serde_json::to_string_pretty(&episodes).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn cue_search(
    state: &AppState,
    params: CueSearchParams,
) -> Result<String, String> {
    let episodes = EpisodeRepo::cue_search(
        &state.pool,
        params.who.as_deref(),
        params.what.as_deref(),
        params.where_.as_deref(),
        params.when.as_deref(),
        params.why.as_deref(),
        params.project.as_deref(),
        params.limit,
    )
    .await
    .map_err(|e| format!("cue search failed: {e}"))?;
    let limit = params.limit.unwrap_or(20) as usize;
    let episodes: Vec<_> = episodes.into_iter().take(limit).collect();
    serde_json::to_string_pretty(&episodes).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn episode_chain(
    state: &AppState,
    params: EpisodeChainParams,
) -> Result<String, String> {
    let direction = params.direction.as_deref().unwrap_or("forward");
    let chain = match direction {
        "backward" | "reverse" => {
            follow_causal_chain_reverse(&state.pool, "episode", &params.episode_id)
                .await
                .map_err(|e| format!("chain traversal failed: {e}"))?
        }
        _ => follow_causal_chain(&state.pool, "episode", &params.episode_id)
            .await
            .map_err(|e| format!("chain traversal failed: {e}"))?,
    };
    serde_json::to_string_pretty(&chain).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn episode_link(
    state: &AppState,
    params: EpisodeLinkParams,
) -> Result<String, String> {
    let relation = params.relation.unwrap_or_else(|| "led_to".to_string());
    let input = CreateRelation {
        source_type: "episode".to_string(),
        source_id: params.source_id,
        target_type: "episode".to_string(),
        target_id: params.target_id,
        relation,
        weight: Some(1.0),
        description: None,
        metadata: None,
    };
    let edge = GraphRepo::create_edge(&state.pool, &input)
        .await
        .map_err(|e| format!("episode link failed: {e}"))?;
    serde_json::to_string_pretty(&edge).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn timeline(state: &AppState, params: TimelineParams) -> Result<String, String> {
    let start = NaiveDate::parse_from_str(&params.start_date, "%Y-%m-%d")
        .map_err(|e| format!("invalid start_date (expected YYYY-MM-DD): {e}"))?;
    let end = NaiveDate::parse_from_str(&params.end_date, "%Y-%m-%d")
        .map_err(|e| format!("invalid end_date (expected YYYY-MM-DD): {e}"))?;

    let start_dt = Utc.from_utc_datetime(
        &start
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| "invalid start time".to_string())?,
    );
    let end_dt = Utc.from_utc_datetime(
        &end.and_hms_opt(23, 59, 59)
            .ok_or_else(|| "invalid end time".to_string())?,
    );

    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let (episodes, sessions) = tokio::join!(
        EpisodeRepo::list_in_range(&state.pool, start_dt, end_dt, project_id.as_deref()),
        SessionRepo::list_in_range(&state.pool, start_dt, end_dt, project_id.as_deref()),
    );

    let episodes = episodes.map_err(|e| format!("failed to fetch episodes: {e}"))?;
    let sessions = sessions.map_err(|e| format!("failed to fetch sessions: {e}"))?;

    #[derive(serde::Serialize)]
    struct TimelineEntry {
        timestamp: String,
        kind: String,
        title: String,
        summary: String,
    }

    let mut entries: Vec<TimelineEntry> = Vec::new();

    for ep in &episodes {
        entries.push(TimelineEntry {
            timestamp: ep.created_at.format("%Y-%m-%d %H:%M").to_string(),
            kind: format!("episode:{}", ep.kind),
            title: ep.title.clone(),
            summary: ep.content.chars().take(200).collect(),
        });
    }

    for sess in &sessions {
        let summary = sess.summary.as_deref().unwrap_or("(no summary)");
        let status = sess.status.as_deref().unwrap_or("unknown");
        entries.push(TimelineEntry {
            timestamp: sess.created_at.format("%Y-%m-%d %H:%M").to_string(),
            kind: format!("session:{status}"),
            title: format!("Session {}", &sess.id[..8.min(sess.id.len())]),
            summary: summary.chars().take(200).collect(),
        });
    }

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let mut md = format!(
        "## Timeline: {} to {}\n\n",
        params.start_date, params.end_date
    );
    md.push_str(&format!(
        "{} episodes, {} sessions\n\n",
        episodes.len(),
        sessions.len()
    ));

    for entry in &entries {
        md.push_str(&format!(
            "- **[{}]** `{}` — {}\n  {}\n\n",
            entry.timestamp, entry.kind, entry.title, entry.summary
        ));
    }

    if entries.is_empty() {
        md.push_str("No activity found in this date range.\n");
    }

    Ok(md)
}

pub(crate) async fn create_reflection(
    state: &AppState,
    params: CreateReflectionParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let action_items: Option<Vec<alaz_core::models::ActionItem>> = params
        .action_items
        .map(|v| serde_json::from_value(v).unwrap_or_default());

    let input = alaz_core::models::CreateReflection {
        session_id: params.session_id,
        what_worked: params.what_worked,
        what_failed: params.what_failed,
        lessons_learned: params.lessons_learned,
        effectiveness_score: None,
        complexity_score: None,
        kind: params.kind,
        action_items,
        overall_score: params.overall_score,
        knowledge_score: params.knowledge_score,
        decision_score: params.decision_score,
        efficiency_score: params.efficiency_score,
        evaluated_episode_ids: params.evaluated_episode_ids,
        project: None,
    };

    let reflection = ReflectionRepo::create(&state.pool, &input, project_id.as_deref())
        .await
        .map_err(|e| format!("failed to create reflection: {e}"))?;
    serde_json::to_string_pretty(&reflection).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn reflections(
    state: &AppState,
    params: ReflectionsParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = alaz_core::models::ListReflectionsFilter {
        project: project_id,
        kind: params.kind,
        session_id: params.session_id,
        limit: params.limit,
        offset: None,
    };
    let reflections = ReflectionRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("failed to list reflections: {e}"))?;
    serde_json::to_string_pretty(&reflections).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn reflection_insights(
    state: &AppState,
    params: ReflectionInsightsParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let days = params.days.unwrap_or(30);
    let trends = ReflectionRepo::score_trends(&state.pool, project_id.as_deref(), days)
        .await
        .map_err(|e| format!("failed to get score trends: {e}"))?;
    serde_json::to_string_pretty(&trends).map_err(|e| format!("json error: {e}"))
}
