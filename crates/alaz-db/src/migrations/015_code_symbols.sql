-- Codebase awareness: index code symbols for impact analysis.
CREATE TABLE IF NOT EXISTS code_symbols (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id),
    file_path TEXT NOT NULL,
    symbol_name TEXT NOT NULL,
    symbol_type TEXT NOT NULL, -- 'function', 'struct', 'enum', 'trait', 'impl', 'const', 'type_alias', 'module'
    signature TEXT,            -- Full signature line (e.g., "pub async fn search(&self, query: &str) -> Result<Vec<Item>>")
    line_number INT NOT NULL DEFAULT 0,
    visibility TEXT NOT NULL DEFAULT 'private', -- 'public', 'crate', 'private'
    parent_symbol TEXT,        -- Enclosing impl/trait (e.g., "KnowledgeRepo" for methods)
    dependencies TEXT[] NOT NULL DEFAULT '{}',  -- Symbols this depends on (imports, calls)
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_code_symbols_project ON code_symbols(project_id);
CREATE INDEX IF NOT EXISTS idx_code_symbols_file ON code_symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_code_symbols_name ON code_symbols(symbol_name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_code_symbols_unique
    ON code_symbols(project_id, file_path, symbol_name, symbol_type);
