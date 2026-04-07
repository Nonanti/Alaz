-- ============================================================
-- Vault Secrets
-- ============================================================
CREATE TABLE IF NOT EXISTS vault_secrets (
    id              TEXT PRIMARY KEY,
    owner_id        TEXT NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    encrypted_value BYTEA NOT NULL,
    nonce           BYTEA NOT NULL,
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(owner_id, name)
);

CREATE INDEX IF NOT EXISTS idx_vault_secrets_owner_id ON vault_secrets(owner_id);
