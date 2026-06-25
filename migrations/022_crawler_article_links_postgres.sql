-- 022: 文章链接表 crawler_article_links (PostgreSQL) — feature 042-web-crawler-collector

CREATE TABLE IF NOT EXISTS crawler_article_links (
    id BIGSERIAL PRIMARY KEY,
    article_id BIGINT NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    link_type VARCHAR(20) NOT NULL CHECK (link_type IN ('pan', 'direct')),
    platform VARCHAR(50),
    url TEXT NOT NULL,
    url_canonical TEXT NOT NULL,
    extract_code VARCHAR(20),
    validity_status VARCHAR(20) NOT NULL DEFAULT 'unknown',
    validity_reason TEXT,
    last_checked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_links_article ON crawler_article_links(article_id);
CREATE INDEX IF NOT EXISTS idx_links_url_canonical ON crawler_article_links(url_canonical);
CREATE INDEX IF NOT EXISTS idx_links_validity ON crawler_article_links(validity_status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_links_article_url_type ON crawler_article_links(article_id, url_canonical, link_type);
