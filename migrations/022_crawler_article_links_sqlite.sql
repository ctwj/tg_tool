-- 022: 文章链接表 crawler_article_links (SQLite) — feature 042-web-crawler-collector
-- 存放网盘链接 + 直链；网盘 brand 见 research.md R6（9 平台对齐 PanCheck）

CREATE TABLE IF NOT EXISTS crawler_article_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    article_id INTEGER NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    link_type TEXT NOT NULL CHECK (link_type IN ('pan', 'direct')),
    platform TEXT,
    url TEXT NOT NULL,
    url_canonical TEXT NOT NULL,
    extract_code TEXT,
    validity_status TEXT NOT NULL DEFAULT 'unknown',
    validity_reason TEXT,
    last_checked_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_links_article ON crawler_article_links(article_id);
CREATE INDEX IF NOT EXISTS idx_links_url_canonical ON crawler_article_links(url_canonical);
CREATE INDEX IF NOT EXISTS idx_links_validity ON crawler_article_links(validity_status);
-- 同文章内同链接同类型不重复
CREATE UNIQUE INDEX IF NOT EXISTS idx_links_article_url_type ON crawler_article_links(article_id, url_canonical, link_type);
