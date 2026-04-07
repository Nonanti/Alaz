//! Adaptive query classification for search strategy routing.
//!
//! Classifies incoming queries into one of five types (Semantic, Temporal,
//! Causal, Decision, Procedural) using lightweight keyword heuristics rather
//! than an LLM call, keeping latency near-zero. Each query type maps to a
//! [`SearchWeights`] struct that biases RRF fusion towards the most relevant
//! retrieval signals.
//!
//! Both English and Turkish keywords are supported.

use std::fmt;

/// Classification of a search query by intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    /// Conceptual / definitional queries ("how does X work", "what is X").
    /// Weights: knowledge high, FTS high.
    Semantic,
    /// Time-oriented queries ("what happened yesterday", "recent errors").
    /// Weights: episodes high, cue search enabled.
    Temporal,
    /// Root-cause queries ("why did X happen", "what caused X").
    /// Weights: graph expansion high, episode chains.
    Causal,
    /// Choice-oriented queries ("what did we decide about X").
    /// Weights: episodes (type=decision), core memory.
    Decision,
    /// Step-by-step queries ("how to deploy", "steps for X").
    /// Weights: procedures high, FTS high.
    Procedural,
}

impl fmt::Display for QueryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryType::Semantic => write!(f, "semantic"),
            QueryType::Temporal => write!(f, "temporal"),
            QueryType::Causal => write!(f, "causal"),
            QueryType::Decision => write!(f, "decision"),
            QueryType::Procedural => write!(f, "procedural"),
        }
    }
}

/// Per-signal weight multipliers applied during RRF fusion.
///
/// Each weight scales the RRF score contribution from the corresponding signal.
/// A weight of 1.0 is neutral. Higher values boost that signal; lower values
/// attenuate it.
#[derive(Debug, Clone)]
pub struct SearchWeights {
    /// Weight for full-text search signal.
    pub fts: f32,
    /// Weight for dense text vector search (+ ColBERT, as they share the
    /// "dense embedding" nature).
    pub dense: f32,
    /// Weight for RAPTOR hierarchical search signal.
    pub raptor: f32,
    /// Weight for graph expansion signal.
    pub graph: f32,
    /// Whether to also run 5W cue search for episodes (useful for temporal
    /// and causal queries).
    pub cue_search: bool,
}

impl QueryType {
    /// Return the default (hardcoded) signal weights for this query type.
    ///
    /// Used as fallback when no learned weights are available.
    pub fn default_weights(&self) -> SearchWeights {
        match self {
            QueryType::Semantic => SearchWeights {
                fts: 1.0,
                dense: 1.5,
                raptor: 1.0,
                graph: 0.5,
                cue_search: false,
            },
            QueryType::Temporal => SearchWeights {
                fts: 0.5,
                dense: 1.0,
                raptor: 0.3,
                graph: 0.5,
                cue_search: true,
            },
            QueryType::Causal => SearchWeights {
                fts: 0.5,
                dense: 1.0,
                raptor: 0.5,
                graph: 2.0,
                cue_search: true,
            },
            QueryType::Decision => SearchWeights {
                fts: 1.0,
                dense: 1.0,
                raptor: 0.5,
                graph: 1.0,
                cue_search: false,
            },
            QueryType::Procedural => SearchWeights {
                fts: 1.5,
                dense: 1.0,
                raptor: 0.5,
                graph: 0.3,
                cue_search: false,
            },
        }
    }

    /// Load signal weights: learned from DB if available, otherwise defaults.
    ///
    /// The weight learning job periodically updates the `signal_weights` table
    /// with weights derived from click-through data. This method checks for
    /// learned weights first and falls back to hardcoded defaults.
    pub async fn weights(&self, pool: &sqlx::PgPool) -> SearchWeights {
        use alaz_db::repos::SignalWeightRepo;

        let qt_str = self.to_string();
        match SignalWeightRepo::get(pool, &qt_str).await {
            Ok(Some(sw)) => {
                tracing::debug!(
                    query_type = %qt_str,
                    fts = sw.fts,
                    dense = sw.dense,
                    raptor = sw.raptor,
                    graph = sw.graph,
                    cue = sw.cue,
                    sample_size = sw.sample_size,
                    "using learned signal weights"
                );
                SearchWeights {
                    fts: sw.fts,
                    dense: sw.dense,
                    raptor: sw.raptor,
                    graph: sw.graph,
                    cue_search: sw.cue > 0.0,
                }
            }
            Ok(None) => {
                tracing::debug!(
                    query_type = %qt_str,
                    "no learned weights, using defaults"
                );
                self.default_weights()
            }
            Err(e) => {
                tracing::warn!(
                    query_type = %qt_str,
                    error = %e,
                    "failed to load learned weights, using defaults"
                );
                self.default_weights()
            }
        }
    }
}

/// Classify a query string into a [`QueryType`] using keyword heuristics.
///
/// The classifier checks for temporal, causal, decision, and procedural
/// indicators in order. If none match the query defaults to [`QueryType::Semantic`].
///
/// Both English and Turkish keywords are supported.
pub fn classify_query(query: &str) -> QueryType {
    let q = query.to_lowercase();

    // --- Temporal indicators ---
    // English
    if q.contains("when")
        || q.contains("yesterday")
        || q.contains("today")
        || q.contains("recent")
        || q.contains("last session")
        || q.contains("last week")
        || q.contains("last time")
        || q.contains("this morning")
        || q.contains("earlier")
        || q.contains("latest")
        || q.contains("happened")
        || q.contains("history")
        || q.contains("timeline")
        || q.contains("ago")
        // Turkish
        || q.contains("ne zaman")
        || q.contains("dün")
        || q.contains("bugün")
        || q.contains("geçen")
        || q.contains("son oturum")
        || q.contains("son zamanlarda")
        || q.contains("yakın zamanda")
        || q.contains("önce")
        || q.contains("geçmişte")
        || q.contains("en son")
        || q.contains("tarihçe")
    {
        return QueryType::Temporal;
    }

    // --- Causal indicators ---
    // English
    if q.contains("why")
        || q.contains("cause")
        || q.contains("because")
        || q.contains("led to")
        || q.contains("resulted in")
        || q.contains("root cause")
        || q.contains("reason for")
        || q.contains("what broke")
        || q.contains("what failed")
        || q.contains("debug")
        // Turkish
        || q.contains("neden")
        || q.contains("niye")
        || q.contains("sebep")
        || q.contains("yüzünden")
        || q.contains("sonuç olarak")
        || q.contains("sorun")
        || q.contains("hata neden")
        || q.contains("kırdı")
        || q.contains("bozuldu")
    {
        return QueryType::Causal;
    }

    // --- Decision indicators ---
    // English
    if q.contains("decide")
        || q.contains("decision")
        || q.contains("chose")
        || q.contains("picked")
        || q.contains("agreed on")
        || q.contains("settled on")
        || q.contains("went with")
        || q.contains("trade-off")
        || q.contains(" vs ")
        || q.contains("alternative")
        // Turkish
        || q.contains("karar")
        || q.contains("seçtik")
        || q.contains("tercih")
        || q.contains("neden seçtik")
        || q.contains("karşılaştır")
        || q.contains("hangisi")
    {
        return QueryType::Decision;
    }

    // --- Procedural indicators ---
    // English
    if q.contains("how to")
        || q.contains("steps")
        || q.contains("procedure")
        || q.contains("process for")
        || q.contains("guide to")
        || q.contains("instructions")
        || q.contains("walkthrough")
        || q.contains("setup")
        || q.contains("configure")
        || q.contains("install")
        || q.contains("deploy")
        || q.contains("migrate")
        || q.contains("upgrade")
        // Turkish
        || q.contains("nasıl")
        || q.contains("adım")
        || q.contains("prosedür")
        || q.contains("süreç")
        || q.contains("kurulum")
        || q.contains("yapılandır")
        || q.contains("yükle")
    {
        return QueryType::Procedural;
    }

    // Default to semantic
    QueryType::Semantic
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Semantic (default) ---
    #[test]
    fn test_semantic_default() {
        assert_eq!(
            classify_query("what is the auth system"),
            QueryType::Semantic
        );
    }

    #[test]
    fn test_semantic_conceptual() {
        assert_eq!(
            classify_query("how does RAPTOR clustering work"),
            QueryType::Semantic
        );
    }

    // --- Temporal ---
    #[test]
    fn test_temporal_english_when() {
        assert_eq!(
            classify_query("when did the deploy fail"),
            QueryType::Temporal
        );
    }

    #[test]
    fn test_temporal_english_yesterday() {
        assert_eq!(
            classify_query("what happened yesterday with the deploy"),
            QueryType::Temporal
        );
    }

    #[test]
    fn test_temporal_english_recent() {
        assert_eq!(classify_query("show me recent errors"), QueryType::Temporal);
    }

    #[test]
    fn test_temporal_english_last_session() {
        assert_eq!(
            classify_query("what did we do last session"),
            QueryType::Temporal
        );
    }

    #[test]
    fn test_temporal_turkish_dun() {
        assert_eq!(classify_query("dün ne yaptık"), QueryType::Temporal);
    }

    #[test]
    fn test_temporal_turkish_bugun() {
        assert_eq!(
            classify_query("bugün hangi hatalar oldu"),
            QueryType::Temporal
        );
    }

    #[test]
    fn test_temporal_turkish_ne_zaman() {
        assert_eq!(classify_query("ne zaman deploy ettik"), QueryType::Temporal);
    }

    // --- Causal ---
    #[test]
    fn test_causal_english_why() {
        assert_eq!(
            classify_query("why did the migration fail"),
            QueryType::Causal
        );
    }

    #[test]
    fn test_causal_english_cause() {
        assert_eq!(classify_query("what caused the timeout"), QueryType::Causal);
    }

    #[test]
    fn test_causal_english_led_to() {
        assert_eq!(classify_query("what led to the outage"), QueryType::Causal);
    }

    #[test]
    fn test_causal_turkish_neden() {
        assert_eq!(classify_query("bu hata neden oluştu"), QueryType::Causal);
    }

    #[test]
    fn test_causal_turkish_sebep() {
        assert_eq!(classify_query("hatanın sebep'i ne"), QueryType::Causal);
    }

    // --- Decision ---
    #[test]
    fn test_decision_english_decide() {
        assert_eq!(
            classify_query("what did we decide about the API"),
            QueryType::Decision
        );
    }

    #[test]
    fn test_decision_english_chose() {
        assert_eq!(
            classify_query("we chose Qdrant over Pinecone"),
            QueryType::Decision
        );
    }

    #[test]
    fn test_decision_turkish_karar() {
        assert_eq!(
            classify_query("veritabanı hakkında karar ne oldu"),
            QueryType::Decision
        );
    }

    // --- Procedural ---
    #[test]
    fn test_procedural_english_how_to() {
        assert_eq!(
            classify_query("how to deploy the application"),
            QueryType::Procedural
        );
    }

    #[test]
    fn test_procedural_english_steps() {
        assert_eq!(
            classify_query("what are the steps for migration"),
            QueryType::Procedural
        );
    }

    // --- New keyword tests ---
    #[test]
    fn test_procedural_deploy() {
        assert_eq!(
            classify_query("deploy to production"),
            QueryType::Procedural
        );
    }

    #[test]
    fn test_temporal_turkish_once() {
        assert_eq!(classify_query("3 gün önce ne yaptık"), QueryType::Temporal);
    }

    #[test]
    fn test_causal_turkish_sorun() {
        assert_eq!(classify_query("sorun ne oldu"), QueryType::Causal);
    }

    #[test]
    fn test_decision_turkish_hangisi() {
        assert_eq!(classify_query("hangisi daha iyi"), QueryType::Decision);
    }

    #[test]
    fn test_procedural_turkish_kurulum() {
        assert_eq!(
            classify_query("kurulum nasıl yapılır"),
            QueryType::Procedural
        );
    }

    #[test]
    fn test_procedural_turkish_nasil() {
        assert_eq!(classify_query("nasıl deploy edilir"), QueryType::Procedural);
    }

    #[test]
    fn test_procedural_turkish_adim() {
        assert_eq!(classify_query("migration adım adım"), QueryType::Procedural);
    }

    // --- Display ---
    #[test]
    fn test_display() {
        assert_eq!(QueryType::Semantic.to_string(), "semantic");
        assert_eq!(QueryType::Temporal.to_string(), "temporal");
        assert_eq!(QueryType::Causal.to_string(), "causal");
        assert_eq!(QueryType::Decision.to_string(), "decision");
        assert_eq!(QueryType::Procedural.to_string(), "procedural");
    }

    // --- Weights ---
    #[test]
    fn test_semantic_weights() {
        let w = QueryType::Semantic.default_weights();
        assert!(!w.cue_search);
        assert!(w.dense > w.fts, "semantic should favour dense over FTS");
    }

    #[test]
    fn test_temporal_weights_enable_cue_search() {
        let w = QueryType::Temporal.default_weights();
        assert!(w.cue_search, "temporal should enable cue search");
    }

    #[test]
    fn test_causal_weights_boost_graph() {
        let w = QueryType::Causal.default_weights();
        assert!(w.graph > w.fts, "causal should boost graph over FTS");
        assert!(w.cue_search, "causal should enable cue search");
    }

    #[test]
    fn test_procedural_weights_boost_fts() {
        let w = QueryType::Procedural.default_weights();
        assert!(w.fts > w.dense, "procedural should boost FTS over dense");
    }

    // --- Priority: temporal should win over causal when both present ---
    #[test]
    fn test_temporal_takes_priority_over_causal() {
        // "why" (causal) + "yesterday" (temporal) -> temporal wins (checked first)
        assert_eq!(
            classify_query("why did the deploy fail yesterday"),
            QueryType::Temporal
        );
    }
}
