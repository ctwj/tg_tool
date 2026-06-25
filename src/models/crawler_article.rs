//! CrawlerArticle / ListItem / Detail 模型（feature 042）

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::models::crawler_article_image::CrawlerArticleImage;
use crate::models::crawler_article_link::CrawlerArticleLink;

/// 爬虫文章 — 完整行结构
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerArticle {
    pub id: i64,
    pub task_id: Option<i64>,
    pub source_type: String,
    pub source_url: String,
    pub source_url_canonical: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub is_edited: bool,
    pub crawled_at: NaiveDateTime,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 列表页展示项（含子表聚合 count + 首图）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerArticleListItem {
    pub id: i64,
    pub task_id: Option<i64>,
    pub source_type: String,
    pub title: Option<String>,
    pub category: Option<String>,
    /// 首图（第一张已上传图的 file_id 或 local_path 或 original_url）
    pub thumbnail: Option<String>,
    pub pan_link_count: i64,
    pub direct_link_count: i64,
    pub image_count: i64,
    pub is_edited: bool,
    pub crawled_at: NaiveDateTime,
}

/// 详情（含 links + images 数组）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerArticleDetail {
    #[serde(flatten)]
    pub article: CrawlerArticle,
    pub links: Vec<CrawlerArticleLink>,
    pub images: Vec<CrawlerArticleImage>,
    /// 关联任务名（即使 task_id NULL 也回填快照）
    pub task_name: Option<String>,
}
