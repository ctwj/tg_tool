-- 020: 爬虫任务表 crawler_tasks (PostgreSQL) — feature 042-web-crawler-collector

CREATE TABLE IF NOT EXISTS crawler_tasks (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    list_urls TEXT NOT NULL,
    selectors TEXT NOT NULL,
    two_stage BOOLEAN NOT NULL DEFAULT TRUE,
    interval_minutes INTEGER NOT NULL DEFAULT 30,
    task_concurrency INTEGER NOT NULL DEFAULT 1 CHECK (task_concurrency >= 1),
    user_agent TEXT,
    request_delay_ms INTEGER NOT NULL DEFAULT 1000,
    proxy TEXT,
    auto_link_check BOOLEAN NOT NULL DEFAULT FALSE,
    block_detection_config TEXT,
    max_consecutive_failures INTEGER NOT NULL DEFAULT 3,
    template_source VARCHAR(50),
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_tasks_status_next_run ON crawler_tasks(status, next_run_at);
CREATE INDEX IF NOT EXISTS idx_tasks_enabled ON crawler_tasks(enabled);
