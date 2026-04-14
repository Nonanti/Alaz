use alaz_core::models::*;
use alaz_db::repos::*;
use tracing::warn;

use super::super::helpers::*;
use super::super::params::*;
use crate::state::AppState;

pub(crate) async fn save(state: &AppState, params: SaveParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let input = CreateKnowledge {
        title: params.title,
        content: params.content,
        description: params.description,
        kind: params.kind,
        language: params.language,
        file_path: params.file_path,
        project: params.project,
        tags: params.tags,
        valid_from: None,
        valid_until: None,
        source: None,
        source_metadata: None,
    };
    let item = KnowledgeRepo::create(&state.pool, &input, project_id.as_deref())
        .await
        .map_err(|e| format!("failed to save: {e}"))?;
    serde_json::to_string_pretty(&item).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn get(state: &AppState, params: GetParams) -> Result<String, String> {
    let item = KnowledgeRepo::get(&state.pool, &params.id)
        .await
        .map_err(|e| format!("not found: {e}"))?;

    // Record implicit feedback (click) for search quality loop
    let pool = state.pool.clone();
    let id = params.id.clone();
    tokio::spawn(async move {
        let _ = SearchQueryRepo::record_click(&pool, &id).await;
    });

    serde_json::to_string_pretty(&item).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn search(state: &AppState, params: SearchParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let limit = params.limit.unwrap_or(10);
    let results =
        KnowledgeRepo::fts_search(&state.pool, &params.query, project_id.as_deref(), limit)
            .await
            .map_err(|e| format!("search failed: {e}"))?;
    let items: Vec<_> = results
        .into_iter()
        .map(|(id, title, rank)| serde_json::json!({"id": id, "title": title, "rank": rank}))
        .collect();
    serde_json::to_string_pretty(&items).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn hybrid_search(
    state: &AppState,
    params: HybridSearchParams,
) -> Result<String, String> {
    use alaz_core::traits::SearchQuery;

    let query = SearchQuery {
        query: params.query,
        project: params.project,
        limit: params.limit,
        rerank: params.rerank,
        hyde: params.hyde,
        graph_expand: params.graph_expand,
    };
    let results = state
        .search
        .hybrid_search(&query)
        .await
        .map_err(|e| format!("hybrid search failed: {e}"))?;

    // Record access for each result entity in the background
    let pool = state.pool.clone();
    let result_entities: Vec<(String, String)> = results
        .iter()
        .map(|r| (r.entity_type.clone(), r.entity_id.clone()))
        .collect();
    tokio::spawn(async move {
        for (entity_type, entity_id) in result_entities {
            let res = match entity_type.as_str() {
                "knowledge_item" | "knowledge" => {
                    KnowledgeRepo::record_access(&pool, &entity_id).await
                }
                "episode" => EpisodeRepo::record_access(&pool, &entity_id).await,
                "procedure" => ProcedureRepo::record_access(&pool, &entity_id).await,
                _ => Ok(()),
            };
            if let Err(e) = res {
                warn!(entity_type = %entity_type, entity_id = %entity_id, error = %e, "failed to record access");
            }
        }
    });

    serde_json::to_string_pretty(&results).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn list(state: &AppState, params: ListParams) -> Result<String, String> {
    let project_id = resolve_project(&state.pool, params.project.as_deref()).await;
    let filter = ListKnowledgeFilter {
        project: project_id,
        kind: params.kind,
        language: params.language,
        tag: params.tag,
        limit: params.limit,
        offset: params.offset,
    };
    let items = KnowledgeRepo::list(&state.pool, &filter)
        .await
        .map_err(|e| format!("list failed: {e}"))?;
    serde_json::to_string_pretty(&items).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn update(state: &AppState, params: UpdateParams) -> Result<String, String> {
    let input = UpdateKnowledge {
        title: params.title,
        content: params.content,
        description: params.description,
        kind: None,
        language: params.language,
        file_path: params.file_path,
        project: None,
        tags: params.tags,
        valid_from: None,
        valid_until: None,
        superseded_by: None,
    };
    let item = KnowledgeRepo::update(&state.pool, &params.id, &input)
        .await
        .map_err(|e| format!("update failed: {e}"))?;
    serde_json::to_string_pretty(&item).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn delete(state: &AppState, params: DeleteParams) -> Result<String, String> {
    KnowledgeRepo::delete(&state.pool, &params.id)
        .await
        .map_err(|e| format!("delete failed: {e}"))?;
    Ok(format!("deleted {}", params.id))
}

pub(crate) async fn similar(state: &AppState, params: SimilarParams) -> Result<String, String> {
    let limit = params.limit.unwrap_or(10);
    let text = match params.entity_type.as_str() {
        "knowledge_item" | "knowledge" => {
            let item = KnowledgeRepo::get(&state.pool, &params.entity_id)
                .await
                .map_err(|e| format!("entity not found: {e}"))?;
            format!("{} {}", item.title, item.content)
        }
        "episode" => {
            let ep = EpisodeRepo::get(&state.pool, &params.entity_id)
                .await
                .map_err(|e| format!("entity not found: {e}"))?;
            format!("{} {}", ep.title, ep.content)
        }
        "procedure" => {
            let proc_item = ProcedureRepo::get(&state.pool, &params.entity_id)
                .await
                .map_err(|e| format!("entity not found: {e}"))?;
            format!("{} {}", proc_item.title, proc_item.content)
        }
        _ => return Err(format!("unsupported entity type: {}", params.entity_type)),
    };

    let embedding = state
        .embedding
        .embed_text(&[text.as_str()])
        .await
        .map_err(|e| format!("embedding failed: {e}"))?;
    let text_vec = embedding
        .into_iter()
        .next()
        .ok_or_else(|| "empty embedding result".to_string())?;

    let vector_results = alaz_vector::dense::DenseVectorOps::search_text(
        state.qdrant.client(),
        text_vec,
        None,
        (limit + 1) as u64,
    )
    .await
    .map_err(|e| format!("vector search failed: {e}"))?;

    // Hydrate results from DB
    let mut results = Vec::new();
    for (entity_type, entity_id, score) in vector_results {
        if entity_id == params.entity_id {
            continue;
        }
        let (title, content, project) = match entity_type.as_str() {
            "knowledge_item" | "knowledge" => {
                match KnowledgeRepo::get_readonly(&state.pool, &entity_id).await {
                    Ok(item) => (item.title, item.content, item.project_id),
                    Err(_) => continue,
                }
            }
            "episode" => match EpisodeRepo::get(&state.pool, &entity_id).await {
                Ok(ep) => (ep.title, ep.content, ep.project_id),
                Err(_) => continue,
            },
            "procedure" => match ProcedureRepo::get(&state.pool, &entity_id).await {
                Ok(p) => (p.title, p.content, p.project_id),
                Err(_) => continue,
            },
            _ => continue,
        };
        results.push(serde_json::json!({
            "entity_type": entity_type,
            "entity_id": entity_id,
            "title": title,
            "content": content,
            "score": score,
            "project": project,
        }));
        if results.len() >= limit {
            break;
        }
    }
    serde_json::to_string_pretty(&results).map_err(|e| format!("json error: {e}"))
}

pub(crate) async fn record_usage(
    state: &AppState,
    params: RecordUsageParams,
) -> Result<String, String> {
    if !["success", "failure", "partial"].contains(&params.outcome.as_str()) {
        return Err("outcome must be 'success', 'failure', or 'partial'".into());
    }

    KnowledgeRepo::record_usage_with_outcome(
        &state.pool,
        &params.id,
        &params.outcome,
        params.context.as_deref(),
    )
    .await
    .map_err(|e| format!("record usage failed: {e}"))?;

    let item = KnowledgeRepo::get_readonly(&state.pool, &params.id)
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    Ok(format!(
        "Recorded {} for \"{}\"\nUsage: {}/{} | Utility: {:.2}",
        params.outcome, item.title, item.times_success, item.times_used, item.utility_score
    ))
}
