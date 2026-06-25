-- 024: 爬虫运行历史 crawler_run_histories (SQLite) — feature 042-web-crawler-collector
-- status: success / partial / failed / blocked（FR-040）

CREATE TABLE IF NOT EXISTS crawler_run_histories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES crawler_tasks(id) ON DELETE CASCADE,
    task_name TEXT NOT NULL,
    started_at TIMESTAMP NOT NULL,
    finished_at TIMESTAMP,
    duration_ms INTEGER,
    status TEXT NOT NULL,
    block_type TEXT,
    crawled_count INTEGER NOT NULL DEFAULT 0,
    new_count INTEGER NOT NULL DEFAULT 0,
    skipped_count INTEGER NOT NULL DEFAULT 0,
    failed_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_history_task ON crawler_run_histories(task_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_status ON crawler_run_histories(status, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_started ON crawler_run_histories(started_at DESC);
