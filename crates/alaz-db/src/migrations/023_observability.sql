-- Structured logs: application logs captured from tracing layer
CREATE TABLE IF NOT EXISTS structured_logs (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    level TEXT NOT NULL,  -- trace, debug, info, warn, error
    target TEXT NOT NULL, -- module path (e.g., "alaz_server::jobs")
    message TEXT NOT NULL,
    fields JSONB,         -- structured fields from tracing event
    fingerprint TEXT,     -- for error grouping (sha256 of target + normalized message)
    search_vector TSVECTOR GENERATED ALWAYS AS (to_tsvector('english', message)) STORED
);

CREATE INDEX IF NOT EXISTS idx_structured_logs_timestamp ON structured_logs (timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_structured_logs_level ON structured_logs (level, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_structured_logs_target ON structured_logs (target, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_structured_logs_fingerprint ON structured_logs (fingerprint) WHERE fingerprint IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_structured_logs_search ON structured_logs USING GIN (search_vector);

-- Error groups: fingerprint-based aggregation of errors (Sentry-style)
CREATE TABLE IF NOT EXISTS error_groups (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    fingerprint TEXT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    target TEXT NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_count BIGINT NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'unresolved',  -- unresolved, resolved, ignored
    resolved_at TIMESTAMPTZ,
    resolution_notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_error_groups_last_seen ON error_groups (status, last_seen DESC);
CREATE INDEX IF NOT EXISTS idx_error_groups_event_count ON error_groups (event_count DESC);

-- Alert rules: conditions to trigger alerts
CREATE TABLE IF NOT EXISTS alert_rules (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    name TEXT NOT NULL,
    description TEXT,
    condition_type TEXT NOT NULL,  -- error_rate, log_level_count, specific_target
    threshold INTEGER NOT NULL,
    window_secs INTEGER NOT NULL DEFAULT 300,
    filter_level TEXT,             -- e.g., "error"
    filter_target TEXT,            -- e.g., "alaz_intel::llm"
    filter_pattern TEXT,           -- regex pattern for message
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_triggered_at TIMESTAMPTZ,
    trigger_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_alert_rules_enabled ON alert_rules (enabled, condition_type);

-- Alert history: log of triggered alerts
CREATE TABLE IF NOT EXISTS alert_history (
    id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    alert_rule_id TEXT NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    triggered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    matched_count INTEGER NOT NULL,
    details JSONB
);

CREATE INDEX IF NOT EXISTS idx_alert_history_rule ON alert_history (alert_rule_id, triggered_at DESC);
