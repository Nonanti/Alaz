-- Learning run analytics: track extraction quality per session
CREATE TABLE IF NOT EXISTS learning_runs (
    id TEXT PRIMARY KEY,
    session_id TEXT,
    project_id TEXT,
    transcript_size_bytes BIGINT DEFAULT 0,
    chunks_processed INT DEFAULT 0,
    patterns_extracted INT DEFAULT 0,
    episodes_extracted INT DEFAULT 0,
    procedures_extracted INT DEFAULT 0,
    memories_extracted INT DEFAULT 0,
    duplicates_skipped INT DEFAULT 0,
    contradictions_resolved INT DEFAULT 0,
    duration_ms BIGINT DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_learning_runs_created ON learning_runs(created_at);
CREATE INDEX IF NOT EXISTS idx_learning_runs_project ON learning_runs(project_id);
