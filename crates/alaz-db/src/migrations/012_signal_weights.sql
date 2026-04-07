-- Adaptive signal weights: track per-query-type signal performance
-- and learn optimal weights from click-through data.

-- Learned signal weights per query type.
-- Updated weekly by the weight learning job.
CREATE TABLE IF NOT EXISTS signal_weights (
    id TEXT PRIMARY KEY,
    query_type TEXT NOT NULL,
    fts REAL NOT NULL DEFAULT 1.0,
    dense REAL NOT NULL DEFAULT 1.0,
    raptor REAL NOT NULL DEFAULT 1.0,
    graph REAL NOT NULL DEFAULT 1.0,
    cue REAL NOT NULL DEFAULT 0.0,
    sample_size INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_signal_weights_query_type
    ON signal_weights(query_type);

-- Extend search_queries with classification and signal attribution.
ALTER TABLE search_queries ADD COLUMN IF NOT EXISTS query_type TEXT;
ALTER TABLE search_queries ADD COLUMN IF NOT EXISTS signal_sources JSONB DEFAULT '{}';
-- signal_sources format: {"entity_id": ["fts", "dense"], ...}
