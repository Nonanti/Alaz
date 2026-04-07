-- Git integration: track commit-level file changes for pattern detection.
CREATE TABLE IF NOT EXISTS git_activity (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id),
    commit_hash TEXT NOT NULL,
    commit_message TEXT NOT NULL DEFAULT '',
    file_path TEXT NOT NULL,
    change_type TEXT NOT NULL, -- 'add', 'modify', 'delete', 'rename'
    lines_added INT NOT NULL DEFAULT 0,
    lines_removed INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_git_activity_project ON git_activity(project_id);
CREATE INDEX IF NOT EXISTS idx_git_activity_file_path ON git_activity(file_path);
CREATE INDEX IF NOT EXISTS idx_git_activity_created_at ON git_activity(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_git_activity_commit ON git_activity(commit_hash);
