-- 033: crawler_tasks 新增 URL 模板分页配置（feature 045）
-- page_url_template: 含 {page} 占位符的 URL 模板；空串=未启用（走字段树 pagination 分页）
-- page_start: 模板生成页码起始值（默认 1）
-- page_end: 模板生成页码上限（0=不限，受 max_pagination_depth 与连续空页早停约束）
ALTER TABLE crawler_tasks ADD COLUMN page_url_template TEXT NOT NULL DEFAULT '';
ALTER TABLE crawler_tasks ADD COLUMN page_start INTEGER NOT NULL DEFAULT 1;
ALTER TABLE crawler_tasks ADD COLUMN page_end INTEGER NOT NULL DEFAULT 0;
