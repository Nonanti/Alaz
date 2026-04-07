//! Reciprocal Rank Fusion (RRF) for combining multiple signal results.
//!
//! RRF is a simple yet effective method for combining ranked lists from
//! multiple retrieval systems. For each result at rank r in signal s,
//! the fused score is: `score += weight * 1 / (RRF_K + r + 1)`
//!
//! The weighted variant (`weighted_reciprocal_rank_fusion`) accepts per-signal
//! weight multipliers so that the search pipeline can bias fusion towards the
//! signals most relevant to a given query type.

use std::collections::{HashMap, HashSet};

use alaz_core::traits::SignalResult;

/// RRF smoothing constant. Higher values reduce the impact of high-ranked items.
const RRF_K: f64 = 60.0;

/// Combine multiple ranked signal results into a single fused ranking using RRF.
///
/// Returns `(entity_type, entity_id, fused_score)` sorted by score descending.
pub fn reciprocal_rank_fusion(signals: Vec<Vec<SignalResult>>) -> Vec<(String, String, f64)> {
    // Delegate to the weighted variant with uniform weights of 1.0
    let weights: Vec<f32> = vec![1.0; signals.len()];
    weighted_reciprocal_rank_fusion(signals, &weights)
}

/// Combine multiple ranked signal results using *weighted* RRF.
///
/// Each signal's RRF score contribution is multiplied by the corresponding
/// weight in `weights`. If `weights` is shorter than `signals`, missing entries
/// default to 1.0.
///
/// Returns `(entity_type, entity_id, fused_score)` sorted by score descending.
pub fn weighted_reciprocal_rank_fusion(
    signals: Vec<Vec<SignalResult>>,
    weights: &[f32],
) -> Vec<(String, String, f64)> {
    let mut scores: HashMap<(String, String), f64> = HashMap::new();

    for (i, signal) in signals.iter().enumerate() {
        let weight = weights.get(i).copied().unwrap_or(1.0) as f64;
        for result in signal {
            let key = (result.entity_type.clone(), result.entity_id.clone());
            let rrf_score = weight * (1.0 / (RRF_K + result.rank as f64 + 1.0));
            *scores.entry(key).or_insert(0.0) += rrf_score;
        }
    }

    let mut fused: Vec<(String, String, f64)> = scores
        .into_iter()
        .map(|((entity_type, entity_id), score)| (entity_type, entity_id, score))
        .collect();

    // Sort by score descending
    fused.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    fused
}

/// Signal names corresponding to the 6-signal pipeline order.
pub const SIGNAL_NAMES: &[&str] = &["fts", "dense", "colbert", "graph", "raptor", "cue"];

/// Per-signal contribution to a fused result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SignalContribution {
    /// Signal name (e.g., "fts", "dense").
    pub signal: String,
    /// Weighted RRF score contribution from this signal.
    pub score: f64,
    /// Rank of the entity within this signal's results (0-based).
    pub rank: usize,
}

/// Full explanation of how a search result was scored.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FusionExplanation {
    pub entity_id: String,
    pub fused_score: f64,
    pub contributions: Vec<SignalContribution>,
}

/// Weighted RRF fusion with per-entity signal breakdowns for explainability.
///
/// Returns `(fused_results, explanations)` where explanations map each entity
/// to its per-signal score contributions.
pub fn weighted_rrf_with_explanations(
    signals: Vec<Vec<SignalResult>>,
    weights: &[f32],
) -> (Vec<(String, String, f64)>, Vec<FusionExplanation>) {
    // Track per-entity: total score + per-signal contributions
    let mut scores: HashMap<(String, String), f64> = HashMap::new();
    let mut contribs: HashMap<String, Vec<SignalContribution>> = HashMap::new(); // keyed by entity_id

    for (i, signal) in signals.iter().enumerate() {
        let weight = weights.get(i).copied().unwrap_or(1.0) as f64;
        let name = SIGNAL_NAMES.get(i).unwrap_or(&"unknown");

        for result in signal {
            let key = (result.entity_type.clone(), result.entity_id.clone());
            let rrf_score = weight * (1.0 / (RRF_K + result.rank as f64 + 1.0));
            *scores.entry(key).or_insert(0.0) += rrf_score;

            contribs
                .entry(result.entity_id.clone())
                .or_default()
                .push(SignalContribution {
                    signal: (*name).to_string(),
                    score: rrf_score,
                    rank: result.rank,
                });
        }
    }

    let mut fused: Vec<(String, String, f64)> = scores
        .into_iter()
        .map(|((entity_type, entity_id), score)| (entity_type, entity_id, score))
        .collect();
    fused.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Build sorted explanation list matching fused order
    let explanations: Vec<FusionExplanation> = fused
        .iter()
        .map(|(_etype, eid, score)| {
            let mut contributions = contribs.remove(eid).unwrap_or_default();
            // Sort contributions by score descending for readability
            contributions.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            FusionExplanation {
                entity_id: eid.clone(),
                fused_score: *score,
                contributions,
            }
        })
        .collect();

    (fused, explanations)
}

/// Build a map of `entity_id → [signal_names]` showing which signals contributed
/// to each result.
///
/// `signals` must be in the same order as [`SIGNAL_NAMES`].
pub fn build_signal_attribution(signals: &[Vec<SignalResult>]) -> HashMap<String, Vec<String>> {
    let mut attr: HashMap<String, HashSet<String>> = HashMap::new();

    for (i, signal) in signals.iter().enumerate() {
        let name = SIGNAL_NAMES.get(i).unwrap_or(&"unknown");
        for result in signal {
            attr.entry(result.entity_id.clone())
                .or_default()
                .insert((*name).to_string());
        }
    }

    attr.into_iter()
        .map(|(id, set)| (id, set.into_iter().collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signal(items: &[(&str, &str)]) -> Vec<SignalResult> {
        items
            .iter()
            .enumerate()
            .map(|(rank, (entity_type, entity_id))| SignalResult {
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                rank,
            })
            .collect()
    }

    #[test]
    fn test_single_signal() {
        let signals = vec![make_signal(&[
            ("knowledge_item", "a"),
            ("knowledge_item", "b"),
            ("knowledge_item", "c"),
        ])];

        let fused = reciprocal_rank_fusion(signals);

        assert_eq!(fused.len(), 3);
        // First item should have highest score: 1/(60+0+1) = 1/61
        assert_eq!(fused[0].1, "a");
        assert_eq!(fused[1].1, "b");
        assert_eq!(fused[2].1, "c");

        let expected_first = 1.0 / 61.0;
        assert!((fused[0].2 - expected_first).abs() < 1e-10);
    }

    #[test]
    fn test_multiple_signals_boost() {
        // Item "a" appears in both signals, so it should get boosted
        let signal1 = make_signal(&[("knowledge_item", "a"), ("knowledge_item", "b")]);
        let signal2 = make_signal(&[("knowledge_item", "a"), ("knowledge_item", "c")]);

        let fused = reciprocal_rank_fusion(vec![signal1, signal2]);

        assert_eq!(fused.len(), 3);
        // "a" should be first because it appears in both signals
        assert_eq!(fused[0].1, "a");
        // Its score should be 2/(60+0+1) = 2/61
        let expected_a = 2.0 / 61.0;
        assert!((fused[0].2 - expected_a).abs() < 1e-10);
    }

    #[test]
    fn test_empty_signals() {
        let fused = reciprocal_rank_fusion(vec![]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_mixed_entity_types() {
        let signal1 = make_signal(&[("knowledge_item", "a"), ("episode", "b")]);
        let signal2 = make_signal(&[("episode", "b"), ("procedure", "c")]);

        let fused = reciprocal_rank_fusion(vec![signal1, signal2]);

        assert_eq!(fused.len(), 3);
        // "episode:b" appears in both at different ranks
        // Signal 1: rank 1 -> 1/(60+1+1) = 1/62
        // Signal 2: rank 0 -> 1/(60+0+1) = 1/61
        // Total: 1/61 + 1/62
        let b_entry = fused
            .iter()
            .find(|e| e.0 == "episode" && e.1 == "b")
            .unwrap();
        let expected_b = 1.0 / 61.0 + 1.0 / 62.0;
        assert!((b_entry.2 - expected_b).abs() < 1e-10);
    }

    #[test]
    fn test_rank_ordering() {
        // Verify that lower rank (higher position) gets higher RRF score
        let signal = make_signal(&[
            ("knowledge_item", "top"),
            ("knowledge_item", "mid"),
            ("knowledge_item", "bot"),
        ]);

        let fused = reciprocal_rank_fusion(vec![signal]);

        assert_eq!(fused[0].1, "top");
        assert_eq!(fused[1].1, "mid");
        assert_eq!(fused[2].1, "bot");
        assert!(fused[0].2 > fused[1].2);
        assert!(fused[1].2 > fused[2].2);
    }

    #[test]
    fn test_many_signals_same_item() {
        // An item appearing in 6 signals at rank 0 should have score 6/61
        let signals: Vec<Vec<SignalResult>> = (0..6)
            .map(|_| make_signal(&[("knowledge_item", "popular")]))
            .collect();

        let fused = reciprocal_rank_fusion(signals);

        assert_eq!(fused.len(), 1);
        let expected = 6.0 / 61.0;
        assert!((fused[0].2 - expected).abs() < 1e-10);
    }

    // --- Weighted RRF tests ---

    #[test]
    fn test_weighted_rrf_uniform_matches_unweighted() {
        let signal1 = make_signal(&[("knowledge_item", "a"), ("knowledge_item", "b")]);
        let signal2 = make_signal(&[("knowledge_item", "a"), ("knowledge_item", "c")]);

        let unweighted = reciprocal_rank_fusion(vec![signal1.clone(), signal2.clone()]);
        let weighted = weighted_reciprocal_rank_fusion(vec![signal1, signal2], &[1.0, 1.0]);

        assert_eq!(unweighted.len(), weighted.len());
        // Compare by collecting into sorted maps (ordering of equal-score items is non-deterministic)
        for u in &unweighted {
            let w = weighted
                .iter()
                .find(|w| w.0 == u.0 && w.1 == u.1)
                .expect("missing entry");
            assert!((u.2 - w.2).abs() < 1e-10);
        }
    }

    #[test]
    fn test_weighted_rrf_higher_weight_boosts_signal() {
        // Two signals each containing a unique item at rank 0.
        // Signal 1 (weight 2.0) has "a", signal 2 (weight 0.5) has "b".
        let signal1 = make_signal(&[("knowledge_item", "a")]);
        let signal2 = make_signal(&[("knowledge_item", "b")]);

        let fused = weighted_reciprocal_rank_fusion(vec![signal1, signal2], &[2.0, 0.5]);

        assert_eq!(fused.len(), 2);
        // "a" should rank first due to higher weight
        assert_eq!(fused[0].1, "a");
        assert_eq!(fused[1].1, "b");

        let expected_a = 2.0 / 61.0;
        let expected_b = 0.5 / 61.0;
        assert!((fused[0].2 - expected_a).abs() < 1e-10);
        assert!((fused[1].2 - expected_b).abs() < 1e-10);
    }

    #[test]
    fn test_weighted_rrf_zero_weight_neutralises_signal() {
        let signal1 = make_signal(&[("knowledge_item", "a")]);
        let signal2 = make_signal(&[("knowledge_item", "b")]);

        let fused = weighted_reciprocal_rank_fusion(vec![signal1, signal2], &[1.0, 0.0]);

        assert_eq!(fused.len(), 2);
        // "b" should have score 0 due to zero weight
        let b_entry = fused.iter().find(|e| e.1 == "b").unwrap();
        assert!(b_entry.2.abs() < 1e-10);
    }

    #[test]
    fn test_weighted_rrf_missing_weights_default_to_one() {
        // Three signals but only two weights provided — third defaults to 1.0
        let signal1 = make_signal(&[("knowledge_item", "a")]);
        let signal2 = make_signal(&[("knowledge_item", "b")]);
        let signal3 = make_signal(&[("knowledge_item", "c")]);

        let fused = weighted_reciprocal_rank_fusion(
            vec![signal1, signal2, signal3],
            &[1.0, 1.0], // only 2 weights for 3 signals
        );

        // "c" should have the default weight of 1.0
        let c_entry = fused.iter().find(|e| e.1 == "c").unwrap();
        let expected_c = 1.0 / 61.0;
        assert!((c_entry.2 - expected_c).abs() < 1e-10);
    }

    #[test]
    fn test_many_signals_large_count() {
        // 12 signals, each with the same item at different ranks
        let signals: Vec<Vec<SignalResult>> = (0..12)
            .map(|i| {
                vec![SignalResult {
                    entity_type: "knowledge_item".to_string(),
                    entity_id: "popular".to_string(),
                    rank: i % 3, // ranks 0, 1, 2 cycling
                }]
            })
            .collect();

        let fused = reciprocal_rank_fusion(signals);
        assert_eq!(fused.len(), 1);
        // Score = 4*(1/61) + 4*(1/62) + 4*(1/63)
        let expected = 4.0 / 61.0 + 4.0 / 62.0 + 4.0 / 63.0;
        assert!(
            (fused[0].2 - expected).abs() < 1e-10,
            "Expected {expected}, got {}",
            fused[0].2
        );
    }

    #[test]
    fn test_all_items_same_rank() {
        // Multiple items all at rank 0 in a single signal
        let signal = vec![
            SignalResult {
                entity_type: "knowledge_item".to_string(),
                entity_id: "a".to_string(),
                rank: 0,
            },
            SignalResult {
                entity_type: "knowledge_item".to_string(),
                entity_id: "b".to_string(),
                rank: 0,
            },
            SignalResult {
                entity_type: "knowledge_item".to_string(),
                entity_id: "c".to_string(),
                rank: 0,
            },
        ];

        let fused = reciprocal_rank_fusion(vec![signal]);
        assert_eq!(fused.len(), 3);
        // All have the same rank, so all should have the same score
        let first_score = fused[0].2;
        for entry in &fused {
            assert!(
                (entry.2 - first_score).abs() < 1e-10,
                "All items at same rank should have same score"
            );
        }
        // Score should be 1/(60+0+1) = 1/61
        assert!((first_score - 1.0 / 61.0).abs() < 1e-10);
    }
}
