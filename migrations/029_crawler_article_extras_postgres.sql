-- 029: 文章扩展字段值表 + crawler_articles.extra_fields_json (PostgreSQL) — feature 043-crawler-configurator

CREATE TABLE IF NOT EXISTS crawler_article_field_values (
    id BIGSERIAL PRIMARY KEY,
    article_id BIGINT NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    field_node_id BIGINT REFERENCES crawler_task_field_nodes(id) ON DELETE SET NULL,
    field_path TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('list_page','detail_page')),
    value_index INTEGER NOT NULL DEFAULT 0,
    value_text TEXT,
    value_number DOUBLE PRECISION,
    is_hit BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_field_values_article ON crawler_article_field_values(article_id, field_path);
CREATE INDEX IF NOT EXISTS idx_field_values_field ON crawler_article_field_values(field_node_id, is_hit);
-- PostgreSQL 部分索引
CREATE INDEX IF NOT EXISTS idx_field_values_unhit ON crawler_article_field_values(is_hit) WHERE is_hit = FALSE;

ALTER TABLE crawler_articles ADD COLUMN IF NOT EXISTS extra_fields_json TEXT;
