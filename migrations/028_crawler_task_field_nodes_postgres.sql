-- 028: 任务字段树节点表 crawler_task_field_nodes (PostgreSQL) — feature 043-crawler-configurator

CREATE TABLE IF NOT EXISTS crawler_task_field_nodes (
    id BIGSERIAL PRIMARY KEY,
    task_id BIGINT NOT NULL REFERENCES crawler_tasks(id) ON DELETE CASCADE,
    parent_id BIGINT REFERENCES crawler_task_field_nodes(id) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('list_page','detail_page')),
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    source_layer TEXT NOT NULL,
    extractor_mode TEXT NOT NULL,
    rule_json TEXT NOT NULL,
    post_processors_json TEXT,
    script_index INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(task_id, scope, parent_id, name)
);

CREATE INDEX IF NOT EXISTS idx_field_nodes_task ON crawler_task_field_nodes(task_id);
CREATE INDEX IF NOT EXISTS idx_field_nodes_parent ON crawler_task_field_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_field_nodes_task_scope ON crawler_task_field_nodes(task_id, scope, sort_order);
