-- 031: crawler_field_library resource 类补 download_url / resource_name + sort_order 重排 (SQLite)
-- 修复已运行过 027（旧 24 条）的现有 DB：把 file_size/duration/version 的 sort_order 后移 2，
-- 再补插 download_url / resource_name（INSERT OR IGNORE 幂等）。
-- 全新 DB 已由 027 直接写入正确数据，本迁移幂等无副作用。

-- 1) 原 resource 类 3 条 sort_order 后移 2（仅当 download_url 尚不存在时执行，避免重复加偏移）
UPDATE crawler_field_library
SET sort_order = sort_order + 2, updated_at = CURRENT_TIMESTAMP
WHERE category = 'resource'
  AND key IN ('file_size', 'duration', 'version')
  AND NOT EXISTS (SELECT 1 FROM crawler_field_library WHERE key = 'download_url');

-- 2) 补插新字段（已存在则跳过）
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('download_url',  '下载地址', 'url',    'resource', '资源下载地址（直链或网盘）。可能需用 follow_url 模式两阶段提取',   'css',   1),
    ('resource_name', '资源名',   'string', 'resource', '资源名称（区别于文章标题 title，适用于一篇文章列多个资源的场景）', 'css',   2);
