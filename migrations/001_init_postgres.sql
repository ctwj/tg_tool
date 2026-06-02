-- PostgreSQL Migration: Initial schema

-- Users table
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    password VARCHAR(255) NOT NULL,
    display_name VARCHAR(255),
    email VARCHAR(255) UNIQUE,
    role INTEGER NOT NULL DEFAULT 1,
    status INTEGER NOT NULL DEFAULT 1,
    access_token VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Telegram clients
CREATE TABLE IF NOT EXISTS clients (
    id VARCHAR(16) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    client_type VARCHAR(20) NOT NULL,
    phone VARCHAR(255),
    token VARCHAR(255),
    status VARCHAR(20) NOT NULL DEFAULT 'new',
    session_path VARCHAR(512),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Forward rules
CREATE TABLE IF NOT EXISTS rules (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    source_chat_id BIGINT NOT NULL,
    source_chat_name VARCHAR(255),
    forward_method VARCHAR(20) NOT NULL,
    forward_config TEXT,
    forward_target VARCHAR(255),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    remark TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Collectors
CREATE TABLE IF NOT EXISTS collectors (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    channel_id BIGINT NOT NULL,
    channel_name VARCHAR(255),
    collector_type VARCHAR(50) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    remark TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Forward messages
CREATE TABLE IF NOT EXISTS messages (
    id SERIAL PRIMARY KEY,
    rule_id INTEGER NOT NULL REFERENCES rules(id),
    chat_id BIGINT,
    message_id BIGINT,
    content TEXT,
    raw_data TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    error_reason TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Collector histories
CREATE TABLE IF NOT EXISTS collector_histories (
    id SERIAL PRIMARY KEY,
    collector_id INTEGER NOT NULL REFERENCES collectors(id),
    channel_id BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    post_time TIMESTAMP,
    raw_data TEXT,
    is_auto_push BOOLEAN DEFAULT FALSE,
    remote_id VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(channel_id, message_id)
);

-- Push histories
CREATE TABLE IF NOT EXISTS push_histories (
    id SERIAL PRIMARY KEY,
    batch_id VARCHAR(255) NOT NULL,
    target VARCHAR(100),
    status VARCHAR(20) NOT NULL,
    data_count INTEGER DEFAULT 0,
    message TEXT,
    error_msg TEXT,
    pushed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- System options
CREATE TABLE IF NOT EXISTS options (
    id SERIAL PRIMARY KEY,
    key VARCHAR(255) NOT NULL UNIQUE,
    value TEXT
);

-- Uploaded files
CREATE TABLE IF NOT EXISTS files (
    id SERIAL PRIMARY KEY,
    filename VARCHAR(255) NOT NULL,
    uploader_id INTEGER NOT NULL REFERENCES users(id),
    link VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_clients_user_id ON clients(user_id);
CREATE INDEX IF NOT EXISTS idx_rules_user_id ON rules(user_id);
CREATE INDEX IF NOT EXISTS idx_rules_source_chat_id ON rules(source_chat_id);
CREATE INDEX IF NOT EXISTS idx_collectors_user_id ON collectors(user_id);
CREATE INDEX IF NOT EXISTS idx_collectors_channel_id ON collectors(channel_id);
CREATE INDEX IF NOT EXISTS idx_messages_rule_id ON messages(rule_id);
CREATE INDEX IF NOT EXISTS idx_collector_histories_collector_id ON collector_histories(collector_id);
CREATE INDEX IF NOT EXISTS idx_push_histories_batch_id ON push_histories(batch_id);
CREATE INDEX IF NOT EXISTS idx_options_key ON options(key);
