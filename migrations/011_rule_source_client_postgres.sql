-- Migration 011: Add source_client_id to rules (PostgreSQL)

ALTER TABLE rules ADD COLUMN source_client_id VARCHAR(100);
