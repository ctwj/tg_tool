-- 029: 文章扩展字段值表 + crawler_articles.extra_fields_json (SQLite) — feature 043-crawler-configurator
-- data-model.md E4/E5：双轨存储（长表 + JSON 聚合）

CREATE TABLE IF NOT EXISTS crawler_article_field_values (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    article_id INTEGER NOT NULL REFERENCES crawler_articles(id) ON DELETE CASCADE,
    field_node_id INTEGER REFERENCES crawler_task_field_nodes(id) ON DELETE SET NULL,
    field_path TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('list_page','detail_page')),
    value_index INTEGER NOT NULL DEFAULT 0,
    value_text TEXT,
    value_number REAL,
    is_hit BOOLEAN NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_field_values_article ON crawler_article_field_values(article_id, field_path);
CREATE INDEX IF NOT EXISTS idx_field_values_field ON crawler_article_field_values(field_node_id, is_hit);
CREATE INDEX IF NOT EXISTS idx_field_values_unhit ON crawler_article_field_values(is_hit) WHERE is_hit = 0;

-- crawler_articles 新增 extra_fields_json 列：列表页快速渲染用聚合
ALTER TABLE crawler_articles ADD COLUMN extra_fields_json TEXT;
