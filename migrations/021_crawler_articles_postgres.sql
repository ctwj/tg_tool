-- 021: 爬虫文章表 crawler_articles (PostgreSQL) — feature 042-web-crawler-collector
-- task_id ON DELETE SET NULL：任务删除时文章保留（FR-033）

CREATE TABLE IF NOT EXISTS crawler_articles (
    id BIGSERIAL PRIMARY KEY,
    task_id BIGINT REFERENCES crawler_tasks(id) ON DELETE SET NULL,
    source_type VARCHAR(255) NOT NULL,
    source_url TEXT NOT NULL,
    source_url_canonical TEXT NOT NULL,
    title TEXT,
    content TEXT,
    category VARCHAR(255),
    tags TEXT,
    is_edited BOOLEAN NOT NULL DEFAULT FALSE,
    crawled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_articles_task_canonical ON crawler_articles(task_id, source_url_canonical);
CREATE INDEX IF NOT EXISTS idx_articles_task ON crawler_articles(task_id, crawled_at DESC);
CREATE INDEX IF NOT EXISTS idx_articles_source_type ON crawler_articles(source_type);
CREATE INDEX IF NOT EXISTS idx_articles_crawled ON crawler_articles(crawled_at DESC);
