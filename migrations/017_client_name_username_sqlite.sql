-- Migration 017: clients 增加 name/username 字段（客户端列表显示 Telegram 账号名）
-- 认证成功 / 添加 Bot 时写入；可空，兼容历史数据
ALTER TABLE clients ADD COLUMN name TEXT;
ALTER TABLE clients ADD COLUMN username TEXT;
