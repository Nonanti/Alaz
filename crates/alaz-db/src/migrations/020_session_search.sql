-- Add searchable text content and FTS vector to session_logs
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS transcript_text TEXT;
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS search_vector TSVECTOR
    GENERATED ALWAYS AS (to_tsvector('english', COALESCE(transcript_text, ''))) STORED;
CREATE INDEX IF NOT EXISTS idx_session_logs_search ON session_logs USING GIN (search_vector);

-- Add summary column for session overview
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS summary TEXT;
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS message_count INTEGER DEFAULT 0;
