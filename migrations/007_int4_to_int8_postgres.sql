-- Convert all SERIAL/INTEGER id and foreign key columns to BIGINT
-- to match Rust models which use i64

-- users
ALTER TABLE users ALTER COLUMN id TYPE BIGINT;

-- clients
ALTER TABLE clients ALTER COLUMN user_id TYPE BIGINT;

-- rules
ALTER TABLE rules ALTER COLUMN id TYPE BIGINT;
ALTER TABLE rules ALTER COLUMN user_id TYPE BIGINT;

-- collectors
ALTER TABLE collectors ALTER COLUMN id TYPE BIGINT;
ALTER TABLE collectors ALTER COLUMN user_id TYPE BIGINT;

-- messages
ALTER TABLE messages ALTER COLUMN id TYPE BIGINT;
ALTER TABLE messages ALTER COLUMN rule_id TYPE BIGINT;

-- collector_histories
ALTER TABLE collector_histories ALTER COLUMN id TYPE BIGINT;
ALTER TABLE collector_histories ALTER COLUMN collector_id TYPE BIGINT;

-- push_histories
ALTER TABLE push_histories ALTER COLUMN id TYPE BIGINT;

-- options
ALTER TABLE options ALTER COLUMN id TYPE BIGINT;

-- files
ALTER TABLE files ALTER COLUMN id TYPE BIGINT;
ALTER TABLE files ALTER COLUMN uploader_id TYPE BIGINT;

-- extracted_resources
ALTER TABLE extracted_resources ALTER COLUMN id TYPE BIGINT;
ALTER TABLE extracted_resources ALTER COLUMN collector_history_id TYPE BIGINT;
