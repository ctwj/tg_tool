-- 023: 文章图片表 crawler_article_images (PostgreSQL) — feature 042-web-crawler-collector

CREATE TABLE IF NOT EXISTS crawler_article_images (
    id BIGSERIAL PRIMARY KEY,
    article_id BIGINT NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    original_url TEXT NOT NULL,
    url_canonical TEXT NOT NULL,
    local_path TEXT,
    image_message_id BIGINT,
    file_id TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_images_article ON crawler_article_images(article_id);
CREATE INDEX IF NOT EXISTS idx_images_status ON crawler_article_images(status, retry_count);
CREATE INDEX IF NOT EXISTS idx_images_url_canonical ON crawler_article_images(url_canonical);
