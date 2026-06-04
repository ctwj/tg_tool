-- Migration 004: Add share_ids column for dedup (SQLite)

ALTER TABLE extracted_resources ADD COLUMN share_ids TEXT;
