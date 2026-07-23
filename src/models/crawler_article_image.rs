//! CrawlerArticleImage 模型（feature 042）

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 文章图片 — 状态机：pending → downloaded → uploading → uploaded / failed
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerArticleImage {
    pub id: i64,
    pub article_id: i64,
    pub original_url: String,
    pub url_canonical: String,
    pub local_path: Option<String>,
    /// 图床群组上传成功后的消息 ID
    pub image_message_id: Option<i64>,
    pub file_id: Option<String>,
    /// `pending` / `downloaded` / `uploading` / `uploaded` / `failed`
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 创建图片（内部使用：engine 写入）
#[derive(Debug, Clone)]
pub struct NewCrawlerArticleImage {
    pub article_id: i64,
    pub original_url: String,
    pub url_canonical: String,
}
