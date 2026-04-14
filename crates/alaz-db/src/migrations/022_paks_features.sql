-- Feature #1: Structured session state
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS goals TEXT[];
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS accomplished TEXT[];
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS pending TEXT[];
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS handoff_summary TEXT;
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS working_context JSONB;
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS related_files TEXT[];
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS current_task TEXT;

-- Feature #2: Work units (cross-session task tracking)
CREATE TABLE IF NOT EXISTS work_units (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    name TEXT NOT NULL,
    description TEXT,
    goal TEXT,
    project_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',  -- active, completed, paused, cancelled
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_work_units_project ON work_units (project_id, status);
CREATE INDEX IF NOT EXISTS idx_work_units_status ON work_units (status, updated_at DESC);

-- Link sessions to work units
ALTER TABLE session_logs ADD COLUMN IF NOT EXISTS work_unit_id TEXT REFERENCES work_units(id);

-- Feature #4: Message-based session storage
CREATE TABLE IF NOT EXISTS session_messages (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,  -- user, assistant, system
    content TEXT NOT NULL,
    tool_use JSONB,
    tool_result JSONB,
    model TEXT,
    search_text TEXT,
    search_vector TSVECTOR GENERATED ALWAYS AS (to_tsvector('english', COALESCE(search_text, ''))) STORED,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_session_messages_session ON session_messages (session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_session_messages_role ON session_messages (session_id, role);
CREATE INDEX IF NOT EXISTS idx_session_messages_search ON session_messages USING GIN (search_vector);

-- Feature #5: Vault audit depth
ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS resource_type TEXT;
ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS resource_id TEXT;
ALTER TABLE audit_logs ADD COLUMN IF NOT EXISTS duration_ms BIGINT;

-- Feature #6: Code symbols depth — add call graph columns
ALTER TABLE code_symbols ADD COLUMN IF NOT EXISTS callers TEXT[];
ALTER TABLE code_symbols ADD COLUMN IF NOT EXISTS callees TEXT[];
ALTER TABLE code_symbols ADD COLUMN IF NOT EXISTS import_path TEXT;
ALTER TABLE code_symbols ADD COLUMN IF NOT EXISTS complexity_score REAL;
