-- 023: 文章图片表 crawler_article_images (SQLite) — feature 042-web-crawler-collector
-- 状态机：pending -> downloaded -> uploading -> uploaded / failed（FR-028）
-- 最大重试 3 次（FR-028a），超过 failed 不再自动重试

CREATE TABLE IF NOT EXISTS crawler_article_images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    article_id INTEGER NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    original_url TEXT NOT NULL,
    url_canonical TEXT NOT NULL,
    local_path TEXT,
    image_message_id INTEGER,
    file_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_images_article ON crawler_article_images(article_id);
-- worker 扫描待处理项
CREATE INDEX IF NOT EXISTS idx_images_status ON crawler_article_images(status, retry_count);
-- 跨文章去重下载
CREATE INDEX IF NOT EXISTS idx_images_url_canonical ON crawler_article_images(url_canonical);
