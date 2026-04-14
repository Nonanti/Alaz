//! Agentic search: iterative multi-hop query refinement using LLM reasoning.
//!
//! For complex questions like "What patterns did we use when fixing the auth
//! issue last week?", a single search pass isn't enough. Agentic search runs
//! 2-3 rounds, letting the LLM analyze intermediate results and refine the
//! query each time.
//!
//! Flow:
//! 1. Run initial hybrid search
//! 2. LLM analyzes results and decides: "enough info" or "need to refine"
//! 3. If refining, LLM generates a new focused query based on what was found
//! 4. Repeat up to [`MAX_HOPS`] times
//! 5. Return deduplicated union of all results, sorted by score

use std::collections::HashSet;

use alaz_core::Result;
use alaz_core::traits::{SearchQuery, SearchResult};
use alaz_intel::LlmClient;
use tracing::{debug, warn};

use crate::SearchPipeline;

/// Maximum number of search iterations for agentic search.
const MAX_HOPS: usize = 3;

/// Agentic search performs iterative query refinement using LLM reasoning.
///
/// Each hop runs a hybrid search, then asks the LLM whether the accumulated
/// results sufficiently answer the original question.  If more context is
/// needed, the LLM produces a refined query for the next hop.
///
/// Results across all hops are deduplicated by `entity_id`, sorted by score
/// descending, and truncated to `limit`.
pub async fn agentic_search(
    pipeline: &SearchPipeline,
    llm: &LlmClient,
    query: &str,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let mut all_results: Vec<SearchResult> = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut current_query = query.to_string();

    for hop in 0..MAX_HOPS {
        // Run hybrid search with current query
        let search_query = SearchQuery {
            query: current_query.clone(),
            project: project.map(String::from),
            limit: Some(limit),
            rerank: Some(true),
            hyde: Some(false),
            graph_expand: Some(true),
        };

        let results = pipeline.hybrid_search(&search_query).await?;

        // Add new results (deduplicate by entity_id)
        for r in results {
            if seen_ids.insert(r.entity_id.clone()) {
                all_results.push(r);
            }
        }

        // On last hop, don't ask LLM for refinement
        if hop == MAX_HOPS - 1 {
            break;
        }

        // Ask LLM if we need to refine
        let context = all_results
            .iter()
            .take(5)
            .map(|r| {
                let snippet = &r.content[..r.content.len().min(200)];
                format!("- {} ({}): {}", r.title, r.entity_type, snippet)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let refinement_prompt = format!(
            "Original question: {}\n\nResults so far:\n{}\n\n\
            If these results fully answer the question, respond with exactly: DONE\n\
            If more information is needed, respond with a refined search query \
            (just the query text, nothing else).",
            query, context
        );

        match llm
            .chat(
                "You are a search query refinement assistant. Be concise.",
                &refinement_prompt,
                0.1,
            )
            .await
        {
            Ok(response) => {
                let response = response.trim();
                if response == "DONE" || response.is_empty() {
                    debug!(hop, "agentic search: LLM says DONE");
                    break;
                }
                debug!(hop, refined_query = %response, "agentic search: refining query");
                current_query = response.to_string();
            }
            Err(e) => {
                warn!(error = %e, "agentic search: LLM refinement failed, stopping");
                break;
            }
        }
    }

    // Sort by score descending, take limit
    all_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_results.truncate(limit);

    Ok(all_results)
}
