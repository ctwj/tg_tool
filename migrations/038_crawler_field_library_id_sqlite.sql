-- 038: crawler_field_library 增量插入 id 字段 (SQLite) — feature 046 后续增强
-- 用途：在 metadata 分类下新增"站点资源 ID"预置字段（如 discuz thread-12345 / wordpress post-12345）
-- 用于跨页去重和站内唯一标识；regex 模式提取（默认从 URL/HTML 提取数字 ID）
-- INSERT OR IGNORE 保证幂等（与 027 种子风格一致）

INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('id', 'ID', 'string', 'metadata', '站点资源 ID（如 discuz thread-12345 中的 12345 / wordpress post-12345 中的 12345，用于跨页去重和站内唯一标识）', 'regex', 7);
