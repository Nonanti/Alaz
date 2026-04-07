-- Migration 006: Enhanced Reflection + Supersession Chains
-- Adds granular reflection scoring, action items, FTS search,
-- and supersession columns to episodes and procedures.

-- ============================================================
-- Reflection enhancements
-- ============================================================

ALTER TABLE reflections ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'session_end';
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS action_items JSONB NOT NULL DEFAULT '[]';
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS overall_score DOUBLE PRECISION;
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS knowledge_score DOUBLE PRECISION;
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS decision_score DOUBLE PRECISION;
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS efficiency_score DOUBLE PRECISION;
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS evaluated_episode_ids TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS needs_embedding BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- FTS search vector for reflections (lessons search)
ALTER TABLE reflections ADD COLUMN IF NOT EXISTS search_vector TSVECTOR
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', COALESCE(what_worked, '')), 'A') ||
        setweight(to_tsvector('english', COALESCE(what_failed, '')), 'A') ||
        setweight(to_tsvector('english', COALESCE(lessons_learned, '')), 'B')
    ) STORED;

-- Indexes for reflection queries
CREATE INDEX IF NOT EXISTS idx_reflections_search_vector ON reflections USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_reflections_needs_embedding ON reflections (id) WHERE needs_embedding = TRUE;
CREATE INDEX IF NOT EXISTS idx_reflections_kind ON reflections (kind);
CREATE INDEX IF NOT EXISTS idx_reflections_project_id ON reflections (project_id);

-- ============================================================
-- Supersession: episodes
-- ============================================================

ALTER TABLE episodes ADD COLUMN IF NOT EXISTS superseded_by TEXT;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS valid_from TIMESTAMPTZ;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS valid_until TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_episodes_superseded ON episodes (superseded_by) WHERE superseded_by IS NOT NULL;

-- ============================================================
-- Supersession: procedures
-- ============================================================

ALTER TABLE procedures ADD COLUMN IF NOT EXISTS superseded_by TEXT;
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS valid_from TIMESTAMPTZ;
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS valid_until TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_procedures_superseded ON procedures (superseded_by) WHERE superseded_by IS NOT NULL;

-- ============================================================
-- Supersession: knowledge_items (invalidation_reason)
-- ============================================================

ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS invalidation_reason TEXT;
