-- Migration 003: Create extracted_resources table (SQLite)

CREATE TABLE IF NOT EXISTS extracted_resources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collector_history_id INTEGER NOT NULL REFERENCES collector_histories(id),
    title TEXT NOT NULL,
    url TEXT,
    description TEXT,
    category TEXT,
    tags TEXT,
    img TEXT,
    source TEXT NOT NULL DEFAULT 'tg',
    extra TEXT,
    extract_mode TEXT NOT NULL DEFAULT 'rule',
    is_pushed BOOLEAN NOT NULL DEFAULT 0,
    is_edited BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_extracted_resources_collector_history_id ON extracted_resources(collector_history_id);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_is_pushed ON extracted_resources(is_pushed);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_category ON extracted_resources(category);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_created_at ON extracted_resources(created_at DESC);
