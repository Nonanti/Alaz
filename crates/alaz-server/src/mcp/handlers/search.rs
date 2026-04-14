use alaz_core::traits::SearchQuery;
use alaz_db::repos::*;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn cross_project(
    state: &AppState,
    params: CrossProjectParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(10);
    let query = SearchQuery {
        query: params.query,
        project: None,
        limit: Some(limit),
        rerank: None,
        hyde: Some(false),
        graph_expand: Some(true),
    };
    let results = state
        .search
        .hybrid_search(&query)
        .await
        .map_err(|e| format!("cross-project search failed: {e}"))?;
    let results: Vec<_> = if let Some(ref exclude) = params.exclude_project {
        results
            .into_iter()
            .filter(|r| r.project.as_deref() != Some(exclude.as_str()))
            .collect()
    } else {
        results
    };
    serde_json::to_string_pretty(&results).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn search_feedback(
    state: &AppState,
    params: SearchFeedbackParams,
) -> Result<String, String> {
    SearchQueryRepo::record_click(&state.pool, &params.entity_id)
        .await
        .map_err(|e| format!("search feedback failed: {e}"))?;
    Ok(format!(
        "recorded click feedback for entity {}",
        params.entity_id
    ))
}

pub(crate) async fn explain(state: &AppState, params: ExplainParams) -> Result<String, String> {
    let row = SearchQueryRepo::get_latest_by_query(&state.pool, &params.query)
        .await
        .map_err(|e| format!("explain lookup failed: {e}"))?;

    let Some(row) = row else {
        return Ok(format!(
            "No recent search found for query: \"{}\"\nRun a search first, then call alaz_explain with the same query.",
            params.query
        ));
    };

    let explanations = row.explanations.as_object().cloned().unwrap_or_default();

    if let Some(ref entity_id) = params.entity_id {
        if let Some(expl) = explanations.get(entity_id) {
            return format_single_explanation(&params.query, entity_id, expl, &row);
        }
        return Ok(format!(
            "Entity \"{}\" was not in the results for query: \"{}\"",
            entity_id, params.query
        ));
    }

    let mut output = format!(
        "## Search Explanation: \"{}\"\n**Type**: {} | **Results**: {} | **Time**: {}\n\n",
        row.query,
        row.query_type.as_deref().unwrap_or("unknown"),
        row.result_ids.len(),
        row.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
    );

    for (rank, entity_id) in row.result_ids.iter().enumerate() {
        if let Some(expl) = explanations.get(entity_id) {
            output.push_str(&format!("### #{} — `{}`\n", rank + 1, entity_id));
            output.push_str(&format_contributions(expl));
            output.push('\n');
        }
    }

    if explanations.is_empty() {
        output.push_str(
            "_No signal breakdown available (search was logged before explainability was enabled)._\n",
        );
    }

    Ok(output)
}

pub(crate) async fn context_budget(
    _state: &AppState,
    params: ContextBudgetParams,
) -> Result<String, String> {
    let current_chars = params.current_length.unwrap_or(0);
    let context_window = params.context_window.unwrap_or(200_000);

    let estimated_used = current_chars.div_ceil(4);
    let remaining = context_window.saturating_sub(estimated_used);
    let percent_used = if context_window > 0 {
        ((estimated_used as f64 / context_window as f64) * 100.0).round() as u64
    } else {
        0
    };

    let (warning_level, should_summarize, suggested_action) = match percent_used {
        0..=24 => (
            "none",
            false,
            "Context budget is healthy. Continue normally.",
        ),
        25..=49 => (
            "low",
            false,
            "Context usage is moderate. No action needed yet.",
        ),
        50..=74 => (
            "medium",
            false,
            "Context is filling up. Consider summarizing completed work.",
        ),
        75..=89 => (
            "high",
            true,
            "Context is getting full. Use alaz_optimize_context to compress, or save a checkpoint with alaz_checkpoint_save.",
        ),
        _ => (
            "critical",
            true,
            "Context is nearly full! Immediately save a checkpoint and use alaz_compact_restore in a new session.",
        ),
    };

    let result = serde_json::json!({
        "estimated_used_tokens": estimated_used,
        "remaining_tokens": remaining,
        "percent_used": percent_used,
        "warning_level": warning_level,
        "should_summarize": should_summarize,
        "suggested_action": suggested_action,
    });
    serde_json::to_string_pretty(&result).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn optimize_context(
    state: &AppState,
    params: OptimizeContextParams,
) -> Result<String, String> {
    let optimizer = alaz_intel::ContextOptimizer::new(state.llm.clone());
    let max_tokens = params.max_tokens.unwrap_or(80_000);
    let use_summarization = params.use_summarization.unwrap_or(true);

    let result = optimizer
        .optimize(&params.text, max_tokens, use_summarization)
        .await
        .map_err(|e| format!("optimize failed: {e}"))?;

    serde_json::to_string_pretty(&result).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn agentic_search(
    state: &AppState,
    params: AgenticSearchParams,
) -> Result<String, String> {
    let project = resolve_project(&state.pool, params.project.as_deref()).await;
    let limit = params.limit.unwrap_or(10);

    let results = alaz_search::agentic::agentic_search(
        &state.search,
        &state.llm,
        &params.query,
        project.as_deref(),
        limit,
    )
    .await
    .map_err(|e| format!("agentic search failed: {e}"))?;

    if results.is_empty() {
        return Ok(format!(
            "No results found for \"{}\" after multi-hop search",
            params.query
        ));
    }

    let mut output = format!(
        "## Agentic Search: \"{}\" ({} results)\n\n",
        params.query,
        results.len()
    );
    for (i, r) in results.iter().enumerate() {
        let snippet = truncate_content(&r.content, 300);
        output.push_str(&format!(
            "{}. **{}** ({}, score: {:.3})\n   {}\n\n",
            i + 1,
            r.title,
            r.entity_type,
            r.score,
            snippet
        ));
    }
    Ok(output)
}

pub(crate) async fn rag_fusion(
    state: &AppState,
    params: RagFusionSearchParams,
) -> Result<String, String> {
    let project = resolve_project(&state.pool, params.project.as_deref()).await;
    let limit = params.limit.unwrap_or(10);

    let query = alaz_core::traits::SearchQuery {
        query: params.query.clone(),
        project,
        limit: Some(limit),
        rerank: Some(true),
        hyde: Some(false),
        graph_expand: Some(true),
    };

    let results = state
        .search
        .rag_fusion_search(&query, &state.llm)
        .await
        .map_err(|e| format!("RAG fusion search failed: {e}"))?;

    if results.is_empty() {
        return Ok(format!(
            "No results found for \"{}\" via RAG fusion",
            params.query
        ));
    }

    let mut output = format!(
        "## RAG Fusion Search: \"{}\" ({} results)\n\n",
        params.query,
        results.len()
    );
    for (i, r) in results.iter().enumerate() {
        let snippet = truncate_content(&r.content, 300);
        output.push_str(&format!(
            "{}. **{}** ({}, score: {:.3})\n   {}\n\n",
            i + 1,
            r.title,
            r.entity_type,
            r.score,
            snippet
        ));
    }
    Ok(output)
}
