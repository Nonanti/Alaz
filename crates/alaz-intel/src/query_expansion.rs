use std::sync::Arc;

use alaz_core::Result;
use tracing::{debug, warn};

use crate::llm::LlmClient;

/// Generates alternative query formulations for RAG fusion.
///
/// Given a search query, asks the LLM to produce 3 diverse rephrased versions.
/// The original query is always included as the first element. On any failure,
/// gracefully falls back to returning only the original query.
pub struct QueryExpander {
    llm: Arc<LlmClient>,
}

const QUERY_EXPANSION_PROMPT: &str = r#"Rephrase this search query 3 different ways for better search coverage. Use diverse strategies:
1. Synonym substitution (replace key terms with synonyms)
2. Broader or narrower scope (generalize or specialize the query)
3. Different perspective (rephrase from a different angle)

Return ONLY a JSON array of 3 strings (the rephrased queries, NOT the original):
["rephrased query 1", "rephrased query 2", "rephrased query 3"]

Rules:
- Each rephrasing must be meaningfully different
- Keep the core intent of the original query
- Do not add information not implied by the original query
- Keep each rephrasing concise (similar length to the original)"#;

impl QueryExpander {
    pub fn new(llm: Arc<LlmClient>) -> Self {
        Self { llm }
    }

    /// Generate alternative query formulations for RAG fusion.
    ///
    /// Returns a vector where the first element is always the original query,
    /// followed by up to 3 LLM-generated reformulations. On any error,
    /// gracefully falls back to just `vec![original]`.
    pub async fn expand(&self, query: &str) -> Result<Vec<String>> {
        let original = query.to_string();

        let rephrased: Vec<String> = match self
            .llm
            .chat_json::<Vec<String>>(QUERY_EXPANSION_PROMPT, query, 0.5)
            .await
        {
            Ok(mut results) => {
                results.truncate(3);
                results
            }
            Err(e) => {
                warn!(error = %e, "query expansion LLM call failed, using original only");
                return Ok(vec![original]);
            }
        };

        let mut expanded = Vec::with_capacity(1 + rephrased.len());
        expanded.push(original);
        for r in rephrased {
            if !r.trim().is_empty() {
                expanded.push(r);
            }
        }

        debug!(
            query = %query,
            expansions = expanded.len() - 1,
            "query expansion complete"
        );

        Ok(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_expander_prompt_is_not_empty() {
        assert!(!QUERY_EXPANSION_PROMPT.is_empty());
    }
}
