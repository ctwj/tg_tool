//! CrawlerRunHistory / CrawlerHistoryStats 模型（feature 042）

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 任务单次运行历史
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerRunHistory {
    pub id: i64,
    pub task_id: i64,
    pub task_name: String,
    pub started_at: NaiveDateTime,
    pub finished_at: Option<NaiveDateTime>,
    pub duration_ms: Option<i64>,
    /// `success` / `partial` / `failed` / `blocked`
    pub status: String,
    /// 拦截类型字符串（`BlockType::as_str()`），仅 status=blocked 时有值
    pub block_type: Option<String>,
    pub crawled_count: i64,
    pub new_count: i64,
    pub skipped_count: i64,
    pub failed_count: i64,
    pub error_message: Option<String>,
    pub created_at: NaiveDateTime,
}

/// 历史详情（含被拦截的原始响应摘要）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerRunHistoryDetail {
    #[serde(flatten)]
    pub history: CrawlerRunHistory,
    /// 拦截时的原始响应片段（前 500 字符），仅 status=blocked 时有值
    pub blocked_response_excerpt: Option<String>,
}

/// `/histories/stats` 聚合统计（仪表盘告警用）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrawlerHistoryStats {
    pub total_runs: i64,
    pub success: i64,
    pub partial: i64,
    pub failed: i64,
    pub blocked: i64,
    /// 拦截类型聚合：`{ "Cloudflare": 3, "HttpBlocked_403": 5, ... }`
    pub block_breakdown: HashMap<String, i64>,
    pub last_run_at: Option<NaiveDateTime>,
    /// 当前 status=auto_blocked 的任务数
    pub auto_blocked_tasks: i64,
}
