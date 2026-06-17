-- Migration 016: users 增加 must_change_password 字段（feature 027，SEC-002）
-- 标记账号需在下次登录强制改密（全新随机口令 root / 存量弱口令 root 置 TRUE，改密后清 FALSE）
ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT FALSE;
