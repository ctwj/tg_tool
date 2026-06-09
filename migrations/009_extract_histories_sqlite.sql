-- Migration 009: Create extract_histories table (SQLite)

CREATE TABLE IF NOT EXISTS extract_histories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    status VARCHAR(20) NOT NULL,
    total_scanned INTEGER NOT NULL DEFAULT 0,
    extracted INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    executed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_extract_histories_executed_at ON extract_histories(executed_at DESC);
CREATE INDEX IF NOT EXISTS idx_extract_histories_status ON extract_histories(status);
