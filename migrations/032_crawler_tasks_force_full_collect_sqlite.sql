-- 032: crawler_tasks 新增 force_full_collect（feature 044）
-- ON=每次全量采集（跑满 max_pagination_depth/翻完）；OFF=连续 3 页零新增早停
ALTER TABLE crawler_tasks ADD COLUMN force_full_collect INTEGER NOT NULL DEFAULT 1;
