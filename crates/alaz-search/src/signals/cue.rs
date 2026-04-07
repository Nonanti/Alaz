//! Cue search signal (6th signal).
//!
//! Searches episodes by 5W cues (who, what, where, when, why) extracted
//! from the query text. This signal is only activated for temporal and
//! causal query types where episodic memory is most relevant.

use alaz_core::Result;
use alaz_core::traits::SignalResult;
use alaz_db::repos::EpisodeRepo;
use sqlx::PgPool;
use tracing::debug;

/// Extract simple cue terms from the query for 5W matching.
///
/// This is a lightweight keyword extraction — not NLP. It splits the query
/// into meaningful tokens and uses them as `what` cues, which is the broadest
/// cue dimension. The `&&` (overlap) operator in the DB query means any
/// single match is enough to surface an episode.
fn extract_cues(query: &str) -> Vec<String> {
    let stop_words: &[&str] = &[
        "the", "a", "an", "is", "was", "were", "are", "been", "be", "have", "has", "had", "do",
        "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can", "what",
        "when", "where", "who", "why", "how", "this", "that", "these", "those", "it", "its", "in",
        "on", "at", "to", "for", "of", "with", "by", "from", "and", "or", "but", "not", "no", "if",
        "then", "so", "about", "up", "out", "just", "also", "very", "all",
        // Turkish stop words
        "bir", "bu", "şu", "ve", "ile", "için", "da", "de", "mi", "mu", "mı", "mü", "ne", "nasıl",
        "neden", "niye",
    ];

    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|w| w.len() >= 3 && !stop_words.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Execute cue search against episodes and return ranked results.
pub async fn execute(
    pool: &PgPool,
    query: &str,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<SignalResult>> {
    let cues = extract_cues(query);

    if cues.is_empty() {
        return Ok(vec![]);
    }

    // Use the extracted terms as `what` cues — the broadest dimension.
    // Episodes with matching who/what/where/when/why cues will surface.
    let episodes = EpisodeRepo::cue_search(
        pool,
        None,        // who - don't match generic keywords against person names
        Some(&cues), // what - primary dimension for keyword matching
        None,        // where
        None,        // when
        None,        // why - don't match generic keywords against reasons
        project,
        Some(limit as i64),
    )
    .await
    .unwrap_or_default();

    let results: Vec<SignalResult> = episodes
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(rank, ep)| SignalResult {
            entity_type: "episode".to_string(),
            entity_id: ep.id,
            rank,
        })
        .collect();

    debug!(
        query = %query,
        cues = ?cues,
        count = results.len(),
        "cue search signal complete"
    );

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cues_filters_stop_words() {
        let cues = extract_cues("what happened with the deploy yesterday");
        assert!(!cues.contains(&"what".to_string()));
        assert!(!cues.contains(&"the".to_string()));
        assert!(cues.contains(&"happened".to_string()));
        assert!(cues.contains(&"deploy".to_string()));
        assert!(cues.contains(&"yesterday".to_string()));
    }

    #[test]
    fn extract_cues_handles_turkish() {
        let cues = extract_cues("dün deploy neden başarısız oldu");
        assert!(!cues.contains(&"neden".to_string()));
        assert!(cues.contains(&"deploy".to_string()));
        assert!(cues.contains(&"başarısız".to_string()));
    }

    #[test]
    fn extract_cues_empty_on_all_stop_words() {
        let cues = extract_cues("what is the");
        assert!(cues.is_empty());
    }

    #[test]
    fn extract_cues_preserves_hyphenated_words() {
        let cues = extract_cues("cross-encoder reranking failed");
        assert!(cues.contains(&"cross-encoder".to_string()));
        assert!(cues.contains(&"reranking".to_string()));
    }
}
