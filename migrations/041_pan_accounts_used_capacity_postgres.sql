-- 041: pan_accounts 加 used_capacity_bytes 列 (PostgreSQL) — feature 047
-- 用途：网盘账号已用容量（字节），与 capacity_bytes（总配额）配对。
-- 默认 NULL：向后兼容，未校验过的账号不显示已用；下次 check_account 后填充。

ALTER TABLE pan_accounts
    ADD COLUMN IF NOT EXISTS used_capacity_bytes BIGINT;
