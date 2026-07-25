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
    ScriptFailed { category: String, message: String },
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
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (Option<String>,)>(
            "SELECT value_text FROM crawler_article_field_values \
                 WHERE article_id = ? AND field_node_id = ? AND is_hit = 1 \
                 ORDER BY value_index ASC LIMIT 1",
        )
        .bind(article_id)
        .bind(field_node_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (Option<String>,)>(
            "SELECT value_text FROM crawler_article_field_values \
                 WHERE article_id = $1 AND field_node_id = $2 AND is_hit = true \
                 ORDER BY value_index ASC LIMIT 1",
        )
        .bind(article_id)
        .bind(field_node_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
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
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT field_path, value_text FROM crawler_article_field_values \
                 WHERE article_id = ? AND field_node_id != ? AND is_hit = 1 \
                 AND scope = 'detail_page' AND value_index = 0",
        )
        .bind(article_id)
        .bind(exclude_field_node_id)
        .fetch_all(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT field_path, value_text FROM crawler_article_field_values \
                 WHERE article_id = $1 AND field_node_id != $2 AND is_hit = true \
                 AND scope = 'detail_page' AND value_index = 0",
        )
        .bind(article_id)
        .bind(exclude_field_node_id)
        .fetch_all(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
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
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, (i64,)>("SELECT task_id FROM crawler_articles WHERE id = ?")
                .bind(article_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| RefreshError::Database(e.to_string()))?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, (i64,)>("SELECT task_id FROM crawler_articles WHERE id = $1")
                .bind(article_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| RefreshError::Database(e.to_string()))?
        }
    };
    row.map(|(t,)| t)
        .ok_or(RefreshError::ArticleNotFound { article_id })
}

/// 加载 article url（详情页 URL）+ task user_agent + proxy 用于脚本求值
async fn load_article_url_and_task_proxy(
    db: &DbPool,
    article_id: i64,
) -> Result<(String, Option<String>, Option<String>), RefreshError> {
    let row: Option<(String, Option<String>, Option<String>)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT a.source_url, t.user_agent, t.proxy \
             FROM crawler_articles a JOIN crawler_tasks t ON a.task_id = t.id \
             WHERE a.id = ?",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT a.source_url, t.user_agent, t.proxy \
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
    let new_value =
        script_runner::run_script(&rule, String::new(), ctx_fields, &url, http_client, &opts)
            .await
            .map_err(
                |ScriptError {
                     category, message, ..
                 }| RefreshError::ScriptFailed {
                    category: category.as_str().to_string(),
                    message,
                },
            )?;

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

// ============================================================================
// 沙盒试跑（不写库） — feature 046 增强
// ============================================================================

/// 沙盒试跑结果（不写库）
#[derive(Debug, Clone, serde::Serialize)]
pub struct SandboxResult {
    /// 求值得到的新值（失败时为 None）
    pub value: Option<String>,
    /// 失败信息（category + message）
    pub error: Option<SandboxErrorInfo>,
    /// 当前 DB 内该字段已有值（按 field_name 在 article_field_values 中查 detail_page 作用域）
    pub current_db_value: Option<String>,
    /// ctx 快照（debug 用，让用户看到注入的兄弟字段是否符合预期）
    pub ctx: SandboxCtx,
    /// 求值耗时（毫秒）
    pub duration_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SandboxErrorInfo {
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SandboxCtx {
    pub url: String,
    /// 兄弟字段（已排除 field_name 自身；script 字段之间不互相暴露，沿用 build_sibling_ctx_fields 语义）
    pub fields: HashMap<String, String>,
}

/// 按 (article_id, field_name) 查 detail_page 作用域字段节点 id
///
/// 用于沙盒场景从兄弟字段中排除"自身"。未保存字段（行不存在）返回 Ok(None)。
async fn load_field_node_id_by_name(
    db: &DbPool,
    article_id: i64,
    field_name: &str,
) -> Result<Option<i64>, RefreshError> {
    // article → task_id（用现成 helper，省一次 JOIN）
    let task_id = load_article_task_id(db, article_id).await?;
    let row: Option<(i64,)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM crawler_task_field_nodes \
             WHERE task_id = ? AND name = ? AND scope = 'detail_page'",
        )
        .bind(task_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM crawler_task_field_nodes \
             WHERE task_id = $1 AND name = $2 AND scope = 'detail_page'",
        )
        .bind(task_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
    };
    Ok(row.map(|(id,)| id))
}

/// 按 (article_id, field_name) 查当前已存的字段值
///
/// 用于沙盒响应里展示新旧对比。失败静默（返回 Ok(None)），不影响主流程。
async fn load_current_value_by_name(
    db: &DbPool,
    article_id: i64,
    field_name: &str,
) -> Result<Option<String>, RefreshError> {
    let value: Option<(Option<String>,)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as::<_, (Option<String>,)>(
            "SELECT v.value_text FROM crawler_article_field_values v \
             JOIN crawler_task_field_nodes n ON v.field_node_id = n.id \
             WHERE v.article_id = ? AND n.name = ? AND n.scope = 'detail_page' \
             AND v.is_hit = 1 ORDER BY v.value_index ASC LIMIT 1",
        )
        .bind(article_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
        DbPool::Postgres(pool) => sqlx::query_as::<_, (Option<String>,)>(
            "SELECT v.value_text FROM crawler_article_field_values v \
             JOIN crawler_task_field_nodes n ON v.field_node_id = n.id \
             WHERE v.article_id = $1 AND n.name = $2 AND n.scope = 'detail_page' \
             AND v.is_hit = true ORDER BY v.value_index ASC LIMIT 1",
        )
        .bind(article_id)
        .bind(field_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| RefreshError::Database(e.to_string()))?,
    };
    Ok(value.and_then(|(v,)| v))
}

/// 沙盒试跑入口：用任意脚本 body 在已采集文章上求值，**不写库**。
///
/// 与 [`get_article_field_for_use`] 的区别：
/// - 不读 `crawler_task_field_nodes` 中的 `script body`（用调用方传入的 `script_body`）
/// - 不调用 `should_refresh` 决策（沙盒永远跑）
/// - 不 DELETE/INSERT `crawler_article_field_values`（不污染数据）
/// - 失败也返回 `Ok(SandboxResult)`，错误在 `error` 字段中（业务可观察）
///
/// 步骤：
/// 1. `load_article_task_id` 校验 article 存在
/// 2. （可选）按 `field_name` 查 `field_node_id`，用于从兄弟字段中排除自身
/// 3. `load_sibling_values` → ctx_fields
/// 4. `load_article_url_and_task_proxy` → url + UA + proxy
/// 5. `engine::build_reqwest_client` 构造 reqwest client（best-effort）
/// 6. `script_runner::run_script` 求值
/// 7. （可选）查 DB 当前值用于对比
pub async fn run_script_sandbox(
    article_id: i64,
    field_name: Option<&str>,
    script_body: &str,
    db: &DbPool,
) -> Result<SandboxResult, RefreshError> {
    let started = std::time::Instant::now();

    // 1. article 存在校验
    let _task_id = load_article_task_id(db, article_id).await?;

    // 2. 排除自身的 field_node_id（查不到则 -1，表示不排除）
    let exclude_node_id: i64 = if let Some(name) = field_name {
        load_field_node_id_by_name(db, article_id, name)
            .await?
            .unwrap_or(-1)
    } else {
        -1
    };

    // 3. 兄弟字段
    let fields = load_sibling_values(db, article_id, exclude_node_id).await?;

    // 4. URL + UA + proxy
    let (url, ua, proxy) = load_article_url_and_task_proxy(db, article_id).await?;

    // 5. reqwest client（best-effort，失败仅降级 ctx.fetch）
    let client =
        crate::services::crawler::engine::build_reqwest_client(ua.as_deref(), proxy.as_deref())
            .ok();

    // 6. 求值
    let rule = ScriptRule {
        body: script_body.to_string(),
        api_version: "v1".into(),
    };
    let opts = ScriptOpts::default();
    let (value, error) = match script_runner::run_script(
        &rule,
        String::new(),
        fields.clone(),
        &url,
        client.as_ref(),
        &opts,
    )
    .await
    {
        Ok(v) => (Some(v), None),
        Err(ScriptError {
            category, message, ..
        }) => (
            None,
            Some(SandboxErrorInfo {
                category: category.as_str().to_string(),
                message,
            }),
        ),
    };

    // 7. DB 当前值（可选，用于前端展示新旧对比）
    let current_db_value = if let Some(name) = field_name {
        load_current_value_by_name(db, article_id, name)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let duration_ms = started.elapsed().as_millis() as u64;

    tracing::trace!(
        target: "crawler",
        article_id = article_id,
        field_name = ?field_name,
        duration_ms = duration_ms,
        ok = value.is_some(),
        "script sandbox executed (no DB write)"
    );

    Ok(SandboxResult {
        value,
        error,
        current_db_value,
        ctx: SandboxCtx { url, fields },
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
