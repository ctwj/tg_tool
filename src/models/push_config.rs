use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 推送配置 — 每条记录代表一个独立的推送目标(API + 数据源)
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushConfig {
    pub id: i64,
    pub name: String,
    pub api_url: String,
    pub api_token: Option<String>,
    pub target: String,
    pub auth_type: String,
    pub auth_key: String,
    pub http_method: String,
    pub body_template: Option<String>,
    pub custom_headers: String,
    pub batch_size: i64,
    pub data_source_type: String, // "all" | "selected"
    pub auto_push: bool,
    pub push_interval: i64,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 推送配置列表项 — 含关联采集器数量
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushConfigWithCollectorCount {
    pub id: i64,
    pub name: String,
    pub api_url: String,
    pub api_token: Option<String>,
    pub target: String,
    pub auth_type: String,
    pub auth_key: String,
    pub http_method: String,
    pub body_template: Option<String>,
    pub custom_headers: String,
    pub batch_size: i64,
    pub data_source_type: String,
    pub auto_push: bool,
    pub push_interval: i64,
    pub is_active: bool,
    pub collector_count: i64,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 资源在每个配置下的推送状态
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResourcePushStatus {
    pub id: i64,
    pub resource_id: i64,
    pub push_config_id: i64,
    pub status: String, // "pending" | "pushed" | "failed"
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
