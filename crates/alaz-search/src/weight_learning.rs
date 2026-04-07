//! Signal weight learning from click-through data.
//!
//! Analyzes `search_queries` to determine which retrieval signals produce
//! the most-clicked results, then updates `signal_weights` with learned
//! values per query type.
//!
//! Uses exponential moving average (EMA) to smooth transitions between
//! old and new weights, preventing sudden shifts from noisy data.

use std::collections::HashMap;

use alaz_core::Result;
use alaz_db::repos::SignalWeightRepo;
use sqlx::PgPool;
use tracing::{debug, info};

use crate::classifier::QueryType;
use crate::fusion::SIGNAL_NAMES;

/// EMA smoothing factor. Higher values weight new data more heavily.
///
/// At α=0.3, it takes ~7 learning cycles for old weights to decay to <10%.
const EMA_ALPHA: f32 = 0.3;

/// Minimum number of clicked queries required before learning begins.
///
/// Prevents learning from noise when data is sparse.
const MIN_SAMPLES: i64 = 10;

/// Rows returned by the signal attribution query.
#[derive(Debug, sqlx::FromRow)]
struct SignalClickRow {
    query_type: String,
    signal_sources: serde_json::Value,
    clicked_ids: Vec<String>,
}

/// Aggregated click statistics per query type: signal name → (click_count, show_count).
struct ClickStats {
    /// Per query_type: signal → (clicks, shows).
    stats: HashMap<String, HashMap<String, (i64, i64)>>,
    /// Number of rows (queries) per query_type.
    sample_counts: HashMap<String, i64>,
}

/// Learn optimal signal weights from the last 7 days of search click data.
///
/// For each query type:
/// 1. Count how many times each signal contributed to a **clicked** result
/// 2. Normalize to get per-signal CTR
/// 3. Smooth with EMA against current weights
/// 4. Store in `signal_weights` table
pub async fn learn_weights(pool: &PgPool) -> Result<u32> {
    let click_stats = match fetch_click_data(pool).await? {
        Some(s) => s,
        None => return Ok(0),
    };

    let mut updated = 0u32;

    for (query_type, signal_stats) in &click_stats.stats {
        let sample_count = click_stats
            .sample_counts
            .get(query_type)
            .copied()
            .unwrap_or(0);

        if sample_count < MIN_SAMPLES {
            debug!(
                query_type,
                samples = sample_count,
                min = MIN_SAMPLES,
                "weight learning: insufficient samples, skipping"
            );
            continue;
        }

        let learned = match compute_signal_ctrs(signal_stats) {
            Some(w) => w,
            None => {
                debug!(query_type, "weight learning: all CTRs zero, skipping");
                continue;
            }
        };

        apply_ema_and_store(pool, query_type, sample_count, &learned).await?;
        updated += 1;
    }

    Ok(updated)
}

/// Fetch queries with both clicks and signal attribution from the last 7 days.
///
/// Returns `None` if no click data is available.
async fn fetch_click_data(pool: &PgPool) -> Result<Option<ClickStats>> {
    let rows = sqlx::query_as::<_, SignalClickRow>(
        r#"
        SELECT query_type, signal_sources, clicked_ids
        FROM search_queries
        WHERE query_type IS NOT NULL
          AND signal_sources IS NOT NULL
          AND signal_sources != '{}'::jsonb
          AND array_length(clicked_ids, 1) > 0
          AND created_at > now() - interval '7 days'
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        info!("weight learning: no click data available, skipping");
        return Ok(None);
    }

    // Aggregate per query_type: signal → (click_count, show_count)
    let mut stats: HashMap<String, HashMap<String, (i64, i64)>> = HashMap::new();
    let mut sample_counts: HashMap<String, i64> = HashMap::new();

    for row in &rows {
        let qt_stats = stats.entry(row.query_type.clone()).or_default();
        *sample_counts.entry(row.query_type.clone()).or_default() += 1;

        // Parse signal_sources: {"entity_id": ["fts", "dense"], ...}
        let sources = row.signal_sources.as_object().cloned().unwrap_or_default();

        // Count shows for every signal that contributed to any result
        for (_entity_id, signal_list) in &sources {
            if let Some(arr) = signal_list.as_array() {
                for signal in arr {
                    if let Some(name) = signal.as_str() {
                        qt_stats.entry(name.to_string()).or_insert((0, 0)).1 += 1;
                    }
                }
            }
        }

        // Count clicks: credit signals that contributed to clicked results
        for clicked_id in &row.clicked_ids {
            if let Some(arr) = sources.get(clicked_id).and_then(|v| v.as_array()) {
                for signal in arr {
                    if let Some(name) = signal.as_str() {
                        qt_stats.entry(name.to_string()).or_insert((0, 0)).0 += 1;
                    }
                }
            }
        }
    }

    Ok(Some(ClickStats {
        stats,
        sample_counts,
    }))
}

/// Compute normalized per-signal CTR weights from raw click/show counts.
///
/// Returns `None` if all CTRs are zero (no signal produced any clicks).
/// Normalizes so max CTR maps to 2.0 and minimum maps to 0.3.
fn compute_signal_ctrs(signal_stats: &HashMap<String, (i64, i64)>) -> Option<[f32; 5]> {
    let mut signal_ctrs: HashMap<&str, f32> = HashMap::new();
    for name in SIGNAL_NAMES {
        let (clicks, shows) = signal_stats.get(*name).copied().unwrap_or((0, 0));
        let ctr = if shows > 0 {
            clicks as f32 / shows as f32
        } else {
            0.0
        };
        signal_ctrs.insert(name, ctr);
    }

    let max_ctr = signal_ctrs.values().copied().fold(0.0_f32, f32::max);
    if max_ctr < f32::EPSILON {
        return None;
    }

    let normalize = |ctr: f32| -> f32 {
        let normalized = ctr / max_ctr; // 0.0 to 1.0
        0.3 + normalized * 1.7 // 0.3 to 2.0
    };

    Some([
        normalize(signal_ctrs["fts"]),
        normalize(signal_ctrs["dense"]),
        normalize(signal_ctrs["raptor"]),
        normalize(signal_ctrs["graph"]),
        normalize(signal_ctrs["cue"]),
    ])
}

/// Apply EMA smoothing against current/default weights and store the result.
async fn apply_ema_and_store(
    pool: &PgPool,
    query_type: &str,
    sample_count: i64,
    learned: &[f32; 5],
) -> Result<()> {
    let qt = parse_query_type(query_type);
    let current = match SignalWeightRepo::get(pool, query_type).await? {
        Some(sw) => [sw.fts, sw.dense, sw.raptor, sw.graph, sw.cue],
        None => {
            let defaults = qt.default_weights();
            [
                defaults.fts,
                defaults.dense,
                defaults.raptor,
                defaults.graph,
                if defaults.cue_search { 1.5 } else { 0.0 },
            ]
        }
    };

    let smoothed: [f32; 5] = std::array::from_fn(|i| ema(learned[i], current[i]));

    info!(
        query_type,
        samples = sample_count,
        fts = format!("{:.2} → {:.2}", current[0], smoothed[0]),
        dense = format!("{:.2} → {:.2}", current[1], smoothed[1]),
        raptor = format!("{:.2} → {:.2}", current[2], smoothed[2]),
        graph = format!("{:.2} → {:.2}", current[3], smoothed[3]),
        cue = format!("{:.2} → {:.2}", current[4], smoothed[4]),
        "weight learning: updating signal weights"
    );

    SignalWeightRepo::upsert(
        pool,
        &alaz_db::repos::signal_weight::UpsertSignalWeight {
            query_type: query_type.to_string(),
            fts: smoothed[0],
            dense: smoothed[1],
            raptor: smoothed[2],
            graph: smoothed[3],
            cue: smoothed[4],
            sample_size: sample_count as i32,
        },
    )
    .await?;

    Ok(())
}

/// Exponential moving average: `α * new + (1-α) * old`.
fn ema(new: f32, old: f32) -> f32 {
    EMA_ALPHA * new + (1.0 - EMA_ALPHA) * old
}

/// Parse a query type string back into the enum.
fn parse_query_type(s: &str) -> QueryType {
    match s {
        "temporal" => QueryType::Temporal,
        "causal" => QueryType::Causal,
        "decision" => QueryType::Decision,
        "procedural" => QueryType::Procedural,
        _ => QueryType::Semantic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_with_alpha_0_3() {
        // new=2.0, old=1.0 → 0.3*2.0 + 0.7*1.0 = 1.3
        let result = ema(2.0, 1.0);
        assert!((result - 1.3).abs() < 1e-6);
    }

    #[test]
    fn ema_same_value_is_identity() {
        let result = ema(1.5, 1.5);
        assert!((result - 1.5).abs() < 1e-6);
    }

    #[test]
    fn ema_from_zero() {
        // Starting from 0, learning 2.0 → 0.3*2.0 + 0.7*0.0 = 0.6
        let result = ema(2.0, 0.0);
        assert!((result - 0.6).abs() < 1e-6);
    }

    #[test]
    fn parse_query_types() {
        assert_eq!(parse_query_type("semantic"), QueryType::Semantic);
        assert_eq!(parse_query_type("temporal"), QueryType::Temporal);
        assert_eq!(parse_query_type("causal"), QueryType::Causal);
        assert_eq!(parse_query_type("decision"), QueryType::Decision);
        assert_eq!(parse_query_type("procedural"), QueryType::Procedural);
        assert_eq!(parse_query_type("unknown"), QueryType::Semantic);
    }
}
