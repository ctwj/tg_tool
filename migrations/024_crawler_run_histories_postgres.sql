-- 024: 爬虫运行历史 crawler_run_histories (PostgreSQL) — feature 042-web-crawler-collector

CREATE TABLE IF NOT EXISTS crawler_run_histories (
    id BIGSERIAL PRIMARY KEY,
    task_id BIGINT NOT NULL REFERENCES crawler_tasks(id) ON DELETE CASCADE,
    task_name VARCHAR(255) NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    duration_ms BIGINT,
    status VARCHAR(20) NOT NULL,
    block_type VARCHAR(50),
    crawled_count INTEGER NOT NULL DEFAULT 0,
    new_count INTEGER NOT NULL DEFAULT 0,
    skipped_count INTEGER NOT NULL DEFAULT 0,
    failed_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_history_task ON crawler_run_histories(task_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_status ON crawler_run_histories(status, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_started ON crawler_run_histories(started_at DESC);
