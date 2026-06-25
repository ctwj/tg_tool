-- 025: crawler_tasks 自动翻页字段（feature 042 增强）
-- pagination_selector: CSS 选择器，一次性匹配页面所有分页链接（如 .pagination a / a[rel=next]）。
--                      引擎会把所有命中的 href 去重后批量抓取。NULL=未启用
-- max_pages: 最大抓取页数（含 list_urls 中的种子页），0 表示不限（靠选择器失配 + URL 去重自然停止）

ALTER TABLE crawler_tasks ADD COLUMN pagination_selector TEXT;
ALTER TABLE crawler_tasks ADD COLUMN max_pages INTEGER NOT NULL DEFAULT 0;
