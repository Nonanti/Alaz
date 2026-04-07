use alaz_core::models::{
    ListCoreMemoryFilter, ListEpisodesFilter, ListKnowledgeFilter, ListSessionsFilter,
};
use alaz_core::{Result, estimate_tokens};
use alaz_db::repos::{CoreMemoryRepo, EpisodeRepo, KnowledgeRepo, SessionRepo};
use sqlx::PgPool;
use tracing::debug;

/// Builds compact restore context for resuming work across sessions.
///
/// Collects 5 digest sections:
/// 1. Checkpoint digest (latest checkpoint)
/// 2. Episode digest (recent episodes)
/// 3. Knowledge digest (recently updated items)
/// 4. Core memory digest (always included)
/// 5. Session summary (for synthetic checkpoint fallback)
pub struct CompactRestorer {
    pool: PgPool,
}

pub struct CompactRestoreResult {
    pub formatted_output: String,
    pub tokens_used: u64,
    pub sections_included: Vec<String>,
}

impl CompactRestorer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn build_restore_context(
        &self,
        session_id: &str,
        project_id: Option<&str>,
        _message_limit: Option<i64>,
    ) -> Result<CompactRestoreResult> {
        let mut output = String::with_capacity(16_000);
        let mut sections = Vec::new();

        output.push_str("COMPACT RESTORE (Alaz Auto-Bridge)\n");
        output.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");

        // Section 1: Checkpoint digest
        let checkpoint = SessionRepo::get_latest_checkpoint(&self.pool, session_id).await?;
        if let Some(ref cp) = checkpoint {
            sections.push("checkpoint".to_string());
            output.push_str("## Checkpoint\n");
            let data_str = serde_json::to_string_pretty(&cp.checkpoint_data)
                .unwrap_or_else(|_| "{}".to_string());
            let preview = truncate_str(&data_str, 2000);
            output.push_str(&format!("Session: {}\n", cp.session_id));
            output.push_str(&format!("Saved: {}\n", cp.created_at));
            output.push_str(&format!("Data:\n{}\n\n", preview));
        } else {
            // Synthetic checkpoint from latest session summary
            let sessions = SessionRepo::list(
                &self.pool,
                &ListSessionsFilter {
                    project: project_id.map(|s| s.to_string()),
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await?;

            if let Some(session) = sessions.first()
                && let Some(ref summary) = session.summary
            {
                sections.push("synthetic_checkpoint".to_string());
                output.push_str("## Checkpoint (Synthetic)\n");
                output.push_str(&format!("Source: session {}\n", session.id));
                output.push_str(&format!("Task: {}\n\n", truncate_str(summary, 500)));
            }
        }

        // Section 2: Episode digest
        let episodes = EpisodeRepo::list(
            &self.pool,
            &ListEpisodesFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(20),
                ..Default::default()
            },
        )
        .await?;

        if !episodes.is_empty() {
            sections.push("episodes".to_string());
            output.push_str("## Recent Episodes\n");
            for ep in episodes.iter().take(10) {
                let icon = match ep.kind.as_str() {
                    "error" => "❌",
                    "decision" => "🔷",
                    "success" => "✅",
                    "discovery" => "💡",
                    _ => "📌",
                };
                let resolved = if ep.resolved { " [resolved]" } else { "" };
                output.push_str(&format!(
                    "{} [{}]{} {}: {}\n",
                    icon,
                    ep.kind,
                    resolved,
                    ep.title,
                    truncate_str(&ep.content, 100),
                ));
            }
            output.push('\n');
        }

        // Section 3: Recent knowledge (last 7 days, max 10)
        let knowledge = KnowledgeRepo::list(
            &self.pool,
            &ListKnowledgeFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(10),
                ..Default::default()
            },
        )
        .await?;

        if !knowledge.is_empty() {
            sections.push("knowledge".to_string());
            output.push_str("## Recent Knowledge\n");
            for item in knowledge.iter().take(5) {
                let lang = item.language.as_deref().unwrap_or("text");
                output.push_str(&format!(
                    "- {} [{}]: {}\n",
                    item.title,
                    lang,
                    truncate_str(&item.content, 500),
                ));
            }
            output.push('\n');
        }

        // Section 4: Core memories (always included)
        let core_memories = CoreMemoryRepo::list(
            &self.pool,
            &ListCoreMemoryFilter {
                project: project_id.map(|s| s.to_string()),
                limit: Some(50),
                ..Default::default()
            },
        )
        .await?;

        if !core_memories.is_empty() {
            sections.push("core_memories".to_string());
            output.push_str("## Core Memories\n");
            for mem in &core_memories {
                output.push_str(&format!("[{}] {}: {}\n", mem.category, mem.key, mem.value));
            }
            output.push('\n');
        }

        // Also include global core memories
        let global_memories = CoreMemoryRepo::list(
            &self.pool,
            &ListCoreMemoryFilter {
                project: None,
                category: None,
                limit: Some(50),
                offset: None,
            },
        )
        .await?;

        if !global_memories.is_empty() {
            if !sections.contains(&"core_memories".to_string()) {
                sections.push("core_memories".to_string());
            }
            output.push_str("## Global Memories\n");
            for mem in &global_memories {
                output.push_str(&format!("[{}] {}: {}\n", mem.category, mem.key, mem.value));
            }
            output.push('\n');
        }

        let tokens_used = estimate_tokens(&output);
        debug!(
            session_id,
            tokens_used,
            sections = ?sections,
            "built compact restore context"
        );

        Ok(CompactRestoreResult {
            formatted_output: output,
            tokens_used,
            sections_included: sections,
        })
    }
}

/// Truncate a string at a safe UTF-8 boundary.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    alaz_core::truncate_utf8(s, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 100), "");
    }

    #[test]
    fn test_truncate_str_within_limit() {
        assert_eq!(truncate_str("hello world", 100), "hello world");
    }

    #[test]
    fn test_truncate_str_at_ascii_boundary() {
        assert_eq!(truncate_str("abcdef", 3), "abc");
    }

    #[test]
    fn test_truncate_str_utf8_boundary() {
        // 'ü' is 2 bytes. "aü" = 3 bytes. Truncating at 2 would split 'ü'.
        let s = "aüb";
        let result = truncate_str(s, 2);
        // byte 2 is mid-'ü', should back up to byte 1
        assert_eq!(result, "a");
    }

    #[test]
    fn test_truncate_str_zero_max() {
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn test_truncate_str_emoji() {
        // 🦀 is 4 bytes
        let s = "a🦀b";
        // "a" = 1 byte, "🦀" = 4 bytes (pos 1-4), "b" = 1 byte (pos 5)
        let result = truncate_str(s, 3);
        // byte 3 is mid-emoji, should back up to byte 1
        assert_eq!(result, "a");
    }

    #[test]
    fn test_truncate_str_exact_boundary() {
        // "aü" = 3 bytes. Truncating at 3 should include everything.
        let s = "aü";
        assert_eq!(truncate_str(s, 3), "aü");
    }
}
