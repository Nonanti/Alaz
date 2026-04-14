-- Debounced learning queue: stores pending learn requests.
-- Background job picks the latest request per session after a cooldown period.
CREATE TABLE IF NOT EXISTS learning_queue (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    session_id TEXT NOT NULL,
    project_id TEXT,
    transcript TEXT NOT NULL,
    message_count INTEGER DEFAULT 0,
    queued_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, processing, completed, cancelled
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_learning_queue_session ON learning_queue (session_id, queued_at DESC);
CREATE INDEX IF NOT EXISTS idx_learning_queue_status ON learning_queue (status, queued_at);
