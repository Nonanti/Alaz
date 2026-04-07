-- Search feedback loop: track queries and boost items based on click-through
CREATE TABLE IF NOT EXISTS search_queries (
    id TEXT PRIMARY KEY,
    query TEXT NOT NULL,
    project_id TEXT,
    result_ids TEXT[] NOT NULL DEFAULT '{}',
    clicked_ids TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_search_queries_created_at ON search_queries(created_at DESC);

ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS feedback_boost REAL NOT NULL DEFAULT 0.0;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS feedback_boost REAL NOT NULL DEFAULT 0.0;
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS feedback_boost REAL NOT NULL DEFAULT 0.0;
