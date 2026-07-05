//! refresh — feature 046-crawler-script-extractor (US4)
//!
//! 已采集数据脚本结果刷新（lazy refresh on read）。
//!
//! 双层控制（FR-019）：
//! - 字段级 `refresh_on_read`（默认 false）：决定该字段"默认"在消费性读取时是否刷新
//! - 调用方 `force_refresh: Option<bool>`：临时覆盖字段默认行为
//!   - `None` → 走字段默认
//!   - `Some(true)` → 强制刷新
//!   - `Some(false)` → 强制不刷新
//!
//! 管理性读取（列表/详情/字段命中率面板）**不**调用本模块，直接读库。
//! 消费性读取（未来下载/上传/推送）调用 [`get_article_field_for_use`]。
//! 手动刷新 API（FR-023）调 [`get_article_field_for_use`] 并传 `Some(true)`。
//!
//! 失败语义（FR-021）：hard fail — 不覆盖旧值，返回错误。

use std::collections::HashMap;

use crate::models::crawler_field_node::FieldNodeRow;
use crate::services::crawler::field_schema::{Rule, ScriptRule};
use crate::services::crawler::script_engine::ScriptError;
use crate::services::crawler::script_runner::{self, ScriptOpts};
use crate::state::DbPool;

/// 决策函数（FR-019 决策矩阵）：是否应刷新？
///
/// | refresh_on_read | force_refresh | 行为 |
/// |-----------------|---------------|------|
/// | true            | None          | 是   |
/// | true            | Some(true)    | 是   |
/// | true            | Some(false)   | 否   |
/// | false           | None          | 否   |
/// | false           | Some(true)    | 是   |
/// | false           | Some(false)   | 否   |
pub fn should_refresh(refresh_on_read: bool, force_refresh: Option<bool>) -> bool {
    force_refresh.unwrap_or(refresh_on_read)
}

/// 刷新失败原因
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("文章 {article_id} 不存在")]
    ArticleNotFound { article_id: i64 },
    #[error("文章 {article_id} 未关联 task，无法刷新")]
    ArticleNoTask { article_id: i64 },
    #[error("字段 {field_name} 在文章 {article_id} 关联的任务字段树中不存在")]
    FieldNodeNotFound { article_id: i64, field_name: String },
    #[error("字段 {field_name} 不是 script 模式（mode={mode}），不可刷新")]
    NotScriptField { field_name: String, mode: String },
    #[error("脚本求值失败 [{category}]: {message}")]
    ScriptFailed {
        category: String,
        message: String,
    },
    #[error("数据库错：{0}")]
    Database(String),
    #[error("Tokio runtime 错：{0}")]
    Runtime(String),
}

/// 刷新结果（成功时返回）
#[derive(Debug, Clone)]
pub struct RefreshedValue {
    /// 旧值（刷新前库内值；首次刷新且库内无值时为空串）
    pub old_value: String,
    /// 新值（脚本求值结果）
    pub new_value: String,
    /// 刷新耗时（毫秒）
    pub duration_ms: u64,
}

/// 加载字段节点配置（按 task_id + name + scope=detail_page 唯一）
///
/// 返回 (node_id, refresh_on_read, script body)。失败：
/// - 行不存在 → None
/// - 行 spec 解析失败 → Database error
async fn load_script_field_node(
    db: &DbPool,
    task_id: i64,
    field_name: &str,
) -> Result<Option<(i64, bool, String)>, RefreshError> {
    let row: Option<FieldNodeRow> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, FieldNodeRow>(
            "SELECT * FROM crawler_task_field_nodes WHERE task_id = ? AND name = ? AND scope = 'detail_page'",
        )
        .bind(task_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, FieldNodeRow>(
            "SELECT * FROM crawler_task_field_nodes WHERE task_id = $1 AND name = $2 AND scope = 'detail_page'",
        )
        .bind(task_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
    };
    let Some(row) = row else { return Ok(None) };
    let spec = row
        .to_spec()
        .map_err(|e| RefreshError::Database(format!("字段 spec 解析失败: {e}")))?;
    let body = match &spec.rule {
        Rule::Script(ScriptRule { body, .. }) => body.clone(),
        _ => {
            return Ok(Some((
                row.id,
                spec.refresh_on_read,
                format!("__not_script__:{}", spec.extractor_mode.as_str()),
            )));
        }
    };
    Ok(Some((row.id, spec.refresh_on_read, body)))
}

/// 加载 article 当前 final_value（取最新一行 value_text）
async fn load_current_value(
    db: &DbPool,
    article_id: i64,
    field_node_id: i64,
) -> Result<Option<String>, RefreshError> {
    let value: Option<(Option<String>,)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, (Option<String>,)>(
                "SELECT value_text FROM crawler_article_field_values \
                 WHERE article_id = ? AND field_node_id = ? AND is_hit = 1 \
                 ORDER BY value_index ASC LIMIT 1",
            )
            .bind(article_id)
            .bind(field_node_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, (Option<String>,)>(
                "SELECT value_text FROM crawler_article_field_values \
                 WHERE article_id = $1 AND field_node_id = $2 AND is_hit = true \
                 ORDER BY value_index ASC LIMIT 1",
            )
            .bind(article_id)
            .bind(field_node_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?
        }
    };
    Ok(value.and_then(|(v,)| v))
}

/// 加载同 article 同 scope=detail_page 的兄弟字段最新值（用于 ctx_fields）
///
/// 仅取每个 field_path 首条命中；排除 field_node_id 对应的字段本身。
async fn load_sibling_values(
    db: &DbPool,
    article_id: i64,
    exclude_field_node_id: i64,
) -> Result<HashMap<String, String>, RefreshError> {
    let rows: Vec<(String, Option<String>)> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT field_path, value_text FROM crawler_article_field_values \
                 WHERE article_id = ? AND field_node_id != ? AND is_hit = 1 \
                 AND scope = 'detail_page' AND value_index = 0",
            )
            .bind(article_id)
            .bind(exclude_field_node_id)
            .fetch_all(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT field_path, value_text FROM crawler_article_field_values \
                 WHERE article_id = $1 AND field_node_id != $2 AND is_hit = true \
                 AND scope = 'detail_page' AND value_index = 0",
            )
            .bind(article_id)
            .bind(exclude_field_node_id)
            .fetch_all(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?
        }
    };
    let mut map = HashMap::new();
    for (path, value) in rows {
        let Some(value) = value else { continue };
        // field_path 物化路径 /detail_page/<name>[/...]，取末段 name 作为 key
        let name = path.rsplit('/').next().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        map.insert(name.to_string(), value);
    }
    Ok(map)
}

/// 加载 article → task_id
async fn load_article_task_id(db: &DbPool, article_id: i64) -> Result<i64, RefreshError> {
    let row: Option<(i64,)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (i64,)>(
            "SELECT task_id FROM crawler_articles WHERE id = ?",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (i64,)>(
            "SELECT task_id FROM crawler_articles WHERE id = $1",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
    };
    row.map(|(t,)| t).ok_or(RefreshError::ArticleNotFound { article_id })
}

/// 加载 article url（详情页 URL）+ task user_agent + proxy 用于脚本求值
async fn load_article_url_and_task_proxy(
    db: &DbPool,
    article_id: i64,
) -> Result<(String, Option<String>, Option<String>), RefreshError> {
    let row: Option<(String, Option<String>, Option<String>)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT a.url, t.user_agent, t.proxy \
             FROM crawler_articles a JOIN crawler_tasks t ON a.task_id = t.id \
             WHERE a.id = ?",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT a.url, t.user_agent, t.proxy \
             FROM crawler_articles a JOIN crawler_tasks t ON a.task_id = t.id \
             WHERE a.id = $1",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
    };
    let Some((url, ua, proxy)) = row else {
        return Err(RefreshError::ArticleNotFound { article_id });
    };
    Ok((url, ua, proxy))
}

/// 主入口：消费性读取 + 按需刷新（FR-019 / FR-020 / FR-021）。
///
/// **流程**：
/// 1. 加载 article → task_id
/// 2. 加载字段节点配置（refresh_on_read + script body）；非 script 字段拒绝
/// 3. 加载当前 final_value
/// 4. `should_refresh` 判定；false → 直接返回 old_value
/// 5. 加载兄弟字段最新值构造 ctx_fields
/// 6. 加载 article url + task user_agent/proxy 构造 reqwest client
/// 7. 调 `script_runner::run_script` 重跑
/// 8. 成功：UPDATE final_value（写新行 is_hit=1）+ 返回 RefreshedValue
/// 9. 失败：hard fail（不 UPDATE）→ Err(ScriptFailed)
pub async fn get_article_field_for_use(
    article_id: i64,
    field_name: &str,
    force_refresh: Option<bool>,
    db: &DbPool,
    http_client: Option<&reqwest::Client>,
) -> Result<RefreshedValue, RefreshError> {
    let started = std::time::Instant::now();

    // 1. article → task_id
    let task_id = load_article_task_id(db, article_id).await?;

    // 2. 字段节点配置
    let node_info = load_script_field_node(db, task_id, field_name).await?;
    let Some((field_node_id, refresh_on_read, body_or_marker)) = node_info else {
        return Err(RefreshError::FieldNodeNotFound {
            article_id,
            field_name: field_name.to_string(),
        });
    };
    if body_or_marker.starts_with("__not_script__:") {
        let mode = body_or_marker.trim_start_matches("__not_script__:");
        return Err(RefreshError::NotScriptField {
            field_name: field_name.to_string(),
            mode: mode.to_string(),
        });
    }
    let script_body = body_or_marker;

    // 3. 当前 final_value
    let old_value = load_current_value(db, article_id, field_node_id)
        .await?
        .unwrap_or_default();

    // 4. 决策
    if !should_refresh(refresh_on_read, force_refresh) {
        let duration_ms = started.elapsed().as_millis() as u64;
        return Ok(RefreshedValue {
            old_value: old_value.clone(),
            new_value: old_value,
            duration_ms,
        });
    }

    // 5. 兄弟字段值
    let ctx_fields = load_sibling_values(db, article_id, field_node_id).await?;

    // 6. article url + task proxy/UA（用于构造 client，仅在调用方未传 client 时）
    let (url, _ua, _proxy) = load_article_url_and_task_proxy(db, article_id).await?;

    // 7. 求值
    let rule = ScriptRule {
        body: script_body,
        api_version: "v1".into(),
    };
    let opts = ScriptOpts::default();
    let new_value = script_runner::run_script(
        &rule,
        String::new(),
        ctx_fields,
        &url,
        http_client,
        &opts,
    )
    .await
    .map_err(|ScriptError { category, message, .. }| RefreshError::ScriptFailed {
        category: category.as_str().to_string(),
        message,
    })?;

    let duration_ms = started.elapsed().as_millis() as u64;

    // [feature 046 FR-016] 结构化日志：force_refresh 来源 + 决策结果
    tracing::trace!(
        target: "crawler",
        article_id = article_id,
        field_name = %field_name,
        refresh_on_read = refresh_on_read,
        force_refresh = ?force_refresh,
        duration_ms = duration_ms,
        new_value_preview = %new_value.chars().take(100).collect::<String>(),
        "script field refreshed (force_source={})",
        if force_refresh.is_some() { "explicit" } else { "lazy" }
    );

    // 8. 写新值（先删旧命中行，再插新行；保持 value_index=0 单值语义）
    let now = chrono::Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "DELETE FROM crawler_article_field_values \
                 WHERE article_id = ? AND field_node_id = ? AND is_hit = 1",
            )
            .bind(article_id)
            .bind(field_node_id)
            .execute(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?;
            sqlx::query(
                "INSERT INTO crawler_article_field_values \
                 (article_id, field_node_id, field_path, scope, value_index, value_text, \
                  value_number, is_hit, created_at) \
                 VALUES (?, ?, ?, 'detail_page', 0, ?, NULL, 1, ?)",
            )
            .bind(article_id)
            .bind(field_node_id)
            .bind(format!("/detail_page/{}", field_name))
            .bind(&new_value)
            .bind(now)
            .execute(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "DELETE FROM crawler_article_field_values \
                 WHERE article_id = $1 AND field_node_id = $2 AND is_hit = true",
            )
            .bind(article_id)
            .bind(field_node_id)
            .execute(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?;
            sqlx::query(
                "INSERT INTO crawler_article_field_values \
                 (article_id, field_node_id, field_path, scope, value_index, value_text, \
                  value_number, is_hit, created_at) \
                 VALUES ($1, $2, $3, 'detail_page', 0, $4, NULL, true, $5)",
            )
            .bind(article_id)
            .bind(field_node_id)
            .bind(format!("/detail_page/{}", field_name))
            .bind(&new_value)
            .bind(now)
            .execute(pool)
            .await
            .map_err(|e| RefreshError::Database(e.to_string()))?;
        }
    }

    Ok(RefreshedValue {
        old_value,
        new_value,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_should_refresh_truth_table() {
        // (refresh_on_read, force_refresh, expected)
        let cases: [(bool, Option<bool>, bool); 6] = [
            (true, None, true),
            (true, Some(true), true),
            (true, Some(false), false),
            (false, None, false),
            (false, Some(true), true),
            (false, Some(false), false),
        ];
        for (ror, fr, expected) in cases {
            assert_eq!(
                should_refresh(ror, fr),
                expected,
                "refresh_on_read={}, force_refresh={:?} → expected {}",
                ror,
                fr,
                expected
            );
        }
    }
}
