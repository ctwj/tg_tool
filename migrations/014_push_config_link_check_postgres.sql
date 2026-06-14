-- 014: 推送配置加「推送前链接有效性检测」开关 — 默认开启（保持原行为）
-- 关闭后推送时不调用 LinkChecker，跳过链接失效过滤，仅保留图片未转存检查
ALTER TABLE push_configs ADD COLUMN link_check_before_push BOOLEAN NOT NULL DEFAULT true;
