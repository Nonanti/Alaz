use alaz_db::repos::*;
use tracing::warn;

pub(crate) async fn default_owner_id(pool: &sqlx::PgPool) -> Result<String, String> {
    OwnerRepo::list(pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .map(|o| o.id)
        .ok_or_else(|| "no owner found".to_string())
}

/// Helper to resolve project name to project_id.
pub(crate) async fn resolve_project(
    pool: &sqlx::PgPool,
    project_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = project_name {
        match ProjectRepo::get_or_create(pool, name, None).await {
            Ok(project) => Some(project.id),
            Err(e) => {
                warn!(error = %e, name, "failed to resolve project");
                None
            }
        }
    } else {
        None
    }
}

/// Detect the entity type by trying knowledge, episode, procedure tables in order.
pub(crate) async fn detect_entity_type(pool: &sqlx::PgPool, id: &str) -> String {
    let result = sqlx::query_scalar::<_, String>(
        r#"
        SELECT entity_type FROM (
            SELECT 'knowledge_item' AS entity_type FROM knowledge_items WHERE id = $1
            UNION ALL
            SELECT 'episode' FROM episodes WHERE id = $1
            UNION ALL
            SELECT 'procedure' FROM procedures WHERE id = $1
            UNION ALL
            SELECT 'session' FROM session_logs WHERE id = $1
        ) sub
        LIMIT 1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await;

    match result {
        Ok(Some(entity_type)) => entity_type,
        _ => "unknown".to_string(),
    }
}

/// Truncate content to `max_chars` at a word boundary.
pub(crate) fn truncate_content(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let truncated = &s[..s.floor_char_boundary(max_chars)];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}…", &truncated[..pos]),
        None => format!("{truncated}…"),
    }
}

/// Format a single entity's explanation.
pub(crate) fn format_single_explanation(
    query: &str,
    entity_id: &str,
    expl: &serde_json::Value,
    row: &SearchQueryRow,
) -> Result<String, String> {
    let mut output = format!(
        "## Explanation: \"{}\" → `{}`\n**Type**: {} | **Time**: {}\n\n",
        query,
        entity_id,
        row.query_type.as_deref().unwrap_or("unknown"),
        row.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
    );
    output.push_str(&format_contributions(expl));
    Ok(output)
}

/// Format per-signal contributions as a readable table.
pub(crate) fn format_contributions(expl: &serde_json::Value) -> String {
    let mut output = String::new();

    let fused_score = expl
        .get("fused_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    output.push_str(&format!("**Fused score**: {:.4}\n", fused_score));

    if let Some(contribs) = expl.get("contributions").and_then(|v| v.as_array()) {
        output.push_str("| Signal | Score | Rank | Bar |\n");
        output.push_str("|--------|-------|------|-----|\n");

        let max_score = contribs
            .iter()
            .filter_map(|c| c.get("score").and_then(|s| s.as_f64()))
            .fold(0.0_f64, f64::max);

        for contrib in contribs {
            let signal = contrib
                .get("signal")
                .and_then(|s| s.as_str())
                .unwrap_or("?");
            let score = contrib.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
            let rank = contrib.get("rank").and_then(|r| r.as_u64()).unwrap_or(0);

            // Visual bar: scale to 12 chars max
            let bar_len = if max_score > 0.0 {
                ((score / max_score) * 12.0).round() as usize
            } else {
                0
            };
            let bar = "█".repeat(bar_len);

            output.push_str(&format!(
                "| {:<7} | {:.4} | {:<4} | {} |\n",
                signal, score, rank, bar
            ));
        }
    }

    output
}
