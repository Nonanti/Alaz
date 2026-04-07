use std::sync::Arc;

use alaz_core::Result;
use serde::Deserialize;
use tracing::debug;

use crate::llm::LlmClient;

/// Generates session reflections by analyzing a session summary with LLM.
pub struct ReflectionGenerator {
    llm: Arc<LlmClient>,
}

/// Structured reflection data produced by the LLM.
#[derive(Debug, Clone, Deserialize)]
pub struct ReflectionData {
    pub what_worked: String,
    pub what_failed: String,
    pub lessons_learned: String,
    pub effectiveness_score: f64,
    pub complexity_score: f64,
    pub overall_score: f64,
    pub knowledge_score: f64,
    pub decision_score: f64,
    pub efficiency_score: f64,
    #[serde(default)]
    pub action_items: Vec<ReflectionActionItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReflectionActionItem {
    pub description: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_priority")]
    pub priority: String,
}

fn default_status() -> String {
    "pending".to_string()
}
fn default_priority() -> String {
    "medium".to_string()
}

const REFLECTION_SYSTEM_PROMPT: &str = r#"You are a software development session analyst. Analyze the given session summary and provide a structured reflection.

Return ONLY valid JSON with this schema:
{
  "what_worked": "Description of what went well in this session",
  "what_failed": "Description of what didn't work or caused problems",
  "lessons_learned": "Key takeaways and lessons for future sessions",
  "effectiveness_score": 0.0-1.0,
  "complexity_score": 0.0-1.0,
  "overall_score": 0.0-1.0,
  "knowledge_score": 0.0-1.0,
  "decision_score": 0.0-1.0,
  "efficiency_score": 0.0-1.0,
  "action_items": [{"description": "...", "status": "pending", "priority": "high|medium|low"}]
}

Scoring guidelines (all 0.0-1.0):
- effectiveness_score: How well were goals accomplished?
- complexity_score: How complex was the task?
- overall_score: Holistic session quality assessment
- knowledge_score: Quality of knowledge captured and reused
- decision_score: Quality of architectural and design decisions made
- efficiency_score: How efficiently was time and effort used?

Action items: List 0-3 concrete follow-up actions from this session.
Be honest, specific, and constructive in your analysis."#;

impl ReflectionGenerator {
    /// Create a new reflection generator.
    pub fn new(llm: Arc<LlmClient>) -> Self {
        Self { llm }
    }

    /// Generate a reflection from a session summary.
    pub async fn generate(&self, session_summary: &str) -> Result<ReflectionData> {
        debug!(
            summary_len = session_summary.len(),
            "generating session reflection"
        );

        let reflection: ReflectionData = self
            .llm
            .chat_json(REFLECTION_SYSTEM_PROMPT, session_summary, 0.3)
            .await?;

        debug!(
            effectiveness = reflection.effectiveness_score,
            complexity = reflection.complexity_score,
            overall = reflection.overall_score,
            action_items = reflection.action_items.len(),
            "generated reflection"
        );

        Ok(reflection)
    }
}
