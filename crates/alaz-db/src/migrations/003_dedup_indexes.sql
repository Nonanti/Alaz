-- Trigram indexes for deduplication during learning pipeline extraction.
-- Enables similarity() function on title fields for fuzzy matching.

CREATE INDEX IF NOT EXISTS idx_episodes_title_trgm
    ON episodes USING GIN (title gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_procedures_title_trgm
    ON procedures USING GIN (title gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_core_memories_key_trgm
    ON core_memories USING GIN (key gin_trgm_ops);

-- Add needs_embedding column to core_memories for vector dedup support.
ALTER TABLE core_memories ADD COLUMN IF NOT EXISTS needs_embedding BOOLEAN NOT NULL DEFAULT TRUE;

-- Fix raptor_trees: allow NULL project_id for global RAPTOR tree.
ALTER TABLE raptor_trees ALTER COLUMN project_id DROP NOT NULL;

-- Partial unique index for NULL project_id (needed for ON CONFLICT WHERE project_id IS NULL)
CREATE UNIQUE INDEX IF NOT EXISTS idx_raptor_trees_null_project
    ON raptor_trees (id) WHERE project_id IS NULL;
