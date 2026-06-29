-- 030: crawler_tasks 新增 max_pagination_depth（feature 043 US5 T056）
-- 旧 042 路径用 max_pages（0=不限）；043 字段树路径下 pagination 字段驱动翻页，
-- 为避免无界循环，默认上限 10 页（FR-022）。
-- 0 表示不限（与 max_pages 语义一致，便于一致表达"翻到底"）。

ALTER TABLE crawler_tasks ADD COLUMN IF NOT EXISTS max_pagination_depth INTEGER NOT NULL DEFAULT 10;
