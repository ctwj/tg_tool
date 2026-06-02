-- Migration 004: Add is_extracted flag to collector_histories (PostgreSQL)

ALTER TABLE collector_histories ADD COLUMN IF NOT EXISTS is_extracted BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_collector_histories_is_extracted ON collector_histories(is_extracted);
