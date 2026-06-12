-- 013: 资源链接有效性检测 — link_check_results + push_skip_records + push_histories 跳过统计列 (SQLite)

-- 链接检测结果（以归一化 URL 为缓存键，跨资源/跨推送去重）
CREATE TABLE IF NOT EXISTS link_check_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url_hash TEXT NOT NULL UNIQUE,
    normalized_url TEXT NOT NULL,
    platform TEXT,
    status TEXT NOT NULL,
    fail_reason TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_link_check_status ON link_check_results(status);
CREATE INDEX IF NOT EXISTS idx_link_check_expires ON link_check_results(expires_at);

-- 推送跳过明细（每次推送被跳过的资源及原因）
CREATE TABLE IF NOT EXISTS push_skip_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    push_history_id INTEGER NOT NULL REFERENCES push_histories(id) ON DELETE CASCADE,
    resource_id INTEGER NOT NULL REFERENCES extracted_resources(id) ON DELETE CASCADE,
    skip_reason TEXT NOT NULL,
    urls_invalid TEXT,
    detail TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_push_skip_history ON push_skip_records(push_history_id);

-- push_histories 新增跳过统计汇总列（明细见 push_skip_records）
ALTER TABLE push_histories ADD COLUMN pushed_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE push_histories ADD COLUMN skipped_image_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE push_histories ADD COLUMN skipped_link_count INTEGER NOT NULL DEFAULT 0;
