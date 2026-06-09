-- Migration 009: Create extract_histories table (PostgreSQL)

CREATE TABLE IF NOT EXISTS extract_histories (
    id BIGSERIAL PRIMARY KEY,
    status VARCHAR(20) NOT NULL,
    total_scanned BIGINT NOT NULL DEFAULT 0,
    extracted BIGINT NOT NULL DEFAULT 0,
    skipped BIGINT NOT NULL DEFAULT 0,
    errors BIGINT NOT NULL DEFAULT 0,
    message TEXT,
    executed_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_extract_histories_executed_at ON extract_histories(executed_at DESC);
CREATE INDEX IF NOT EXISTS idx_extract_histories_status ON extract_histories(status);
