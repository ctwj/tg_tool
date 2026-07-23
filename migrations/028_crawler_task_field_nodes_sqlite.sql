-- 028: 任务字段树节点表 crawler_task_field_nodes (SQLite) — feature 043-crawler-configurator
-- data-model.md E3：取代 042 crawler_tasks.selectors JSON 列。
-- 通过 parent_id 自引用表达父子嵌套，sort_order 表达顺序。

CREATE TABLE IF NOT EXISTS crawler_task_field_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES crawler_tasks(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES crawler_task_field_nodes(id) ON DELETE CASCADE,
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
    is_active BOOLEAN NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(task_id, scope, parent_id, name)
);

CREATE INDEX IF NOT EXISTS idx_field_nodes_task ON crawler_task_field_nodes(task_id);
CREATE INDEX IF NOT EXISTS idx_field_nodes_parent ON crawler_task_field_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_field_nodes_task_scope ON crawler_task_field_nodes(task_id, scope, sort_order);
