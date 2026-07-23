-- 020: 爬虫任务表 crawler_tasks (SQLite) — feature 042-web-crawler-collector

CREATE TABLE IF NOT EXISTS crawler_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    list_urls TEXT NOT NULL,
    selectors TEXT NOT NULL,
    two_stage BOOLEAN NOT NULL DEFAULT 1,
    interval_minutes INTEGER NOT NULL DEFAULT 30,
    task_concurrency INTEGER NOT NULL DEFAULT 1 CHECK (task_concurrency >= 1),
    user_agent TEXT,
    request_delay_ms INTEGER NOT NULL DEFAULT 1000,
    proxy TEXT,
    auto_link_check BOOLEAN NOT NULL DEFAULT 0,
    block_detection_config TEXT,
    max_consecutive_failures INTEGER NOT NULL DEFAULT 3,
    template_source TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_run_at TIMESTAMP,
    next_run_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 调度扫描索引：active + 到期
CREATE INDEX IF NOT EXISTS idx_tasks_status_next_run ON crawler_tasks(status, next_run_at);
-- 启停筛选
CREATE INDEX IF NOT EXISTS idx_tasks_enabled ON crawler_tasks(enabled);
