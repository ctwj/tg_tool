-- 020: 网盘账号管理与链接转存 (SQLite) — feature 047

-- 网盘账号（夸克/UC/百度），凭据 AES-256-GCM 加密存储
CREATE TABLE IF NOT EXISTS pan_accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,              -- quark | uc | baidu
    display_name TEXT NOT NULL,
    credential_cipher TEXT NOT NULL,     -- base64 密文
    credential_nonce TEXT NOT NULL,      -- base64 GCM nonce(12B)
    status TEXT NOT NULL DEFAULT 'active', -- active | disabled | expired
    target_dir TEXT NOT NULL,            -- 固定转存/上传目录（平铺）
    capacity_bytes INTEGER,
    last_checked_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_pan_accounts_platform ON pan_accounts(platform);

-- 分享记录（由我方账号生成）
CREATE TABLE IF NOT EXISTS share_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL REFERENCES pan_accounts(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    share_url TEXT NOT NULL,
    extract_code TEXT,
    remote_file_id TEXT,
    expires_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_share_records_account ON share_records(account_id);

-- 转存/上传任务（状态机 + 幂等）
CREATE TABLE IF NOT EXISTS transfer_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_url TEXT NOT NULL,
    source_type TEXT NOT NULL,           -- pan_share | direct_link
    source_platform TEXT,                -- quark | uc | baidu（直链为 NULL）
    extract_code TEXT,
    target_account_id INTEGER NOT NULL REFERENCES pan_accounts(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending', -- pending | processing | succeeded | failed
    failure_reason TEXT,
    share_id INTEGER REFERENCES share_records(id),
    source_origin TEXT NOT NULL DEFAULT 'manual', -- manual | api | resource_integration
    idempotency_key TEXT NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_transfer_tasks_idem ON transfer_tasks(idempotency_key);
CREATE INDEX IF NOT EXISTS idx_transfer_tasks_status ON transfer_tasks(status);
CREATE INDEX IF NOT EXISTS idx_transfer_tasks_account ON transfer_tasks(target_account_id);

-- 开放 API 凭据（每外部系统独立 Key + 配额）
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    system_name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,       -- SHA-256(明文 Key)
    key_prefix TEXT NOT NULL,            -- 明文前 8 位（列表识别）
    status TEXT NOT NULL DEFAULT 'enabled', -- enabled | disabled
    quota_limit INTEGER NOT NULL DEFAULT 0, -- 0 = 无限
    quota_used INTEGER NOT NULL DEFAULT 0,
    quota_reset_at DATETIME,
    rate_limit_qps INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at DATETIME,
    rotated_at DATETIME
);
