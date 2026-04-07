-- Search explainability: store per-result signal breakdowns.
ALTER TABLE search_queries ADD COLUMN IF NOT EXISTS explanations JSONB DEFAULT '{}';
-- Format: {"entity_id": {"fused_score": 0.05, "contributions": [{"signal": "fts", "score": 0.016, "rank": 0}, ...]}}
