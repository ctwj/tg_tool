-- Migration 008: Image mappings + forward tasks tables (PostgreSQL)

-- remote_id -> file_id mapping, created after sendPhoto succeeds
CREATE TABLE IF NOT EXISTS image_mappings (
    remote_id VARCHAR(255) PRIMARY KEY,
    file_id VARCHAR(512) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Forward task queue
CREATE TABLE IF NOT EXISTS forward_tasks (
    id BIGSERIAL PRIMARY KEY,
    remote_id VARCHAR(255) NOT NULL,
    channel_id BIGINT,
    message_id BIGINT,
    title TEXT,
    description TEXT,
    link TEXT,
    file_id VARCHAR(512),
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    retry_count BIGINT NOT NULL DEFAULT 0,
    error TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Index for pending task queries
CREATE INDEX IF NOT EXISTS idx_forward_tasks_status ON forward_tasks(status);
