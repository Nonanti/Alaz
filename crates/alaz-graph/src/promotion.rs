use std::collections::HashSet;

use alaz_core::Result;
use alaz_core::models::{CreateKnowledge, CreateRelation};
use alaz_db::repos::{GraphRepo, KnowledgeRepo};
use sqlx::PgPool;
use tracing::info;

/// Check whether a knowledge item should be promoted to a global pattern.
///
/// A pattern is promoted when similar items (by title, threshold 0.4) exist
/// in 2 or more distinct projects. When promoted, a new global copy is created
/// (with `project_id = NULL`) and `derived_from` edges are created from the
/// global copy to each source item.
///
/// Returns the new global knowledge ID if promotion occurred, or `None`.
pub async fn check_and_promote(
    pool: &PgPool,
    knowledge_id: &str,
    title: &str,
) -> Result<Option<String>> {
    // Find similar items across all projects (no project filter)
    let similar = KnowledgeRepo::find_similar_by_title(pool, title, 0.4, None).await?;

    // Collect distinct projects (only items that have a project)
    let projects: HashSet<&str> = similar
        .iter()
        .filter_map(|item| item.project_id.as_deref())
        .collect();

    if projects.len() < 2 {
        return Ok(None);
    }

    info!(
        knowledge_id,
        title,
        project_count = projects.len(),
        "promoting pattern to global"
    );

    // Get the source item to copy its content
    let source = KnowledgeRepo::get(pool, knowledge_id).await?;

    // Create a global copy (project_id = NULL)
    let global = KnowledgeRepo::create(
        pool,
        &CreateKnowledge {
            title: source.title.clone(),
            content: source.content.clone(),
            description: source.description.clone(),
            kind: Some("pattern".to_string()),
            language: source.language.clone(),
            file_path: None,
            project: None, // Global — no project
            tags: Some(source.tags.clone()),
            valid_from: None,
            valid_until: None,
            source: source.source.clone(),
            source_metadata: source.source_metadata.clone(),
        },
        None, // project_id = NULL
    )
    .await?;

    // Create derived_from edges from the global copy to each similar item
    for item in &similar {
        if item.project_id.is_some() {
            GraphRepo::create_edge(
                pool,
                &CreateRelation {
                    source_type: "knowledge_item".to_string(),
                    source_id: global.id.clone(),
                    target_type: "knowledge_item".to_string(),
                    target_id: item.id.clone(),
                    relation: "derived_from".to_string(),
                    weight: Some(1.0),
                    description: Some("global pattern derived from project copy".to_string()),
                    metadata: None,
                },
            )
            .await?;
        }
    }

    info!(
        global_id = global.id,
        similar_count = similar.len(),
        "created global pattern with derived_from edges"
    );

    Ok(Some(global.id))
}
