-- Spaced repetition: track review schedules for important knowledge.
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_interval_days INT NOT NULL DEFAULT 1;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_easiness REAL NOT NULL DEFAULT 2.5;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_next_review TIMESTAMPTZ;
ALTER TABLE knowledge_items ADD COLUMN IF NOT EXISTS sr_repetitions INT NOT NULL DEFAULT 0;
