-- Migration 003: Create extracted_resources table (PostgreSQL)

CREATE TABLE IF NOT EXISTS extracted_resources (
    id BIGSERIAL PRIMARY KEY,
    collector_history_id BIGINT NOT NULL REFERENCES collector_histories(id),
    title VARCHAR(500) NOT NULL,
    url TEXT,
    description TEXT,
    category VARCHAR(50),
    tags TEXT,
    img TEXT,
    source VARCHAR(20) NOT NULL DEFAULT 'tg',
    extra TEXT,
    extract_mode VARCHAR(10) NOT NULL DEFAULT 'rule',
    is_pushed BOOLEAN NOT NULL DEFAULT FALSE,
    is_edited BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_extracted_resources_collector_history_id ON extracted_resources(collector_history_id);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_is_pushed ON extracted_resources(is_pushed);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_category ON extracted_resources(category);
CREATE INDEX IF NOT EXISTS idx_extracted_resources_created_at ON extracted_resources(created_at DESC);
