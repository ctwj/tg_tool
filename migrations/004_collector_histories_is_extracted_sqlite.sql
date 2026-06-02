-- Migration 004: Add is_extracted flag to collector_histories (SQLite)

ALTER TABLE collector_histories ADD COLUMN is_extracted BOOLEAN NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_collector_histories_is_extracted ON collector_histories(is_extracted);
