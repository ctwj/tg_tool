use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ForwardTask {
    pub id: i64,
    pub remote_id: String,
    pub channel_id: Option<i64>,
    pub message_id: Option<i64>,
    /// 群组A 中的消息 ID，阶段1（copy_media 转存）成功后写入
    /// NULL 表示阶段1 未完成；用于阶段2 Bot forwardMessage 调用
    pub image_message_id: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub link: Option<String>,
    pub file_id: Option<String>,
    pub status: String,
    pub retry_count: i64,
    pub error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 转发任务 + 关联采集器信息
/// queue_status 通过 channel_id 关联 collectors 带出 collector_id / channel_name，
/// 供前端显示频道名并跳转到采集记录页。独立于 ForwardTask，避免影响 SELECT * 映射。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ForwardTaskWithCollector {
    pub id: i64,
    pub remote_id: String,
    pub channel_id: Option<i64>,
    pub message_id: Option<i64>,
    pub image_message_id: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub link: Option<String>,
    pub file_id: Option<String>,
    pub status: String,
    pub retry_count: i64,
    pub error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub collector_id: Option<i64>,
    pub channel_name: Option<String>,
}
