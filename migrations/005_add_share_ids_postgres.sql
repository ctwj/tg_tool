-- Migration 004: Add share_ids column for dedup (PostgreSQL)

ALTER TABLE extracted_resources ADD COLUMN IF NOT EXISTS share_ids TEXT;
