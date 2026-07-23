-- 026: 删除 crawler_tasks.selectors 列（feature 043-crawler-configurator）— PostgreSQL 版
-- 043 直接取代 042 旧抓取路径，无生产数据需保留。

ALTER TABLE crawler_tasks DROP COLUMN IF EXISTS selectors;
