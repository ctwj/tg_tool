-- Migration 002: Add client_id to collectors table

-- PostgreSQL
ALTER TABLE collectors ADD COLUMN client_id VARCHAR(16);
