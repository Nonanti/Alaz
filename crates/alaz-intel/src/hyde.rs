use std::sync::Arc;

use alaz_core::Result;
use tracing::debug;

use crate::llm::LlmClient;

/// Hypothetical Document Embeddings (HyDE) generator.
///
/// Given a user query, generates a hypothetical ideal document that would
/// answer the query. This document is then embedded and used for vector
/// search, which often produces better results than embedding the raw query.
pub struct HydeGenerator {
    llm: Arc<LlmClient>,
}

const HYDE_SYSTEM_PROMPT: &str = r#"You are a technical documentation writer. Given a search query, write a short, detailed document that would be the ideal answer to this query in a software development knowledge base.

Write as if you are creating the knowledge base entry that perfectly answers the query. Be specific and technical. Keep it under 500 words.

Return ONLY the document text, no preamble or explanation."#;

impl HydeGenerator {
    /// Create a new HyDE generator.
    pub fn new(llm: Arc<LlmClient>) -> Self {
        Self { llm }
    }

    /// Generate a hypothetical document for the given query.
    pub async fn generate(&self, query: &str) -> Result<String> {
        debug!(query = %query, "generating HyDE document");

        let document = self.llm.chat(HYDE_SYSTEM_PROMPT, query, 0.5).await?;

        debug!(
            query = %query,
            doc_len = document.len(),
            "generated HyDE document"
        );

        Ok(document)
    }
}
