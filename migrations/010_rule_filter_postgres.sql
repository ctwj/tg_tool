-- Migration 010: Add filter + forward_client_id columns to rules (PostgreSQL)

ALTER TABLE rules ADD COLUMN forward_client_id VARCHAR(100);
ALTER TABLE rules ADD COLUMN filter_mode VARCHAR(10) DEFAULT 'none';
ALTER TABLE rules ADD COLUMN keywords TEXT;
ALTER TABLE rules ADD COLUMN media_filter VARCHAR(20) DEFAULT 'all';
