//! CrawlerTask / CrawlerTaskInput 模型（feature 042）

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::services::crawler::extractor::FieldSelectors;

/// 爬虫任务 — 每条记录代表一个独立的网站爬虫配置
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerTask {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// JSON 数组（字符串形式）— 列表页 URL 列表
    pub list_urls: String,
    /// JSON — 字段选择器配置（解析为 [`FieldSelectors`]）
    pub selectors: String,
    pub two_stage: bool,
    pub interval_minutes: i64,
    pub task_concurrency: i64,
    pub user_agent: Option<String>,
    pub request_delay_ms: i64,
    pub proxy: Option<String>,
    pub auto_link_check: bool,
    /// JSON — 自定义拦截关键词覆盖
    pub block_detection_config: Option<String>,
    pub max_consecutive_failures: i64,
    pub template_source: Option<String>,
    pub status: String,
    pub consecutive_failures: i64,
    pub last_run_at: Option<NaiveDateTime>,
    pub next_run_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 创建/更新任务时的输入 body（不含 id/时间戳/status 计数字段）
///
/// 用于 `POST /api/crawler/tasks`、`PUT /api/crawler/tasks/{id}`、`POST /api/crawler/tasks/import`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerTaskInput {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub list_urls: Vec<String>,
    pub selectors: FieldSelectors,
    #[serde(default = "default_true")]
    pub two_stage: bool,
    #[serde(default = "default_interval")]
    pub interval_minutes: i64,
    #[serde(default = "default_one")]
    pub task_concurrency: i64,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default = "default_delay")]
    pub request_delay_ms: i64,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub auto_link_check: bool,
    #[serde(default)]
    pub block_detection_config: Option<String>,
    #[serde(default = "default_three")]
    pub max_consecutive_failures: i64,
    #[serde(default)]
    pub template_source: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_one() -> i64 {
    1
}
fn default_three() -> i64 {
    3
}
fn default_interval() -> i64 {
    30
}
fn default_delay() -> i64 {
    1000
}

impl CrawlerTaskInput {
    /// 将 list_urls 序列化为 JSON 字符串（DB 存储）
    pub fn list_urls_json(&self) -> String {
        serde_json::to_string(&self.list_urls).unwrap_or_else(|_| "[]".to_string())
    }

    /// 将 selectors 序列化为 JSON 字符串（DB 存储）
    pub fn selectors_json(&self) -> String {
        serde_json::to_string(&self.selectors).unwrap_or_else(|_| "{}".to_string())
    }

    /// 校验输入合法性（返回 first-error）
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("任务名不能为空".into());
        }
        if self.list_urls.is_empty() {
            return Err("list_urls 不能为空".into());
        }
        if self.interval_minutes < 1 {
            return Err("interval_minutes 必须 >= 1".into());
        }
        if self.task_concurrency < 1 {
            return Err("task_concurrency 必须 >= 1".into());
        }
        // 两阶段抓取需要 list_item + detail_link；单阶段需要至少一个字段选择器
        if self.two_stage
            && (self.selectors.list_item.is_empty() || self.selectors.detail_link.is_empty())
        {
            return Err("两阶段抓取必须配置 list_item + detail_link 选择器".into());
        }
        if self.max_consecutive_failures < 1 {
            return Err("max_consecutive_failures 必须 >= 1".into());
        }
        Ok(())
    }
}
