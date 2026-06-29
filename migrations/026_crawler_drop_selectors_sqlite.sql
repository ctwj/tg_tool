-- 026: 删除 crawler_tasks.selectors 列（feature 043-crawler-configurator）
-- 043 直接取代 042 旧抓取路径：selectors JSON 列由 crawler_task_field_nodes 字段树表取代。
-- 前向不兼容，但项目尚未上线、无生产数据需保留（参考 spec.md FR-021/22 + research.md R10）。
--
-- SQLite 3.35+ 支持 ALTER TABLE DROP COLUMN（sqlx 0.8 已 bundled 新版 SQLite）。
-- 若旧版 SQLite 报错，按 SQLite 12-step 重建表流程手动处理。

ALTER TABLE crawler_tasks DROP COLUMN selectors;
