use alaz_core::models::{
    ListCoreMemoryFilter, ListEpisodesFilter, ListKnowledgeFilter, ListProceduresFilter,
    ListReflectionsFilter, ListSessionsFilter,
};
use alaz_core::{Result, estimate_tokens};
use alaz_db::repos::{
    CoreMemoryRepo, EpisodeRepo, KnowledgeRepo, ProcedureRepo, ReflectionRepo, SessionRepo,
};
use sqlx::PgPool;
use tracing::debug;

/// Result from context building with token tracking.
pub struct ContextResult {
    /// The assembled context string.
    pub context: String,
    /// Estimated tokens used by the context.
    pub tokens_used: u64,
    /// Names of sections that were included.
    pub sections_included: Vec<String>,
    /// IDs of entities that were injected into context.
    pub injected_entity_ids: Vec<String>,
}

/// Builds priority-based context for injecting into LLM prompts.
///
/// Assembles knowledge from the database in priority order within a
/// token budget (~8K tokens, ~32K chars).
pub struct ContextInjector {
    pool: PgPool,
}

/// Maximum context budget in characters (~8K tokens).
const MAX_CONTEXT_CHARS: usize = 32_000;

impl ContextInjector {
    /// Create a new context injector.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Build a priority-based context string for a given project.
    ///
    /// Priority levels:
    /// - P0 (mandatory): All core memories for project
    /// - P1 (high): Unresolved episodes + recent errored sessions (last 5)
    /// - P2 (medium): Project patterns, proven procedures (Wilson score >0.3),
    ///   global patterns, recent reflections (last 3), cross-project intelligence
    /// - P3 (low): Recent sessions (last 5), recent code snippets (last 5)
    pub async fn build_context(&self, project_path: &str) -> Result<String> {
        let result = self.build_context_with_tracking(project_path).await?;
        Ok(result.context)
    }

    /// Build context with detailed token tracking per section.
    pub async fn build_context_with_tracking(&self, project_path: &str) -> Result<ContextResult> {
        let mut context = String::with_capacity(MAX_CONTEXT_CHARS);
        let mut budget_remaining = MAX_CONTEXT_CHARS;
        let mut sections_included = Vec::new();
        let mut injected_entity_ids = Vec::new();

        // Resolve project name to project ID for filtering
        let project_name = std::path::Path::new(project_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(project_path);

        let project_id = alaz_db::repos::ProjectRepo::get_by_name(&self.pool, project_name)
            .await
            .ok()
            .flatten()
            .map(|p| p.id);

        let pid = project_id.as_deref();

        /// Append a section to context if it fits within budget.
        /// Also collects entity IDs when provided.
        macro_rules! append_section {
            ($section_opt:expr, $name:expr) => {
                if let Some(section) = $section_opt {
                    if section.len() <= budget_remaining {
                        context.push_str(&section);
                        budget_remaining -= section.len();
                        sections_included.push($name.to_string());
                    }
                }
            };
            ($section_opt:expr, $name:expr, $ids:expr) => {
                if let Some(section) = $section_opt {
                    if section.len() <= budget_remaining {
                        context.push_str(&section);
                        budget_remaining -= section.len();
                        sections_included.push($name.to_string());
                        injected_entity_ids.extend($ids.into_iter());
                    }
                }
            };
        }

        // === Priority 0: Core memories (mandatory) ===
        let (p0_core, p0_global) = tokio::join!(
            self.build_core_memories_section_tracked(pid),
            self.build_global_memories_section_tracked(),
        );
        let (core_section, core_ids) = p0_core?;
        append_section!(core_section, "core_memories", core_ids);
        let (global_section, global_ids) = p0_global?;
        append_section!(global_section, "global_memories", global_ids);

        // === Priority 1: Unresolved episodes + errored sessions + seed query ===
        let (p1_episodes, p1_errors, p1_seed) = tokio::join!(
            self.build_unresolved_episodes_section_tracked(pid),
            self.build_errored_sessions_section_tracked(pid),
            self.build_seed_query(pid),
        );
        if budget_remaining > 0 {
            let (section, ids) = p1_episodes?;
            append_section!(section, "unresolved_episodes", ids);
        }
        if budget_remaining > 0 {
            let (section, ids) = p1_errors?;
            append_section!(section, "errored_sessions", ids);
        }
        let seed_query = p1_seed?.unwrap_or_default();

        // === Priority 2: Patterns, procedures, reflections ===
        let (p2_patterns, p2_procedures, p2_global, p2_reflections) = tokio::join!(
            self.build_patterns_section_tracked(pid, &seed_query),
            self.build_procedures_section_tracked(pid),
            self.build_global_patterns_section_tracked(),
            self.build_reflections_section_tracked(pid),
        );
        if budget_remaining > 0 {
            let (section, ids) = p2_patterns?;
            append_section!(section, "project_patterns", ids);
        }
        if budget_remaining > 0 {
            let (section, ids) = p2_procedures?;
            append_section!(section, "proven_procedures", ids);
        }
        if budget_remaining > 0 {
            let (section, ids) = p2_global?;
            append_section!(section, "global_patterns", ids);
        }
        if budget_remaining > 0 {
            let (section, ids) = p2_reflections?;
            append_section!(section, "reflections", ids);
        }

        // === Priority 3: Recent sessions, code snippets, review due ===
        let (p3_sessions, p3_snippets, p3_review) = tokio::join!(
            self.build_recent_sessions_section_tracked(pid),
            self.build_code_snippets_section_tracked(pid, &seed_query),
            self.build_review_due_section(pid),
        );
        if budget_remaining > 0 {
            let (section, ids) = p3_sessions?;
            append_section!(section, "recent_sessions", ids);
        }
        if budget_remaining > 0 {
            let (section, ids) = p3_snippets?;
            append_section!(section, "code_snippets", ids);
        }

        let tokens_used = estimate_tokens(&context);

        if budget_remaining > 0 {
            append_section!(p3_review?, "review_due");
        }

        let _ = budget_remaining; // suppress unused warning

        debug!(
            project = %project_path,
            context_len = context.len(),
            tokens_used,
            sections = ?sections_included,
            injected_entities = injected_entity_ids.len(),
            "built context"
        );

        Ok(ContextResult {
            context,
            tokens_used,
            sections_included,
            injected_entity_ids,
        })
    }

    // --- Private helper methods ---

    /// Build seed query from recent session summaries for semantic relevance.
    async fn build_seed_query(&self, project_id: Option<&str>) -> Result<Option<String>> {
        let recent = SessionRepo::list(
            &self.pool,
            &ListSessionsFilter {
                project: project_id.map(String::from),
                limit: Some(3),
                ..Default::default()
            },
        )
        .await
        .unwrap_or_default();

        let query: String = recent
            .iter()
            .filter_map(|s| s.summary.as_deref())
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(500)
            .collect();

        if query.is_empty() {
            Ok(None)
        } else {
            Ok(Some(query))
        }
    }

    // --- Tracked variants: return (Option<String>, Vec<String>) with entity IDs ---

    async fn build_core_memories_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let core_memories = CoreMemoryRepo::list(
            &self.pool,
            &ListCoreMemoryFilter {
                project: project_id.map(String::from),
                limit: Some(50),
                ..Default::default()
            },
        )
        .await?;

        if core_memories.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = core_memories.iter().map(|m| m.id.clone()).collect();
        Ok((
            Some(format_section(
                "CORE MEMORIES",
                &core_memories
                    .iter()
                    .map(|m| format!("[{}] {}: {}", m.category, m.key, m.value))
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_global_memories_section_tracked(&self) -> Result<(Option<String>, Vec<String>)> {
        let global_memories = CoreMemoryRepo::list_global(&self.pool, 50).await?;

        if global_memories.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = global_memories.iter().map(|m| m.id.clone()).collect();
        Ok((
            Some(format_section(
                "GLOBAL MEMORIES",
                &global_memories
                    .iter()
                    .map(|m| format!("[{}] {}: {}", m.category, m.key, m.value))
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_unresolved_episodes_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let unresolved_episodes = EpisodeRepo::list(
            &self.pool,
            &ListEpisodesFilter {
                project: project_id.map(String::from),
                resolved: Some(false),
                limit: Some(10),
                ..Default::default()
            },
        )
        .await?;

        if unresolved_episodes.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = unresolved_episodes.iter().map(|e| e.id.clone()).collect();
        Ok((
            Some(format_section(
                "UNRESOLVED ISSUES",
                &unresolved_episodes
                    .iter()
                    .map(|e| format!("[{}] {}: {}", e.kind, e.title, truncate(&e.content, 200)))
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_errored_sessions_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let errored_sessions = SessionRepo::list(
            &self.pool,
            &ListSessionsFilter {
                project: project_id.map(String::from),
                status: Some("error".to_string()),
                limit: Some(5),
                ..Default::default()
            },
        )
        .await?;

        if errored_sessions.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = errored_sessions.iter().map(|s| s.id.clone()).collect();
        Ok((
            Some(format_section(
                "RECENT ERRORS",
                &errored_sessions
                    .iter()
                    .filter_map(|s| {
                        s.summary
                            .as_ref()
                            .map(|sum| format!("[{}] {}", s.id, truncate(sum, 200)))
                    })
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_patterns_section_tracked(
        &self,
        project_id: Option<&str>,
        seed_query: &str,
    ) -> Result<(Option<String>, Vec<String>)> {
        let patterns = if !seed_query.is_empty() {
            let fts_results = KnowledgeRepo::fts_search(&self.pool, seed_query, project_id, 20)
                .await
                .unwrap_or_default();

            let fetch_ids: Vec<_> = fts_results
                .iter()
                .take(20)
                .map(|(id, _, _)| id.clone())
                .collect();
            let items = KnowledgeRepo::get_many(&self.pool, &fetch_ids)
                .await
                .unwrap_or_default();
            let found: Vec<_> = items
                .into_iter()
                .filter(|item| item.kind == "pattern")
                .take(10)
                .collect();
            if found.is_empty() {
                KnowledgeRepo::list(
                    &self.pool,
                    &ListKnowledgeFilter {
                        project: project_id.map(String::from),
                        kind: Some("pattern".to_string()),
                        limit: Some(10),
                        ..Default::default()
                    },
                )
                .await?
            } else {
                found
            }
        } else {
            KnowledgeRepo::list(
                &self.pool,
                &ListKnowledgeFilter {
                    project: project_id.map(String::from),
                    kind: Some("pattern".to_string()),
                    limit: Some(10),
                    ..Default::default()
                },
            )
            .await?
        };

        if patterns.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = patterns.iter().map(|p| p.id.clone()).collect();
        Ok((
            Some(format_section(
                "PROJECT PATTERNS",
                &patterns
                    .iter()
                    .map(|p| format!("## {}\n{}", p.title, truncate(&p.content, 300)))
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_procedures_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let procedures = ProcedureRepo::list(
            &self.pool,
            &ListProceduresFilter {
                project: project_id.map(String::from),
                limit: Some(10),
                ..Default::default()
            },
        )
        .await?;

        let proven: Vec<_> = procedures
            .iter()
            .filter(|p| p.success_rate.unwrap_or(0.0) > 0.3)
            .collect();

        if proven.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = proven.iter().map(|p| p.id.clone()).collect();
        Ok((
            Some(format_section(
                "PROVEN PROCEDURES",
                &proven
                    .iter()
                    .map(|p| {
                        format!(
                            "## {} (confidence: {:.0}%, {}/{} runs)\n{}",
                            p.title,
                            p.success_rate.unwrap_or(0.0) * 100.0,
                            p.success,
                            p.times_used,
                            truncate(&p.content, 300)
                        )
                    })
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_global_patterns_section_tracked(&self) -> Result<(Option<String>, Vec<String>)> {
        let global_patterns = KnowledgeRepo::list_global(&self.pool, "pattern", 5).await?;

        if global_patterns.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = global_patterns.iter().map(|p| p.id.clone()).collect();
        Ok((
            Some(format_section(
                "GLOBAL PATTERNS",
                &global_patterns
                    .iter()
                    .map(|p| format!("## {}\n{}", p.title, truncate(&p.content, 200)))
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_reflections_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let pid = match project_id {
            Some(pid) => pid,
            None => return Ok((None, vec![])),
        };

        let reflections = ReflectionRepo::list(
            &self.pool,
            &ListReflectionsFilter {
                project: Some(pid.to_string()),
                limit: Some(3),
                ..Default::default()
            },
        )
        .await?;

        if reflections.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = reflections.iter().map(|r| r.id.clone()).collect();
        Ok((
            Some(format_section(
                "RECENT REFLECTIONS",
                &reflections
                    .iter()
                    .map(|r| {
                        let mut parts = Vec::new();
                        if let Some(ref worked) = r.what_worked {
                            parts.push(format!("Worked: {}", truncate(worked, 150)));
                        }
                        if let Some(ref failed) = r.what_failed {
                            parts.push(format!("Failed: {}", truncate(failed, 150)));
                        }
                        if let Some(ref lessons) = r.lessons_learned {
                            parts.push(format!("Lessons: {}", truncate(lessons, 150)));
                        }
                        parts.join("\n")
                    })
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_recent_sessions_section_tracked(
        &self,
        project_id: Option<&str>,
    ) -> Result<(Option<String>, Vec<String>)> {
        let recent_sessions = SessionRepo::list(
            &self.pool,
            &ListSessionsFilter {
                project: project_id.map(String::from),
                limit: Some(5),
                ..Default::default()
            },
        )
        .await?;

        if recent_sessions.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = recent_sessions.iter().map(|s| s.id.clone()).collect();
        Ok((
            Some(format_section(
                "RECENT SESSIONS",
                &recent_sessions
                    .iter()
                    .filter_map(|s| {
                        s.summary.as_ref().map(|sum| {
                            format!(
                                "[{}] status={} {}",
                                s.id,
                                s.status.as_deref().unwrap_or("unknown"),
                                truncate(sum, 150)
                            )
                        })
                    })
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    async fn build_code_snippets_section_tracked(
        &self,
        project_id: Option<&str>,
        seed_query: &str,
    ) -> Result<(Option<String>, Vec<String>)> {
        let code_snippets = if !seed_query.is_empty() {
            let fts_results = KnowledgeRepo::fts_search(&self.pool, seed_query, project_id, 20)
                .await
                .unwrap_or_default();

            let fetch_ids: Vec<_> = fts_results
                .iter()
                .take(20)
                .map(|(id, _, _)| id.clone())
                .collect();
            let items = KnowledgeRepo::get_many(&self.pool, &fetch_ids)
                .await
                .unwrap_or_default();
            let found: Vec<_> = items
                .into_iter()
                .filter(|item| item.kind == "artifact")
                .take(10)
                .collect();
            if found.is_empty() {
                KnowledgeRepo::list(
                    &self.pool,
                    &ListKnowledgeFilter {
                        project: project_id.map(String::from),
                        kind: Some("artifact".to_string()),
                        limit: Some(5),
                        ..Default::default()
                    },
                )
                .await?
            } else {
                found
            }
        } else {
            KnowledgeRepo::list(
                &self.pool,
                &ListKnowledgeFilter {
                    project: project_id.map(String::from),
                    kind: Some("artifact".to_string()),
                    limit: Some(5),
                    ..Default::default()
                },
            )
            .await?
        };

        if code_snippets.is_empty() {
            return Ok((None, vec![]));
        }

        let ids: Vec<String> = code_snippets.iter().map(|s| s.id.clone()).collect();
        Ok((
            Some(format_section(
                "RECENT CODE SNIPPETS",
                &code_snippets
                    .iter()
                    .map(|s| {
                        format!(
                            "## {} [{}]\n{}",
                            s.title,
                            s.language.as_deref().unwrap_or("unknown"),
                            truncate(&s.content, 300)
                        )
                    })
                    .collect::<Vec<_>>(),
            )),
            ids,
        ))
    }

    /// P4: Spaced repetition review items.
    async fn build_review_due_section(&self, project_id: Option<&str>) -> Result<Option<String>> {
        let due_items = crate::evolution::items_due_for_review(&self.pool, project_id, 3)
            .await
            .unwrap_or_default();

        if due_items.is_empty() {
            return Ok(None);
        }

        Ok(Some(format_section(
            "REVIEW DUE (Spaced Repetition)",
            &due_items
                .iter()
                .map(|item| {
                    format!(
                        "- **{}** (`{}`): {}",
                        item.title,
                        item.id,
                        truncate(&item.content, 80),
                    )
                })
                .collect::<Vec<_>>(),
        )))
    }
}

/// Format a section with a header and items.
fn format_section(header: &str, items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }

    let mut section = format!("\n=== {} ===\n", header);
    for item in items {
        section.push_str(item);
        section.push('\n');
    }
    section.push('\n');
    section
}

/// Truncate a string to a maximum number of characters.
fn truncate(s: &str, max_chars: usize) -> &str {
    alaz_core::truncate_utf8(s, max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === truncate tests ===

    #[test]
    fn test_truncate_empty_string() {
        assert_eq!(truncate("", 100), "");
    }

    #[test]
    fn test_truncate_ascii_within_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_ascii_at_limit() {
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_utf8_multibyte_at_boundary() {
        // 'İ' is 2 bytes (0xC4 0xB0). Truncating at byte 1 should back up.
        let s = "İstanbul";
        let result = truncate(s, 1);
        // Should back up to 0 since byte 1 is mid-char
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_string_shorter_than_max() {
        let s = "short";
        assert_eq!(truncate(s, 1000), "short");
    }

    #[test]
    fn test_truncate_turkish_chars() {
        // ğ = 2 bytes, ü = 2 bytes, ş = 2 bytes
        let s = "güneş"; // g(1) ü(2) n(1) e(1) ş(2) = 7 bytes
        let result = truncate(s, 4);
        // Byte 4 is in the middle of 'e' or 'n' — let's check
        // g=1byte, ü=2bytes(pos1-2), n=1byte(pos3), e=1byte(pos4) -> boundary at 4 is valid
        assert_eq!(result, "gün");
    }

    // === format_section tests ===

    #[test]
    fn test_format_section_empty_items() {
        let result = format_section("HEADER", &[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_section_single_item() {
        let items = vec!["item one".to_string()];
        let result = format_section("MY HEADER", &items);
        assert!(result.contains("=== MY HEADER ==="));
        assert!(result.contains("item one"));
    }

    #[test]
    fn test_format_section_multiple_items() {
        let items = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];
        let result = format_section("TEST", &items);
        assert!(result.contains("=== TEST ==="));
        assert!(result.contains("first\n"));
        assert!(result.contains("second\n"));
        assert!(result.contains("third\n"));
    }

    #[test]
    fn test_format_section_header_formatting() {
        let items = vec!["x".to_string()];
        let result = format_section("CORE MEMORIES", &items);
        // Header should be wrapped in === ... ===
        assert!(result.starts_with("\n=== CORE MEMORIES ===\n"));
        // Should end with double newline
        assert!(result.ends_with("\n\n"));
    }

    // === ContextResult tests ===

    #[test]
    fn test_context_result_has_injected_entity_ids() {
        let result = ContextResult {
            context: "test context".to_string(),
            tokens_used: 42,
            sections_included: vec!["core_memories".to_string()],
            injected_entity_ids: vec!["id1".to_string(), "id2".to_string()],
        };

        assert_eq!(result.injected_entity_ids.len(), 2);
        assert_eq!(result.injected_entity_ids[0], "id1");
        assert_eq!(result.injected_entity_ids[1], "id2");
    }

    #[test]
    fn test_context_result_empty_entity_ids() {
        let result = ContextResult {
            context: String::new(),
            tokens_used: 0,
            sections_included: vec![],
            injected_entity_ids: vec![],
        };

        assert!(result.injected_entity_ids.is_empty());
        assert!(result.sections_included.is_empty());
    }
}
