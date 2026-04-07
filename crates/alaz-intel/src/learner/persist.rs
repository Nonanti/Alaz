use alaz_core::Result;
use alaz_core::models::{
    CreateEpisode, CreateKnowledge, CreateProcedure, CreateReflection, CreateRelation,
    UpsertCoreMemory,
};
use alaz_db::repos::ReflectionRepo;
use alaz_db::repos::{CoreMemoryRepo, EpisodeRepo, GraphRepo, KnowledgeRepo, ProcedureRepo};
use tracing::{debug, warn};

use crate::contradiction::{ContradictionDetector, ContradictionResult};
use crate::reflection::ReflectionGenerator;
use crate::tool_mining::ToolSequenceMiner;

use super::LearningSummary;
use super::dedup::SessionDedup;
use super::extraction::AggregatedExtraction;

/// Options controlling how extracted items are persisted.
pub(crate) struct SaveContext<'a> {
    /// If set, creates graph edges from session to produced entities.
    pub session_id: Option<&'a str>,
    /// Project scope for dedup and save operations.
    pub project_id: Option<&'a str>,
    /// Source identifier (e.g., "mobile", "web-clip").
    pub source: Option<&'a str>,
}

/// Result of the save operation, carrying accumulated counts and state.
pub(crate) struct SaveResult {
    pub summary: LearningSummary,
    pub saved_episode_ids: Vec<String>,
    pub session_dedup: SessionDedup,
}

impl super::SessionLearner {
    /// Save all extracted items to the database with dedup, contradiction detection,
    /// and optional session graph edges.
    pub(crate) async fn save_all_extracted(
        &self,
        extracted: &AggregatedExtraction,
        ctx: &SaveContext<'_>,
    ) -> Result<SaveResult> {
        let mut summary = LearningSummary {
            patterns_saved: 0,
            episodes_saved: 0,
            procedures_saved: 0,
            memories_saved: 0,
            contradictions_resolved: 0,
            duplicates_skipped: 0,
            promotions: 0,
            outcomes_recorded: 0,
        };
        let mut session_dedup = SessionDedup::new();
        let mut saved_episode_ids: Vec<String> = Vec::new();

        let source_metadata = ctx.source.map(|_| serde_json::json!({}));

        // --- Save patterns (as knowledge items) ---
        for pattern in &extracted.patterns {
            if self
                .is_duplicate_knowledge(&pattern.title, ctx.project_id, &session_dedup)
                .await?
            {
                debug!(title = %pattern.title, "skipping duplicate pattern");
                summary.duplicates_skipped += 1;
                continue;
            }

            let input = CreateKnowledge {
                title: pattern.title.clone(),
                content: pattern.content.clone(),
                description: None,
                kind: Some("pattern".to_string()),
                language: pattern.language.clone(),
                file_path: None,
                project: None,
                tags: Some(pattern.tags.clone()),
                valid_from: None,
                valid_until: None,
                source: ctx.source.map(|s| s.to_string()),
                source_metadata: source_metadata.clone(),
            };

            match KnowledgeRepo::create(&self.pool, &input, ctx.project_id).await {
                Ok(item) => {
                    if let Some(sid) = ctx.session_id {
                        self.create_session_edge(sid, "knowledge_item", &item.id)
                            .await;
                    }
                    summary.patterns_saved += 1;

                    // Record in session dedup buffer
                    if let Ok(vecs) = self.embedding.embed_text(&[pattern.title.as_str()]).await
                        && let Some(vec) = vecs.into_iter().next()
                    {
                        session_dedup.record("knowledge_item", vec);
                    }

                    // Contradiction check
                    let detector = ContradictionDetector::new(self.pool.clone(), self.llm.clone());
                    match detector
                        .check(&pattern.title, &pattern.content, ctx.project_id)
                        .await
                    {
                        Ok(Some(result)) => match result {
                            ContradictionResult::Contradiction { old_id, .. }
                            | ContradictionResult::Update { old_id, .. } => {
                                if let Err(e) =
                                    KnowledgeRepo::supersede(&self.pool, &old_id, &item.id, None)
                                        .await
                                {
                                    warn!(error = %e, old_id = %old_id, "failed to supersede");
                                } else {
                                    summary.contradictions_resolved += 1;
                                }
                            }
                            _ => {}
                        },
                        Ok(None) => {}
                        Err(e) => warn!(error = %e, "contradiction check failed"),
                    }

                    // Cross-project promotion check (only for session-based learning)
                    if ctx.session_id.is_some() {
                        match alaz_graph::check_and_promote(&self.pool, &item.id, &pattern.title)
                            .await
                        {
                            Ok(Some(_)) => summary.promotions += 1,
                            Ok(None) => {}
                            Err(e) => {
                                warn!(error = %e, "cross-project promotion check failed");
                            }
                        }
                    }
                }
                Err(e) => warn!(title = %pattern.title, error = %e, "failed to save pattern"),
            }
        }

        // --- Save episodes and auto-chain them sequentially ---
        for episode in &extracted.episodes {
            if self
                .is_duplicate_episode(&episode.title, ctx.project_id, &session_dedup)
                .await?
            {
                debug!(title = %episode.title, "skipping duplicate episode");
                summary.duplicates_skipped += 1;
                continue;
            }

            let input = CreateEpisode {
                title: episode.title.clone(),
                content: episode.content.clone(),
                kind: episode.kind.clone(),
                severity: episode.severity.clone(),
                resolved: Some(false),
                who_cues: Some(episode.who_cues.clone()),
                what_cues: Some(episode.what_cues.clone()),
                where_cues: Some(episode.where_cues.clone()),
                when_cues: Some(episode.when_cues.clone()),
                why_cues: Some(episode.why_cues.clone()),
                project: None,
                source: ctx.source.map(|s| s.to_string()),
                source_metadata: source_metadata.clone(),
                action: None,
                outcome: None,
                outcome_score: None,
                related_files: None,
            };

            match EpisodeRepo::create(&self.pool, &input, ctx.project_id).await {
                Ok(ep) => {
                    if let Some(sid) = ctx.session_id {
                        self.create_session_edge(sid, "episode", &ep.id).await;
                    }
                    saved_episode_ids.push(ep.id);
                    summary.episodes_saved += 1;

                    // Record in session dedup buffer
                    if let Ok(vecs) = self.embedding.embed_text(&[episode.title.as_str()]).await
                        && let Some(vec) = vecs.into_iter().next()
                    {
                        session_dedup.record("episode", vec);
                    }
                }
                Err(e) => warn!(title = %episode.title, error = %e, "failed to save episode"),
            }
        }

        // Auto-chain: link episodes sequentially with "led_to" edges
        if ctx.session_id.is_some() {
            for window in saved_episode_ids.windows(2) {
                let relation = CreateRelation {
                    source_type: "episode".to_string(),
                    source_id: window[0].clone(),
                    target_type: "episode".to_string(),
                    target_id: window[1].clone(),
                    relation: "led_to".to_string(),
                    weight: Some(1.0),
                    description: None,
                    metadata: None,
                };
                if let Err(e) = GraphRepo::create_edge(&self.pool, &relation).await {
                    warn!(error = %e, "failed to auto-chain episodes");
                }
            }
        }

        // --- Save procedures ---
        for procedure in &extracted.procedures {
            if self
                .is_duplicate_procedure(&procedure.title, ctx.project_id, &session_dedup)
                .await?
            {
                debug!(title = %procedure.title, "skipping duplicate procedure");
                summary.duplicates_skipped += 1;
                continue;
            }

            let steps_json =
                serde_json::to_value(&procedure.steps).unwrap_or(serde_json::Value::Array(vec![]));

            let input = CreateProcedure {
                title: procedure.title.clone(),
                content: procedure.content.clone(),
                steps: Some(steps_json),
                project: None,
                tags: None,
                source: ctx.source.map(|s| s.to_string()),
                source_metadata: source_metadata.clone(),
            };

            match ProcedureRepo::create(&self.pool, &input, ctx.project_id).await {
                Ok(proc_item) => {
                    if let Some(sid) = ctx.session_id {
                        self.create_session_edge(sid, "procedure", &proc_item.id)
                            .await;
                    }
                    summary.procedures_saved += 1;

                    // Record in session dedup buffer
                    if let Ok(vecs) = self.embedding.embed_text(&[procedure.title.as_str()]).await
                        && let Some(vec) = vecs.into_iter().next()
                    {
                        session_dedup.record("procedure", vec);
                    }
                }
                Err(e) => warn!(title = %procedure.title, error = %e, "failed to save procedure"),
            }
        }

        // --- Save core memories (with key normalization via similarity check) ---
        for memory in &extracted.core_memories {
            let effective_key = match self
                .find_existing_memory_key(&memory.category, &memory.key, ctx.project_id)
                .await
            {
                Ok(Some(existing_key)) => {
                    debug!(
                        new_key = %memory.key,
                        existing_key = %existing_key,
                        "normalizing core memory key to existing"
                    );
                    existing_key
                }
                _ => memory.key.clone(),
            };

            let input = UpsertCoreMemory {
                category: memory.category.clone(),
                key: effective_key,
                value: memory.value.clone(),
                confidence: Some(0.8),
                project: None,
            };

            match CoreMemoryRepo::upsert(&self.pool, &input, ctx.project_id).await {
                Ok(_) => summary.memories_saved += 1,
                Err(e) => {
                    warn!(key = %memory.key, error = %e, "failed to save core memory");
                }
            }
        }

        Ok(SaveResult {
            summary,
            saved_episode_ids,
            session_dedup,
        })
    }

    /// Mine tool sequences from transcript and save as procedures.
    pub(crate) async fn save_tool_sequences(
        &self,
        transcript: &str,
        project_id: Option<&str>,
        session_dedup: &mut SessionDedup,
    ) -> usize {
        let sequences = ToolSequenceMiner::mine(transcript).unwrap_or_default();
        let mut saved = 0usize;

        for (tools, frequency) in sequences.iter().take(5) {
            let title = format!("Tool sequence: {}", tools.join(" → "));

            // Check for existing similar procedure before creating
            match self
                .is_duplicate_procedure(&title, project_id, session_dedup)
                .await
            {
                Ok(true) => {
                    debug!(title = %title, "skipping duplicate tool sequence");
                    continue;
                }
                Err(e) => {
                    warn!(error = %e, "dedup check failed for tool sequence");
                    continue;
                }
                Ok(false) => {}
            }

            let content = format!(
                "Sequence of {} tools used {} times: {}",
                tools.len(),
                frequency,
                tools.join(" → ")
            );
            let steps = serde_json::to_value(tools).unwrap_or_default();

            let input = CreateProcedure {
                title,
                content,
                steps: Some(steps),
                project: None,
                tags: Some(vec!["tool-sequence".to_string()]),
                source: None,
                source_metadata: None,
            };

            match ProcedureRepo::create(&self.pool, &input, project_id).await {
                Ok(_) => saved += 1,
                Err(e) => warn!(error = %e, "failed to save tool sequence"),
            }
        }

        saved
    }

    /// Generate and save a session reflection from extracted content.
    pub(crate) async fn generate_and_save_reflection(
        &self,
        session_id: &str,
        extracted: &AggregatedExtraction,
        saved_episode_ids: &[String],
        transcript_len: usize,
        summary: &LearningSummary,
        project_id: Option<&str>,
    ) {
        let reflector = ReflectionGenerator::new(self.llm.clone());
        let summary_text = build_reflection_input(extracted, summary, transcript_len);

        match reflector.generate(&summary_text).await {
            Ok(reflection) => {
                let reflection_input = CreateReflection {
                    session_id: session_id.to_string(),
                    what_worked: Some(reflection.what_worked.clone()),
                    what_failed: Some(reflection.what_failed.clone()),
                    lessons_learned: Some(reflection.lessons_learned.clone()),
                    effectiveness_score: Some(reflection.effectiveness_score),
                    complexity_score: Some(reflection.complexity_score),
                    kind: Some("session_end".to_string()),
                    action_items: Some(
                        reflection
                            .action_items
                            .iter()
                            .map(|a| alaz_core::models::ActionItem {
                                description: a.description.clone(),
                                status: a.status.clone(),
                                priority: a.priority.clone(),
                            })
                            .collect(),
                    ),
                    overall_score: Some(reflection.overall_score),
                    knowledge_score: Some(reflection.knowledge_score),
                    decision_score: Some(reflection.decision_score),
                    efficiency_score: Some(reflection.efficiency_score),
                    evaluated_episode_ids: Some(saved_episode_ids.to_vec()),
                    project: None,
                };
                // Only save if session exists in DB (avoid FK violation for ad-hoc learn calls)
                match alaz_db::repos::SessionRepo::exists(&self.pool, session_id).await {
                    Ok(true) => {
                        if let Err(e) =
                            ReflectionRepo::create(&self.pool, &reflection_input, project_id).await
                        {
                            warn!(error = %e, "failed to save reflection");
                        }
                    }
                    _ => {
                        debug!(session_id, "skipping reflection save — session not in DB");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "reflection generation failed");
            }
        }
    }

    /// Create a graph edge from session to a produced entity.
    pub(crate) async fn create_session_edge(
        &self,
        session_id: &str,
        target_type: &str,
        target_id: &str,
    ) {
        let relation = CreateRelation {
            source_type: "session".to_string(),
            source_id: session_id.to_string(),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            relation: "produced_by".to_string(),
            weight: Some(1.0),
            description: None,
            metadata: None,
        };

        if let Err(e) = GraphRepo::create_edge(&self.pool, &relation).await {
            warn!(
                session_id,
                target_type,
                target_id,
                error = %e,
                "failed to create session graph edge"
            );
        }
    }
}

/// Build the text summary passed to the reflection generator.
fn build_reflection_input(
    extracted: &AggregatedExtraction,
    summary: &LearningSummary,
    transcript_len: usize,
) -> String {
    let mut parts = Vec::new();

    if !extracted.patterns.is_empty() {
        parts.push("## Patterns Extracted".to_string());
        for p in &extracted.patterns {
            let truncated: String = p.content.chars().take(150).collect();
            parts.push(format!("- {}: {}", p.title, truncated));
        }
    }
    if !extracted.episodes.is_empty() {
        parts.push("## Episodes".to_string());
        for e in &extracted.episodes {
            let truncated: String = e.content.chars().take(150).collect();
            parts.push(format!(
                "- [{}] {}: {}",
                e.kind.as_deref().unwrap_or("discovery"),
                e.title,
                truncated
            ));
        }
    }
    if !extracted.procedures.is_empty() {
        parts.push("## Procedures".to_string());
        for p in &extracted.procedures {
            parts.push(format!("- {}", p.title));
        }
    }
    if !extracted.core_memories.is_empty() {
        parts.push("## Core Memories".to_string());
        for m in &extracted.core_memories {
            parts.push(format!("- [{}] {}: {}", m.category, m.key, m.value));
        }
    }
    parts.push(format!(
        "\nStats: {} patterns, {} episodes, {} procedures, {} memories from {} bytes",
        summary.patterns_saved,
        summary.episodes_saved,
        summary.procedures_saved,
        summary.memories_saved,
        transcript_len
    ));
    parts.join("\n")
}
