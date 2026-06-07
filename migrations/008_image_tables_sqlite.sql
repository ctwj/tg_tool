-- Migration 008: Image mappings + forward tasks tables (SQLite)

-- remote_id -> file_id mapping, created after sendPhoto succeeds
CREATE TABLE IF NOT EXISTS image_mappings (
    remote_id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Forward task queue
CREATE TABLE IF NOT EXISTS forward_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    remote_id TEXT NOT NULL,
    channel_id INTEGER,
    message_id INTEGER,
    title TEXT,
    description TEXT,
    link TEXT,
    file_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Index for pending task queries
CREATE INDEX IF NOT EXISTS idx_forward_tasks_status ON forward_tasks(status);
