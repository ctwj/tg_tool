-- 032: crawler_tasks 新增 force_full_collect（feature 044）
-- ON=每次全量采集；OFF=连续 3 页零新增早停
ALTER TABLE crawler_tasks ADD COLUMN IF NOT EXISTS force_full_collect BOOLEAN NOT NULL DEFAULT true;
