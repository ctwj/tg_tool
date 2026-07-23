//! CrawlerArticleLink 模型（feature 042）

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 文章链接 — 同时存放网盘链接（link_type=pan）和直链（link_type=direct）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerArticleLink {
    pub id: i64,
    pub article_id: i64,
    /// `pan` | `direct`
    pub link_type: String,
    /// 网盘品牌（9 平台之一）；直链为 NULL
    pub platform: Option<String>,
    pub url: String,
    pub url_canonical: String,
    pub extract_code: Option<String>,
    /// `valid` / `invalid` / `pending` / `unknown`
    pub validity_status: String,
    pub validity_reason: Option<String>,
    pub last_checked_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 创建链接（内部使用：engine 写入）
#[derive(Debug, Clone)]
pub struct NewCrawlerArticleLink {
    pub article_id: i64,
    pub link_type: String,
    pub platform: Option<String>,
    pub url: String,
    pub url_canonical: String,
    pub extract_code: Option<String>,
}
