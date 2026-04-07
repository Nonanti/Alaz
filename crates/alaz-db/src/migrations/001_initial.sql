-- 001_initial.sql: Full Alaz schema
-- Idempotent: uses IF NOT EXISTS throughout

-- Extensions
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS vector;

-- ============================================================
-- Projects
-- ============================================================
CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    path        TEXT,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Knowledge Items
-- ============================================================
CREATE TABLE IF NOT EXISTS knowledge_items (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    description     TEXT,
    type            TEXT NOT NULL DEFAULT 'artifact',
    language        TEXT,
    file_path       TEXT,
    project_id      TEXT REFERENCES projects(id),
    tags            TEXT[] NOT NULL DEFAULT '{}',
    access_count    BIGINT NOT NULL DEFAULT 0,
    last_accessed_at TIMESTAMPTZ,
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    valid_from      TIMESTAMPTZ,
    valid_until     TIMESTAMPTZ,
    superseded_by   TEXT,
    search_vector   TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(description, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'C')
    ) STORED,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_knowledge_items_search_vector
    ON knowledge_items USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_project_id
    ON knowledge_items (project_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_type
    ON knowledge_items (type);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_tags
    ON knowledge_items USING GIN (tags);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_title_trgm
    ON knowledge_items USING GIN (title gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_needs_embedding
    ON knowledge_items (id) WHERE needs_embedding = TRUE;

-- ============================================================
-- Episodes
-- ============================================================
CREATE TABLE IF NOT EXISTS episodes (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    type            TEXT NOT NULL DEFAULT 'discovery',
    severity        TEXT,
    resolved        BOOLEAN NOT NULL DEFAULT FALSE,
    who_cues        TEXT[] NOT NULL DEFAULT '{}',
    what_cues       TEXT[] NOT NULL DEFAULT '{}',
    where_cues      TEXT[] NOT NULL DEFAULT '{}',
    when_cues       TEXT[] NOT NULL DEFAULT '{}',
    why_cues        TEXT[] NOT NULL DEFAULT '{}',
    project_id      TEXT REFERENCES projects(id),
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    search_vector   TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'B')
    ) STORED,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_episodes_search_vector
    ON episodes USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_episodes_project_id
    ON episodes (project_id);
CREATE INDEX IF NOT EXISTS idx_episodes_type
    ON episodes (type);
CREATE INDEX IF NOT EXISTS idx_episodes_resolved
    ON episodes (resolved);
CREATE INDEX IF NOT EXISTS idx_episodes_who_cues
    ON episodes USING GIN (who_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_what_cues
    ON episodes USING GIN (what_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_where_cues
    ON episodes USING GIN (where_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_when_cues
    ON episodes USING GIN (when_cues);
CREATE INDEX IF NOT EXISTS idx_episodes_why_cues
    ON episodes USING GIN (why_cues);

-- ============================================================
-- Procedures
-- ============================================================
CREATE TABLE IF NOT EXISTS procedures (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    steps           JSONB NOT NULL DEFAULT '[]',
    times_used      BIGINT NOT NULL DEFAULT 0,
    times_success   BIGINT NOT NULL DEFAULT 0,
    times_failure   BIGINT NOT NULL DEFAULT 0,
    success_rate    DOUBLE PRECISION GENERATED ALWAYS AS (
        CASE WHEN times_used > 0
            THEN times_success::DOUBLE PRECISION / times_used::DOUBLE PRECISION
            ELSE NULL
        END
    ) STORED,
    project_id      TEXT REFERENCES projects(id),
    tags            TEXT[] NOT NULL DEFAULT '{}',
    needs_embedding BOOLEAN NOT NULL DEFAULT TRUE,
    search_vector   TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(content, '')), 'B')
    ) STORED,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_procedures_project_id
    ON procedures (project_id);
CREATE INDEX IF NOT EXISTS idx_procedures_tags
    ON procedures USING GIN (tags);

-- ============================================================
-- Core Memories
-- ============================================================
CREATE TABLE IF NOT EXISTS core_memories (
    id              TEXT PRIMARY KEY,
    category        TEXT NOT NULL,
    key             TEXT NOT NULL,
    value           TEXT NOT NULL,
    confidence      DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    confirmations   BIGINT NOT NULL DEFAULT 1,
    contradictions  BIGINT NOT NULL DEFAULT 0,
    project_id      TEXT REFERENCES projects(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (category, key, project_id)
);

-- Partial unique index for NULL project_id (PostgreSQL UNIQUE treats NULLs as distinct)
CREATE UNIQUE INDEX IF NOT EXISTS idx_core_memories_category_key_null_project
    ON core_memories (category, key) WHERE project_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_core_memories_project_id
    ON core_memories (project_id);
CREATE INDEX IF NOT EXISTS idx_core_memories_category
    ON core_memories (category);

-- ============================================================
-- Session Logs
-- ============================================================
CREATE TABLE IF NOT EXISTS session_logs (
    id               TEXT PRIMARY KEY,
    project_id       TEXT REFERENCES projects(id),
    cost             DOUBLE PRECISION,
    input_tokens     BIGINT,
    output_tokens    BIGINT,
    duration_seconds DOUBLE PRECISION,
    tools_used       JSONB NOT NULL DEFAULT '[]',
    status           TEXT,
    summary          TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_session_logs_project_id
    ON session_logs (project_id);
CREATE INDEX IF NOT EXISTS idx_session_logs_status
    ON session_logs (status);

-- ============================================================
-- Session Checkpoints
-- ============================================================
CREATE TABLE IF NOT EXISTS session_checkpoints (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES session_logs(id) ON DELETE CASCADE,
    checkpoint_data JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_session_checkpoints_session_id
    ON session_checkpoints (session_id);

-- ============================================================
-- Reflections
-- ============================================================
CREATE TABLE IF NOT EXISTS reflections (
    id                  TEXT PRIMARY KEY,
    session_id          TEXT NOT NULL REFERENCES session_logs(id) ON DELETE CASCADE,
    what_worked         TEXT,
    what_failed         TEXT,
    lessons_learned     TEXT,
    effectiveness_score DOUBLE PRECISION,
    complexity_score    DOUBLE PRECISION,
    project_id          TEXT REFERENCES projects(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_reflections_session_id
    ON reflections (session_id);
CREATE INDEX IF NOT EXISTS idx_reflections_project_id
    ON reflections (project_id);

-- ============================================================
-- Graph Edges
-- ============================================================
CREATE TABLE IF NOT EXISTS graph_edges (
    id           TEXT PRIMARY KEY,
    source_type  TEXT NOT NULL,
    source_id    TEXT NOT NULL,
    target_type  TEXT NOT NULL,
    target_id    TEXT NOT NULL,
    relation     TEXT NOT NULL,
    weight       DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    usage_count  BIGINT NOT NULL DEFAULT 1,
    description  TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    UNIQUE (source_type, source_id, target_type, target_id, relation)
);

CREATE INDEX IF NOT EXISTS idx_graph_edges_source
    ON graph_edges (source_type, source_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_target
    ON graph_edges (target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_graph_edges_relation
    ON graph_edges (relation);

-- ============================================================
-- RAPTOR Trees
-- ============================================================
CREATE TABLE IF NOT EXISTS raptor_trees (
    id          TEXT PRIMARY KEY,
    project_id  TEXT UNIQUE REFERENCES projects(id),
    status      TEXT NOT NULL DEFAULT 'pending',
    total_nodes BIGINT NOT NULL DEFAULT 0,
    max_depth   INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- RAPTOR Nodes
-- ============================================================
CREATE TABLE IF NOT EXISTS raptor_nodes (
    id             TEXT PRIMARY KEY,
    tree_id        TEXT NOT NULL REFERENCES raptor_trees(id) ON DELETE CASCADE,
    level          INTEGER NOT NULL DEFAULT 0,
    parent_id      TEXT REFERENCES raptor_nodes(id),
    entity_type    TEXT NOT NULL,
    entity_id      TEXT NOT NULL,
    summary        TEXT,
    children_count INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_raptor_nodes_tree_id
    ON raptor_nodes (tree_id);
CREATE INDEX IF NOT EXISTS idx_raptor_nodes_parent_id
    ON raptor_nodes (parent_id);

-- ============================================================
-- Auth: Owners
-- ============================================================
CREATE TABLE IF NOT EXISTS owners (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Auth: Devices
-- ============================================================
CREATE TABLE IF NOT EXISTS devices (
    id           TEXT PRIMARY KEY,
    owner_id     TEXT NOT NULL REFERENCES owners(id),
    fingerprint  TEXT NOT NULL UNIQUE,
    name         TEXT,
    trusted      BOOLEAN NOT NULL DEFAULT FALSE,
    last_seen_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_devices_owner_id
    ON devices (owner_id);

-- ============================================================
-- Auth: API Keys
-- ============================================================
CREATE TABLE IF NOT EXISTS api_keys (
    id           TEXT PRIMARY KEY,
    owner_id     TEXT NOT NULL REFERENCES owners(id),
    key_hash     TEXT NOT NULL UNIQUE,
    name         TEXT,
    last_used_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_owner_id
    ON api_keys (owner_id);

-- ============================================================
-- Auth: Audit Logs
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id         TEXT PRIMARY KEY,
    owner_id   TEXT,
    event      TEXT NOT NULL,
    details    JSONB NOT NULL DEFAULT '{}',
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_owner_id
    ON audit_logs (owner_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_event
    ON audit_logs (event);

-- ============================================================
-- Orchestration Jobs
-- ============================================================
CREATE TABLE IF NOT EXISTS orchestration_jobs (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'pending',
    tasks      JSONB NOT NULL DEFAULT '[]',
    results    JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
