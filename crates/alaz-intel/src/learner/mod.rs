mod dedup;
mod extraction;
mod outcomes;
mod persist;

use std::sync::Arc;
use std::time::Instant;

use alaz_core::Result;
use alaz_db::repos::{LearningRun, LearningRunRepo};
use alaz_vector::QdrantManager;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::chunking::chunk_transcript;
use crate::domain::ContentDomain;
use crate::embeddings::EmbeddingService;
use crate::llm::LlmClient;
use persist::SaveContext;

/// Maximum transcript size in bytes before truncation.
const MAX_TRANSCRIPT_BYTES: usize = 512 * 1024; // 512KB

/// The core learning pipeline. Called when a session ends to extract
/// patterns, episodes, procedures, and core memories from a transcript.
pub struct SessionLearner {
    pool: PgPool,
    llm: Arc<LlmClient>,
    embedding: Arc<EmbeddingService>,
    qdrant: Arc<QdrantManager>,
}

/// Summary of what was learned from a session.
pub struct LearningSummary {
    pub patterns_saved: usize,
    pub episodes_saved: usize,
    pub procedures_saved: usize,
    pub memories_saved: usize,
    pub contradictions_resolved: usize,
    pub duplicates_skipped: usize,
    pub promotions: usize,
    pub outcomes_recorded: usize,
}

impl SessionLearner {
    /// Create a new session learner.
    pub fn new(
        pool: PgPool,
        llm: Arc<LlmClient>,
        embedding: Arc<EmbeddingService>,
        qdrant: Arc<QdrantManager>,
    ) -> Self {
        Self {
            pool,
            llm,
            embedding,
            qdrant,
        }
    }

    /// Learn from a completed session by extracting knowledge from the transcript.
    ///
    /// Steps:
    /// 1. Truncate transcript to max 512KB
    /// 2. Chunk at ~24KB boundaries on "[USER]:" turn markers
    /// 3. For each chunk, call LLM to extract structured knowledge
    /// 4. Dedup via title substring matching
    /// 5. Save to DB
    /// 6. Create graph edges (session -> entity)
    /// 7. Return summary counts
    pub async fn learn_from_session(
        &self,
        session_id: &str,
        transcript: &str,
        project_id: Option<&str>,
    ) -> Result<LearningSummary> {
        info!(session_id, "starting learning pipeline");
        let pipeline_start = Instant::now();

        let transcript = truncate_utf8(transcript, MAX_TRANSCRIPT_BYTES);
        let chunks = chunk_transcript(transcript);
        let chunks_processed = chunks.len();
        let domain = crate::domain::detect_domain(transcript);
        info!(session_id, %domain, chunk_count = chunks.len(), "chunked transcript");

        // Extract structured knowledge from chunks via parallel LLM calls
        let extracted = self.extract_from_chunks(&chunks, domain).await;

        // Dedup and save to DB
        let ctx = SaveContext {
            session_id: Some(session_id),
            project_id,
            source: None,
        };
        let mut save_result = self.save_all_extracted(&extracted, &ctx).await?;

        // Detect outcomes of existing procedures (skip for very short sessions)
        save_result.summary.outcomes_recorded = if transcript.len() > 500 {
            self.detect_procedure_outcomes(transcript, project_id)
                .await
                .unwrap_or_else(|e| {
                    warn!(error = %e, "procedure outcome detection failed");
                    0
                })
        } else {
            0
        };

        // Tool sequence mining
        save_result.summary.procedures_saved += self
            .save_tool_sequences(transcript, project_id, &mut save_result.session_dedup)
            .await;

        // Generate session reflection with real extracted content
        self.generate_and_save_reflection(
            session_id,
            &extracted,
            &save_result.saved_episode_ids,
            transcript.len(),
            &save_result.summary,
            project_id,
        )
        .await;

        let summary = save_result.summary;
        let duration = pipeline_start.elapsed();

        // Record the learning run for analytics
        let run = LearningRun {
            id: cuid2::create_id(),
            session_id: Some(session_id.to_string()),
            project_id: project_id.map(|s| s.to_string()),
            transcript_size_bytes: transcript.len() as i64,
            chunks_processed: chunks_processed as i32,
            patterns_extracted: summary.patterns_saved as i32,
            episodes_extracted: summary.episodes_saved as i32,
            procedures_extracted: summary.procedures_saved as i32,
            memories_extracted: summary.memories_saved as i32,
            duplicates_skipped: summary.duplicates_skipped as i32,
            contradictions_resolved: summary.contradictions_resolved as i32,
            duration_ms: duration.as_millis() as i64,
            created_at: chrono::Utc::now(),
        };
        if let Err(e) = LearningRunRepo::record(&self.pool, &run).await {
            warn!(error = %e, "failed to record learning run");
        }

        info!(
            session_id,
            patterns = summary.patterns_saved,
            episodes = summary.episodes_saved,
            procedures = summary.procedures_saved,
            memories = summary.memories_saved,
            duplicates = summary.duplicates_skipped,
            contradictions = summary.contradictions_resolved,
            promotions = summary.promotions,
            outcomes = summary.outcomes_recorded,
            duration_ms = duration.as_millis() as u64,
            "learning pipeline completed"
        );

        Ok(summary)
    }

    /// Learn from arbitrary content (generalized version of learn_from_session).
    ///
    /// This is the universal entry point for the JARVIS system. It accepts content
    /// from any source (mobile notes, web clips, voice memos, etc.) and runs
    /// domain-aware extraction.
    pub async fn learn_from_content(
        &self,
        content: &str,
        source: &str,
        domain: ContentDomain,
        project_id: Option<&str>,
    ) -> Result<LearningSummary> {
        info!(source, domain = %domain, "starting content learning pipeline");
        let pipeline_start = Instant::now();

        let content = truncate_utf8(content, MAX_TRANSCRIPT_BYTES);
        let chunks = chunk_transcript(content);
        let chunks_processed = chunks.len();
        info!(chunk_count = chunks.len(), "chunked content");

        // Extract structured knowledge from chunks via parallel LLM calls
        let extracted = self.extract_from_chunks(&chunks, domain).await;

        // Dedup and save to DB
        let ctx = SaveContext {
            session_id: None,
            project_id,
            source: Some(source),
        };
        let save_result = self.save_all_extracted(&extracted, &ctx).await?;

        let summary = save_result.summary;
        let duration = pipeline_start.elapsed();

        // Record the learning run for analytics
        let run = LearningRun {
            id: cuid2::create_id(),
            session_id: None,
            project_id: project_id.map(|s| s.to_string()),
            transcript_size_bytes: content.len() as i64,
            chunks_processed: chunks_processed as i32,
            patterns_extracted: summary.patterns_saved as i32,
            episodes_extracted: summary.episodes_saved as i32,
            procedures_extracted: summary.procedures_saved as i32,
            memories_extracted: summary.memories_saved as i32,
            duplicates_skipped: summary.duplicates_skipped as i32,
            contradictions_resolved: summary.contradictions_resolved as i32,
            duration_ms: duration.as_millis() as i64,
            created_at: chrono::Utc::now(),
        };
        if let Err(e) = LearningRunRepo::record(&self.pool, &run).await {
            warn!(error = %e, "failed to record learning run");
        }

        info!(
            source,
            patterns = summary.patterns_saved,
            episodes = summary.episodes_saved,
            procedures = summary.procedures_saved,
            memories = summary.memories_saved,
            duplicates = summary.duplicates_skipped,
            duration_ms = duration.as_millis() as u64,
            "content learning pipeline completed"
        );

        Ok(summary)
    }
}

/// Truncate a string to at most `max_bytes`, respecting UTF-8 char boundaries.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    warn!(
        original_len = s.len(),
        truncated_to = max_bytes,
        "content truncated"
    );
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
