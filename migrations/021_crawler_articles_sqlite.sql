-- 021: 爬虫文章表 crawler_articles (SQLite) — feature 042-web-crawler-collector
-- task_id ON DELETE SET NULL：任务删除时文章保留（FR-033）

CREATE TABLE IF NOT EXISTS crawler_articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER REFERENCES crawler_tasks(id) ON DELETE SET NULL,
    source_type TEXT NOT NULL,
    source_url TEXT NOT NULL,
    source_url_canonical TEXT NOT NULL,
    title TEXT,
    content TEXT,
    category TEXT,
    tags TEXT,
    is_edited BOOLEAN NOT NULL DEFAULT 0,
    crawled_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 同任务内 URL 幂等（FR-022）
CREATE UNIQUE INDEX IF NOT EXISTS idx_articles_task_canonical ON crawler_articles(task_id, source_url_canonical);
-- 任务下按时间倒序
CREATE INDEX IF NOT EXISTS idx_articles_task ON crawler_articles(task_id, crawled_at DESC);
-- 按来源筛选（未来推送接入）
CREATE INDEX IF NOT EXISTS idx_articles_source_type ON crawler_articles(source_type);
-- 全局按时间筛选
CREATE INDEX IF NOT EXISTS idx_articles_crawled ON crawler_articles(crawled_at DESC);
