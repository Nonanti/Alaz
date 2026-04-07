-- 004_memory_decay.sql: Add utility scoring columns for memory decay and pruning.
-- Idempotent: uses IF NOT EXISTS / ADD COLUMN IF NOT EXISTS throughout.

-- Knowledge items: add utility_score (access_count and last_accessed_at already exist from 001_initial.sql)
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS utility_score DOUBLE PRECISION NOT NULL DEFAULT 1.0;

-- Episodes: add utility_score, last_accessed_at, access_count
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS utility_score DOUBLE PRECISION NOT NULL DEFAULT 1.0;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS last_accessed_at TIMESTAMPTZ;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS access_count BIGINT NOT NULL DEFAULT 0;

-- Procedures: add utility_score, last_accessed_at, access_count
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS utility_score DOUBLE PRECISION NOT NULL DEFAULT 1.0;
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS last_accessed_at TIMESTAMPTZ;
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS access_count BIGINT NOT NULL DEFAULT 0;

-- Indexes for efficient decay queries (find items needing decay or pruning)
CREATE INDEX IF NOT EXISTS idx_knowledge_items_utility_score
    ON knowledge_items (utility_score) WHERE utility_score < 1.0;
CREATE INDEX IF NOT EXISTS idx_episodes_utility_score
    ON episodes (utility_score) WHERE utility_score < 1.0;
CREATE INDEX IF NOT EXISTS idx_procedures_utility_score
    ON procedures (utility_score) WHERE utility_score < 1.0;
