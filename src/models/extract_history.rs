use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 提取历史记录 — 记录每次提取批次执行结果（成功/失败）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExtractHistory {
    pub id: i64,
    pub status: String,
    pub total_scanned: i64,
    pub extracted: i64,
    pub skipped: i64,
    pub errors: i64,
    pub message: Option<String>,
    pub executed_at: NaiveDateTime,
}
