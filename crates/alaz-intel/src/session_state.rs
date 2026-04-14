//! Session state extraction via LLM.
//!
//! Analyzes the tail of a session transcript to extract structured state:
//! goals, accomplished items, pending items, a handoff summary, and current task.

use std::sync::Arc;

use alaz_core::Result;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::llm::LlmClient;

/// Structured session state extracted from a transcript.
#[derive(Debug, Default, Deserialize, serde::Serialize, Clone)]
pub struct ExtractedSessionState {
    /// High-level goals identified in the session.
    #[serde(default)]
    pub goals: Vec<String>,
    /// Items that were completed during the session.
    #[serde(default)]
    pub accomplished: Vec<String>,
    /// Items that remain unfinished.
    #[serde(default)]
    pub pending: Vec<String>,
    /// A concise summary suitable for handing off to a new session.
    #[serde(default)]
    pub handoff_summary: String,
    /// The task the user was working on when the session ended, if any.
    #[serde(default)]
    pub current_task: Option<String>,
}

/// Extracts structured session state from transcripts using an LLM.
pub struct SessionStateExtractor {
    llm: Arc<LlmClient>,
}

impl SessionStateExtractor {
    pub fn new(llm: Arc<LlmClient>) -> Self {
        Self { llm }
    }

    /// Extract structured session state from the tail of a transcript.
    ///
    /// Takes the last ~8 KB (character-aligned) of the transcript and asks the
    /// LLM to produce a JSON summary. On any failure, returns an empty state
    /// rather than propagating the error.
    pub async fn extract(&self, transcript: &str) -> Result<ExtractedSessionState> {
        if transcript.trim().is_empty() {
            return Ok(ExtractedSessionState::default());
        }

        // Take last ~8 KB of transcript, aligned to a char boundary.
        let max_bytes = 32_000;
        let start = transcript.len().saturating_sub(max_bytes);
        // Find the next valid char boundary at or after `start`.
        let start = (start..transcript.len())
            .find(|&i| transcript.is_char_boundary(i))
            .unwrap_or(0);
        let tail = &transcript[start..];

        debug!(tail_len = tail.len(), "extracting session state");

        let system = r#"You are a session analyst. Analyze the provided session transcript and extract structured state.

Return ONLY valid JSON with this exact schema (no markdown fences, no explanation):
{
  "goals": ["string"],
  "accomplished": ["string"],
  "pending": ["string"],
  "handoff_summary": "string",
  "current_task": "string or null"
}

Rules:
- "goals": the high-level objectives the user was working toward
- "accomplished": concrete items that were completed
- "pending": items that remain unfinished or were mentioned but not done
- "handoff_summary": a concise 1-3 sentence summary suitable for continuing in a new session
- "current_task": what the user was actively working on at the end, or null if unclear
- Keep each list to at most 10 items
- Be concise — each item should be one short sentence"#;

        let user_prompt = format!(
            "Analyze this session transcript and extract the structured state:\n\n{}",
            tail
        );

        match self
            .llm
            .chat_json::<ExtractedSessionState>(system, &user_prompt, 0.2)
            .await
        {
            Ok(state) => {
                debug!(
                    goals = state.goals.len(),
                    accomplished = state.accomplished.len(),
                    pending = state.pending.len(),
                    has_current = state.current_task.is_some(),
                    "session state extracted"
                );
                Ok(state)
            }
            Err(e) => {
                warn!(error = %e, "failed to extract session state, returning empty state");
                Ok(ExtractedSessionState::default())
            }
        }
    }
}
