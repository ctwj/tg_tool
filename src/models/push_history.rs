use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushHistory {
    pub id: i64,
    pub batch_id: String,
    pub target: Option<String>,
    pub status: String,
    pub data_count: i64,
    pub message: Option<String>,
    pub error_msg: Option<String>,
    pub pushed_at: NaiveDateTime,
    /// 实际推送资源数（迁移 013）
    #[sqlx(default)]
    pub pushed_count: i64,
    /// 图片未转存跳过数（迁移 013）
    #[sqlx(default)]
    pub skipped_image_count: i64,
    /// 链接失效跳过数（迁移 013）
    #[sqlx(default)]
    pub skipped_link_count: i64,
}
