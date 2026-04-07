-- 007: Source tracking for multi-input support (JARVIS generalization)
-- Adds source and source_metadata columns to knowledge_items, episodes, and procedures
-- to track where content originated (claude_code, mobile_note, web_clip, voice_memo, etc.)

ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS source VARCHAR(50) DEFAULT 'claude_code';
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS source_metadata JSONB DEFAULT '{}';

ALTER TABLE episodes ADD COLUMN IF NOT EXISTS source VARCHAR(50) DEFAULT 'claude_code';
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS source_metadata JSONB DEFAULT '{}';

ALTER TABLE procedures ADD COLUMN IF NOT EXISTS source VARCHAR(50) DEFAULT 'claude_code';
ALTER TABLE procedures ADD COLUMN IF NOT EXISTS source_metadata JSONB DEFAULT '{}';

-- Index for filtering by source
CREATE INDEX IF NOT EXISTS idx_knowledge_source ON knowledge_items(source);
CREATE INDEX IF NOT EXISTS idx_episodes_source ON episodes(source);
CREATE INDEX IF NOT EXISTS idx_procedures_source ON procedures(source);
