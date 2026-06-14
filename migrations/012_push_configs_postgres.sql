-- 012: 推送管理多 API 配置 — push_configs + push_config_collectors + resource_push_status (PostgreSQL)

-- 推送配置主表
CREATE TABLE IF NOT EXISTS push_configs (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    api_url TEXT NOT NULL DEFAULT '',
    api_token TEXT,
    target TEXT NOT NULL DEFAULT '',
    auth_type TEXT NOT NULL DEFAULT 'custom_header',
    auth_key TEXT NOT NULL DEFAULT 'X-API-Token',
    http_method TEXT NOT NULL DEFAULT 'POST',
    body_template TEXT,
    custom_headers TEXT NOT NULL DEFAULT '[]',
    batch_size BIGINT NOT NULL DEFAULT 1000,
    data_source_type TEXT NOT NULL DEFAULT 'all',
    auto_push BOOLEAN NOT NULL DEFAULT false,
    push_interval BIGINT NOT NULL DEFAULT 30,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 推送配置 ↔ 采集器多对多关联
CREATE TABLE IF NOT EXISTS push_config_collectors (
    push_config_id BIGINT NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE,
    collector_id BIGINT NOT NULL REFERENCES collectors(id) ON DELETE CASCADE,
    PRIMARY KEY (push_config_id, collector_id)
);

-- 资源在每个配置下的独立推送状态
CREATE TABLE IF NOT EXISTS resource_push_status (
    id BIGSERIAL PRIMARY KEY,
    resource_id BIGINT NOT NULL REFERENCES extracted_resources(id) ON DELETE CASCADE,
    push_config_id BIGINT NOT NULL REFERENCES push_configs(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(resource_id, push_config_id)
);

CREATE INDEX IF NOT EXISTS idx_resource_push_status_config ON resource_push_status(push_config_id);
CREATE INDEX IF NOT EXISTS idx_resource_push_status_status ON resource_push_status(status);

-- push_histories 新增关联推送配置 ID
ALTER TABLE push_histories ADD COLUMN push_config_id BIGINT REFERENCES push_configs(id);
