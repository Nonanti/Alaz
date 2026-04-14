use alaz_core::models::*;
use alaz_db::repos::*;
use alaz_intel::RaptorBuilder;
use tracing::warn;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn health(state: &AppState, params: HealthParams) -> Result<String, String> {
    let report = alaz_intel::health::compute_health(&state.pool, params.project.as_deref())
        .await
        .map_err(|e| format!("health check failed: {e}"))?;

    let mut output = format!(
        "## 📊 Knowledge Health — {}\n**Total**: {} items | **Stale**: {} items\n\n",
        report.project, report.total_items, report.stale_items,
    );

    output.push_str("### Topics\n");
    output.push_str("| Status | Topic | Items | Freshness | Confidence | Stale | Overall |\n");
    output.push_str("|--------|-------|-------|-----------|------------|-------|---------|\n");
    for t in &report.topics {
        output.push_str(&format!(
            "| {} | {} | {} | {:.0}% | {:.0}% | {} | {:.0}% |\n",
            t.status,
            t.topic,
            t.item_count,
            t.freshness * 100.0,
            t.confidence * 100.0,
            t.stale_count,
            t.overall * 100.0,
        ));
    }

    if !report.gaps.is_empty() {
        output.push_str(&format!(
            "\n### 🕳️ Knowledge Gaps ({})\n",
            report.gaps.len()
        ));
        for gap in &report.gaps {
            output.push_str(&format!(
                "- **\"{}\"** — searched {} times, 0 clicks\n  → {}\n",
                gap.topic, gap.search_count, gap.suggestion,
            ));
        }
    }

    Ok(output)
}

pub(crate) async fn impact(state: &AppState, params: ImpactParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let definitions =
        CodeSymbolRepo::get_by_name(&state.pool, project_id.as_deref(), &params.symbol_name)
            .await
            .map_err(|e| format!("impact lookup failed: {e}"))?;

    let callers =
        CodeSymbolRepo::find_callers(&state.pool, project_id.as_deref(), &params.symbol_name)
            .await
            .map_err(|e| format!("caller lookup failed: {e}"))?;

    if definitions.is_empty() && callers.is_empty() {
        return Ok(format!(
            "Symbol \"{}\" not found in the code index.\n\
             Index the codebase first via POST /api/v1/code/index.",
            params.symbol_name
        ));
    }

    let mut output = format!("## 🎯 Impact Analysis: `{}`\n\n", params.symbol_name);

    output.push_str(&format!("### Definitions ({})\n", definitions.len()));
    for def in &definitions {
        output.push_str(&format!(
            "- `{}` ({}) — {}:{}\n",
            def.signature.as_deref().unwrap_or(&def.symbol_name),
            def.symbol_type,
            def.file_path,
            def.line_number,
        ));
    }

    output.push_str(&format!("\n### Direct Callers ({})\n", callers.len()));
    if callers.is_empty() {
        output.push_str("_No callers found in the index._\n");
    } else {
        for caller in &callers {
            let parent = caller
                .parent_symbol
                .as_deref()
                .map(|p| format!(" (in `{p}`)"))
                .unwrap_or_default();
            output.push_str(&format!(
                "- `{}`{} — {}:{}\n",
                caller.symbol_name, parent, caller.file_path, caller.line_number,
            ));
        }
    }

    let risk = match callers.len() {
        0 => "🟢 Low — no callers, safe to change",
        1..=3 => "🟡 Medium — few callers, review each one",
        _ => "🔴 High — many callers, change with caution",
    };
    output.push_str(&format!("\n**Risk**: {risk}\n"));

    Ok(output)
}

pub(crate) async fn evolution(state: &AppState, params: EvolutionParams) -> Result<String, String> {
    let chain = alaz_intel::evolution::get_evolution_chain(&state.pool, &params.id)
        .await
        .map_err(|e| format!("evolution lookup failed: {e}"))?;

    if chain.total_versions == 1 {
        return Ok(format!(
            "## 📜 Evolution: \"{}\"\nThis item has no previous versions (v1, current).",
            chain.current_title,
        ));
    }

    let mut output = format!(
        "## 📜 Evolution: \"{}\" ({} versions)\n\n",
        chain.current_title, chain.total_versions,
    );

    for entry in &chain.entries {
        let marker = if entry.is_current { " ← current" } else { "" };
        let reason = entry
            .reason
            .as_deref()
            .map(|r| format!("\n     Reason: \"{r}\""))
            .unwrap_or_default();
        output.push_str(&format!(
            "  v{} ({}) — {}{}{}\n",
            entry.version, entry.created_at, entry.title, marker, reason,
        ));
    }

    if chain.total_versions >= 3
        && let (Some(first), Some(last)) = (chain.entries.first(), chain.entries.last())
    {
        output.push_str(&format!(
            "\n**Trend**: {} versions from {} to {} — this knowledge is actively evolving.\n",
            chain.total_versions, first.created_at, last.created_at,
        ));
    }

    Ok(output)
}

pub(crate) async fn review(state: &AppState, params: ReviewParams) -> Result<String, String> {
    alaz_intel::evolution::record_review(&state.pool, &params.id, params.quality)
        .await
        .map_err(|e| format!("review failed: {e}"))?;

    let item = KnowledgeRepo::get(&state.pool, &params.id)
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    let sr_info = sqlx::query_as::<_, (i32, f32, i32)>(
        "SELECT sr_interval_days, sr_easiness, sr_repetitions FROM knowledge_items WHERE id = $1",
    )
    .bind(&params.id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    let interval_days = sr_info.map(|(days, _, _)| days).unwrap_or(1);

    Ok(format!(
        "✅ Review recorded for \"{}\"\nQuality: {}/5 | Next review in: {} days",
        item.title, params.quality, interval_days,
    ))
}

pub(crate) async fn review_list(
    state: &AppState,
    params: ReviewListParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let limit = params.limit.unwrap_or(5);

    let items =
        alaz_intel::evolution::items_due_for_review(&state.pool, project_id.as_deref(), limit)
            .await
            .map_err(|e| format!("review list failed: {e}"))?;

    if items.is_empty() {
        return Ok("📝 No items due for review. All caught up!".to_string());
    }

    let mut output = format!("## 📝 Items Due for Review ({})\n\n", items.len());
    for item in &items {
        output.push_str(&format!(
            "- **{}** (`{}`)\n  {}\n\n",
            item.title,
            item.id,
            truncate_content(&item.content, 100),
        ));
    }
    output.push_str("Use `alaz_review(id, quality)` to record your review (0-5 scale).\n");

    Ok(output)
}

pub(crate) async fn supersede(state: &AppState, params: SupersedeParams) -> Result<String, String> {
    let entity_type = detect_entity_type(&state.pool, &params.old_id).await;

    match entity_type.as_str() {
        "knowledge_item" | "knowledge" => {
            KnowledgeRepo::supersede(
                &state.pool,
                &params.old_id,
                &params.new_id,
                params.reason.as_deref(),
            )
            .await
            .map_err(|e| format!("failed to supersede knowledge item: {e}"))?;
        }
        "episode" => {
            EpisodeRepo::supersede(&state.pool, &params.old_id, &params.new_id)
                .await
                .map_err(|e| format!("failed to supersede episode: {e}"))?;
        }
        "procedure" => {
            ProcedureRepo::supersede(&state.pool, &params.old_id, &params.new_id)
                .await
                .map_err(|e| format!("failed to supersede procedure: {e}"))?;
        }
        _ => {
            return Err(format!(
                "entity {} not found or unknown type",
                params.old_id
            ));
        }
    }

    let input = CreateRelation {
        source_type: entity_type.clone(),
        source_id: params.new_id.clone(),
        target_type: entity_type,
        target_id: params.old_id.clone(),
        relation: "supersedes".to_string(),
        weight: Some(1.0),
        description: params.reason.clone(),
        metadata: None,
    };
    if let Err(e) = GraphRepo::create_edge(&state.pool, &input).await {
        warn!(error = %e, "failed to create supersession graph edge");
    }

    Ok(format!(
        "superseded {} with {}",
        params.old_id, params.new_id
    ))
}

pub(crate) async fn procedure_outcome(
    state: &AppState,
    params: ProcedureOutcomeParams,
) -> Result<String, String> {
    ProcedureRepo::record_outcome(&state.pool, &params.id, params.success)
        .await
        .map_err(|e| format!("record outcome failed: {e}"))?;

    let proc = ProcedureRepo::get(&state.pool, &params.id)
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    let status = if params.success {
        "✅ success"
    } else {
        "❌ failure"
    };
    let wilson = proc
        .success_rate
        .map(|s| format!("{:.1}%", s * 100.0))
        .unwrap_or("N/A".to_string());

    Ok(format!(
        "Recorded {status} for \"{}\"\nStats: {}/{} runs, Wilson confidence: {wilson}",
        proc.title, proc.success, proc.times_used
    ))
}

pub(crate) async fn ingest(state: &AppState, params: IngestParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let domain = alaz_intel::detect_domain(&params.content);

    let learner = alaz_intel::SessionLearner::new(
        state.pool.clone(),
        state.llm.clone(),
        state.embedding.clone(),
        state.qdrant.clone(),
    );

    let content = if let Some(ref title) = params.title {
        format!("# {title}\n\n{}", params.content)
    } else {
        params.content
    };

    let summary = learner
        .learn_from_content(&content, &params.source, domain, project_id.as_deref())
        .await
        .map_err(|e| format!("ingestion failed: {e}"))?;

    let total = summary.patterns_saved
        + summary.episodes_saved
        + summary.procedures_saved
        + summary.memories_saved;

    let result = serde_json::json!({
        "items_extracted": total,
        "patterns": summary.patterns_saved,
        "episodes": summary.episodes_saved,
        "procedures": summary.procedures_saved,
        "memories": summary.memories_saved,
        "source": params.source,
        "domain": domain.to_string(),
    });
    serde_json::to_string_pretty(&result).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn raptor_rebuild(
    state: &AppState,
    params: RaptorRebuildInput,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let builder = RaptorBuilder::new(
        state.pool.clone(),
        state.llm.clone(),
        state.embedding.clone(),
        state.qdrant.clone(),
    );
    let tree = builder
        .rebuild_tree(project_id.as_deref())
        .await
        .map_err(|e| format!("raptor rebuild failed: {e}"))?;
    serde_json::to_string_pretty(&tree).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn raptor_status(
    state: &AppState,
    params: RaptorStatusInput,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let tree = RaptorRepo::get_tree(&state.pool, project_id.as_deref())
        .await
        .map_err(|e| format!("raptor status failed: {e}"))?;
    match tree {
        Some(t) => serde_json::to_string_pretty(&t).map_err(|e| format!("json error: {e}")),
        None => Ok(r#"{"status": "no tree found"}"#.to_string()),
    }
}

pub(crate) async fn procedures(
    state: &AppState,
    params: ProceduresParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = ListProceduresFilter {
        project: project_id,
        tag: params.tag,
        limit: params.limit,
        offset: None,
    };
    let procedures = ProcedureRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("procedures failed: {e}"))?;
    serde_json::to_string_pretty(&procedures).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn core_memory(
    state: &AppState,
    params: CoreMemoryParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = ListCoreMemoryFilter {
        project: project_id,
        category: params.category,
        limit: params.limit,
        offset: None,
    };
    let memories = CoreMemoryRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("core memory failed: {e}"))?;
    serde_json::to_string_pretty(&memories).map_err(|e| format!("json error: {e}"))
}

// --- Observability MCP handlers ---

pub(crate) async fn metrics(state: &AppState) -> Result<String, String> {
    let m = &state.metrics;
    let snap = m.snapshot();

    Ok(format!(
        "## System Metrics\n\n\
        | Metric | Value |\n|---|---|\n\
        | Uptime | {}h {}m |\n\
        | Search requests | {} |\n\
        | Search avg latency | {}ms |\n\
        | Search max latency | {}ms |\n\
        | LLM calls | {} |\n\
        | LLM errors | {} |\n\
        | Embeddings | {} |\n\
        | Backfill processed | {} |\n\
        | Decay pruned | {} |\n\
        | Consolidation merged | {} |",
        snap.uptime_seconds / 3600,
        (snap.uptime_seconds % 3600) / 60,
        snap.search_count,
        snap.search_avg_latency_ms,
        snap.search_max_latency_ms,
        snap.llm_call_count,
        snap.llm_error_count,
        snap.embedding_count,
        snap.backfill_processed,
        snap.decay_pruned,
        snap.consolidation_merged,
    ))
}

pub(crate) async fn learning_analytics(
    state: &AppState,
    params: LearningAnalyticsParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(10);
    let runs = LearningRunRepo::recent(&state.pool, limit)
        .await
        .map_err(|e| format!("learning analytics failed: {e}"))?;

    if runs.is_empty() {
        return Ok("No learning runs recorded yet.".into());
    }

    let mut output = format!("## Learning Analytics ({} recent runs)\n\n", runs.len());
    output.push_str("| Date | Patterns | Episodes | Procedures | Memories | Dupes | Duration |\n");
    output.push_str("|------|----------|----------|------------|----------|-------|----------|\n");

    for run in &runs {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {}ms |\n",
            run.created_at.format("%m-%d %H:%M"),
            run.patterns_extracted,
            run.episodes_extracted,
            run.procedures_extracted,
            run.memories_extracted,
            run.duplicates_skipped,
            run.duration_ms,
        ));
    }

    Ok(output)
}

pub(crate) async fn search_analytics(
    state: &AppState,
    params: SearchAnalyticsParams,
) -> Result<String, String> {
    let days = params.days.unwrap_or(7);
    let analytics = SearchQueryRepo::analytics(&state.pool, days)
        .await
        .map_err(|e| format!("search analytics failed: {e}"))?;

    serde_json::to_string_pretty(&analytics).map_err(|e| format!("json error: {e}"))
}

// --- Project Health ---

pub(crate) async fn project_health(
    state: &AppState,
    params: super::super::params::ProjectHealthParams,
) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;

    let report =
        alaz_intel::health_score::compute_project_health(&state.pool, project_id.as_deref())
            .await
            .map_err(|e| format!("project health failed: {e}"))?;

    let project_label = report.project_id.as_deref().unwrap_or("global");
    let mut output = format!(
        "## Project Health — {}\n**Overall Score**: {:.0}%\n\n",
        project_label,
        report.overall_score * 100.0,
    );

    output.push_str("### Dimensions\n");
    output.push_str("| Dimension | Score | Status | Detail |\n");
    output.push_str("|-----------|-------|--------|--------|\n");

    for dim in &report.dimensions {
        let bar_len = (dim.score * 10.0).round() as usize;
        let bar = "█".repeat(bar_len);
        output.push_str(&format!(
            "| {} | {:.0}% {} | {} | {} |\n",
            dim.name,
            dim.score * 100.0,
            bar,
            dim.status,
            dim.detail,
        ));
    }

    if !report.recommendations.is_empty() {
        output.push_str("\n### Recommendations\n");
        for rec in &report.recommendations {
            output.push_str(&format!("- {}\n", rec));
        }
    }

    Ok(output)
}
