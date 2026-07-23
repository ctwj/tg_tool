-- 037: crawler_task_field_nodes 加 refresh_on_read 列 (SQLite) — feature 046-crawler-script-extractor
-- 用途：脚本字段（extractor_mode=script）配置读取时按需刷新（lazy refresh on read）开关。
-- 默认 FALSE：向后兼容，现有所有字段 refresh_on_read=false，行为不变。
-- 仅 script 模式字段配置 true 时有意义；非 script 字段配置由应用层 validate 拒绝。

ALTER TABLE crawler_task_field_nodes
    ADD COLUMN refresh_on_read BOOLEAN NOT NULL DEFAULT 0;
