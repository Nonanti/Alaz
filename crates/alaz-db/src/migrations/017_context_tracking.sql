-- Context injection tracking: measure which injected entities get referenced during sessions
CREATE TABLE IF NOT EXISTS context_injections (
    id TEXT PRIMARY KEY,
    session_id TEXT,
    project_id TEXT,
    injected_entity_ids TEXT[] DEFAULT '{}',
    injected_sections TEXT[] DEFAULT '{}',
    tokens_used BIGINT DEFAULT 0,
    referenced_entity_ids TEXT[] DEFAULT '{}',
    reference_count INT DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_context_injections_session ON context_injections(session_id);
CREATE INDEX IF NOT EXISTS idx_context_injections_created ON context_injections(created_at);
