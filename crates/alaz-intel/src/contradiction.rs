use std::sync::Arc;

use alaz_core::Result;
use alaz_db::repos::KnowledgeRepo;
use serde::Deserialize;
use sqlx::PgPool;
use tracing::{debug, warn};

use crate::llm::LlmClient;

/// Detects contradictions between a new piece of knowledge and existing items.
pub struct ContradictionDetector {
    pool: PgPool,
    llm: Arc<LlmClient>,
}

/// The result of checking for contradictions.
#[derive(Debug, Clone)]
pub enum ContradictionResult {
    /// The new item contradicts an existing item.
    Contradiction { old_id: String, confidence: f64 },
    /// The new item is an update/supersession of an existing item.
    Update { old_id: String, confidence: f64 },
    /// The items are complementary and can coexist.
    Complementary,
    /// The items are unrelated.
    Unrelated,
}

/// Internal LLM classification response.
#[derive(Deserialize)]
struct ClassificationResponse {
    relationship: String,
    confidence: f64,
}

const CLASSIFICATION_SYSTEM_PROMPT: &str = r#"You are a knowledge relationship classifier. Given two pieces of knowledge, determine their relationship.

Return ONLY valid JSON with this schema:
{
  "relationship": "contradiction|update|complementary|unrelated",
  "confidence": 0.0-1.0
}

Definitions:
- "contradiction": The new item directly contradicts the existing item (they cannot both be true)
- "update": The new item supersedes or updates the existing item (same topic, newer/better info)
- "complementary": The items cover the same topic but add to each other
- "unrelated": The items are about different topics"#;

impl ContradictionDetector {
    /// Create a new contradiction detector.
    pub fn new(pool: PgPool, llm: Arc<LlmClient>) -> Self {
        Self { pool, llm }
    }

    /// Check if a new piece of knowledge contradicts or updates any existing item.
    ///
    /// 1. Find similar items via title similarity (threshold 0.4)
    /// 2. For the most similar item, ask LLM to classify the relationship
    /// 3. Return the result if confidence >= 0.8 for contradiction/update
    pub async fn check(
        &self,
        new_title: &str,
        new_content: &str,
        project_id: Option<&str>,
    ) -> Result<Option<ContradictionResult>> {
        // Find similar items by title
        let similar =
            KnowledgeRepo::find_similar_by_title(&self.pool, new_title, 0.4, project_id).await?;

        if similar.is_empty() {
            debug!(title = %new_title, "no similar items found for contradiction check");
            return Ok(None);
        }

        // Check the most similar item
        let candidate = &similar[0];
        debug!(
            new_title = %new_title,
            existing_title = %candidate.title,
            existing_id = %candidate.id,
            "checking for contradiction"
        );

        let user_prompt = format!(
            "EXISTING ITEM:\nTitle: {}\nContent: {}\n\nNEW ITEM:\nTitle: {}\nContent: {}",
            candidate.title, candidate.content, new_title, new_content
        );

        let classification: ClassificationResponse = match self
            .llm
            .chat_json(CLASSIFICATION_SYSTEM_PROMPT, &user_prompt, 0.1)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "failed to classify contradiction, assuming unrelated");
                return Ok(None);
            }
        };

        debug!(
            relationship = %classification.relationship,
            confidence = classification.confidence,
            "contradiction classification result"
        );

        match classification.relationship.as_str() {
            "contradiction" if classification.confidence >= 0.8 => {
                Ok(Some(ContradictionResult::Contradiction {
                    old_id: candidate.id.clone(),
                    confidence: classification.confidence,
                }))
            }
            "update" if classification.confidence >= 0.8 => Ok(Some(ContradictionResult::Update {
                old_id: candidate.id.clone(),
                confidence: classification.confidence,
            })),
            "complementary" => Ok(Some(ContradictionResult::Complementary)),
            _ => Ok(Some(ContradictionResult::Unrelated)),
        }
    }
}
