//! Crawler handlers — Phase 3 US1 (T025-T027) + Phase 4 US2 (T035-T039)
//!
//! 涵盖任务 CRUD + run/test + 模板/导入导出；以及文章列表/详情/编辑/删除/
//! 图片重试/链接检测。Histories / Stats 端点属于后续 Phase（US3）。

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors::AppError;
use crate::models::crawler_article::{CrawlerArticle, CrawlerArticleDetail, CrawlerArticleListItem};
use crate::models::crawler_article_image::CrawlerArticleImage;
use crate::models::crawler_article_link::CrawlerArticleLink;
use crate::models::crawler_run_history::{
    CrawlerHistoryStats, CrawlerRunHistory, CrawlerRunHistoryDetail,
};
use crate::models::crawler_task::{CrawlerTask, CrawlerTaskInput};
use crate::services::crawler::templates::builtin_templates;
use crate::state::{AppState, DbPool};

/// 任务列表查询参数
#[derive(Deserialize)]
pub struct TaskListParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub enabled: Option<bool>,
    pub keyword: Option<String>,
}

/// `GET /api/crawler/tasks` — 任务列表
pub async fn list_tasks(
    State(state): State<AppState>,
    Query(params): Query<TaskListParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * page_size;

    let keyword = params.keyword.as_deref().unwrap_or("").trim();
    let status = params
        .status
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all")
        .map(String::from);
    let enabled_flag = params.enabled;

    let where_sql = build_where(keyword, status.as_deref(), enabled_flag, false);

    let total: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let sql = format!("SELECT COUNT(*) FROM crawler_tasks {where}", where = where_sql.sqlite);
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let sql = format!("SELECT COUNT(*) FROM crawler_tasks {where}", where = where_sql.postgres);
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            q.fetch_one(pool).await?
        }
    };

    let select_cols = "id, name, enabled, list_urls, selectors, two_stage, \
         interval_minutes, task_concurrency, user_agent, request_delay_ms, \
         proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
         template_source, status, consecutive_failures, last_run_at, next_run_at, \
         created_at, updated_at";

    let rows: Vec<CrawlerTask> = match &state.db {
        DbPool::Sqlite(pool) => {
            let sql = format!(
                "SELECT {select_cols} FROM crawler_tasks {where} \
                 ORDER BY id DESC LIMIT ? OFFSET ?",
                where = where_sql.sqlite
            );
            let mut q = sqlx::query_as::<_, CrawlerTask>(&sql);
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            q.bind(page_size).bind(offset).fetch_all(pool).await?
        }
        DbPool::Postgres(pool) => {
            // Postgres：WHERE 用 $1..$N，LIMIT/OFFSET 接续编号
            let n_filters = pg_filter_count(keyword, status.as_deref(), enabled_flag);
            let limit_n = n_filters + 1;
            let offset_n = n_filters + 2;
            let where_clause = where_sql.postgres;
            let sql = format!(
                "SELECT {select_cols} FROM crawler_tasks {where_clause} \
                 ORDER BY id DESC LIMIT ${limit_n} OFFSET ${offset_n}"
            );
            let mut q = sqlx::query_as::<_, CrawlerTask>(&sql);
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            q.bind(page_size).bind(offset).fetch_all(pool).await?
        }
    };

    Ok(Json(json!({
        "success": true,
        "data": {
            "list": rows,
            "pagination": { "page": page, "page_size": page_size, "total": total }
        }
    })))
}

/// `POST /api/crawler/tasks` — 新建任务
pub async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CrawlerTaskInput>,
) -> Result<Json<Value>, AppError> {
    body.validate().map_err(AppError::BadRequest)?;
    let now = chrono::Utc::now().naive_utc();

    let list_urls_json = body.list_urls_json();
    let selectors_json = body.selectors_json();
    // enabled=true 立即可调度（next_run_at=now()）
    let next_run_at = if body.enabled { Some(now) } else { None };

    let id: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, status, consecutive_failures, next_run_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 0, ?)",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(body.two_stage)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(next_run_at)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &body.name))?;
            r.last_insert_rowid()
        }
        DbPool::Postgres(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, status, consecutive_failures, next_run_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'active', 0, $15) \
                 RETURNING id",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(body.two_stage)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(next_run_at)
            .fetch_one(pool)
            .await
            .map_err(|e| map_unique_err(e, &body.name))?;
            let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
            id_val.as_i64().unwrap_or(0)
        }
    };

    // 读回完整行
    let task = fetch_task(&state, id).await?.ok_or_else(|| {
        AppError::Internal("刚插入的任务读取失败".into())
    })?;
    tracing::info!(target: "crawler", "Crawler task created: id={id} name={}", body.name);
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `GET /api/crawler/tasks/{id}` — 任务详情
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let task = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `PUT /api/crawler/tasks/{id}` — 部分字段更新
pub async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    // 必须存在
    let existing = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;

    // 合并：以 body 字段覆盖 existing（如果提供）
    let mut merged: CrawlerTaskInput = decode_task_to_input(&existing)?;
    if let Some(n) = body.get("name").and_then(|v| v.as_str()) {
        merged.name = n.to_string();
    }
    if let Some(e) = body.get("enabled").and_then(|v| v.as_bool()) {
        merged.enabled = e;
    }
    if let Some(arr) = body.get("list_urls").and_then(|v| v.as_array()) {
        merged.list_urls = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(s) = body.get("selectors") {
        merged.selectors = serde_json::from_value(s.clone())
            .map_err(|e| AppError::BadRequest(format!("selectors 格式错误: {e}")))?;
    }
    if let Some(t) = body.get("two_stage").and_then(|v| v.as_bool()) {
        merged.two_stage = t;
    }
    if let Some(i) = body.get("interval_minutes").and_then(|v| v.as_i64()) {
        merged.interval_minutes = i;
    }
    if let Some(c) = body.get("task_concurrency").and_then(|v| v.as_i64()) {
        merged.task_concurrency = c;
    }
    if let Some(u) = body.get("user_agent") {
        merged.user_agent = u.as_str().map(String::from);
    }
    if let Some(d) = body.get("request_delay_ms").and_then(|v| v.as_i64()) {
        merged.request_delay_ms = d;
    }
    if let Some(p) = body.get("proxy") {
        merged.proxy = p.as_str().map(String::from);
    }
    if let Some(a) = body.get("auto_link_check").and_then(|v| v.as_bool()) {
        merged.auto_link_check = a;
    }
    if let Some(b) = body.get("block_detection_config") {
        merged.block_detection_config =
            if b.is_null() { None } else { Some(b.to_string()) };
    }
    if let Some(m) = body.get("max_consecutive_failures").and_then(|v| v.as_i64()) {
        merged.max_consecutive_failures = m;
    }
    merged.validate().map_err(AppError::BadRequest)?;

    let now = chrono::Utc::now().naive_utc();
    // 改 interval_minutes → 重算 next_run_at = max(last_run_at + new_interval, now())
    let interval_changed = merged.interval_minutes != existing.interval_minutes;
    let next_run_at = if merged.enabled {
        if interval_changed {
            let base = existing.last_run_at.unwrap_or(now);
            let target = base + chrono::Duration::minutes(merged.interval_minutes);
            Some(target.max(now))
        } else {
            existing.next_run_at.or(Some(now))
        }
    } else {
        None
    };

    let list_urls_json = merged.list_urls_json();
    let selectors_json = merged.selectors_json();
    match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_tasks SET name=?, enabled=?, list_urls=?, selectors=?, \
                 two_stage=?, interval_minutes=?, task_concurrency=?, user_agent=?, \
                 request_delay_ms=?, proxy=?, auto_link_check=?, block_detection_config=?, \
                 max_consecutive_failures=?, next_run_at=?, updated_at=? WHERE id=?",
            )
            .bind(&merged.name)
            .bind(merged.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(merged.two_stage)
            .bind(merged.interval_minutes)
            .bind(merged.task_concurrency)
            .bind(merged.user_agent.as_deref())
            .bind(merged.request_delay_ms)
            .bind(merged.proxy.as_deref())
            .bind(merged.auto_link_check)
            .bind(merged.block_detection_config.as_deref())
            .bind(merged.max_consecutive_failures)
            .bind(next_run_at)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &merged.name))?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_tasks SET name=$1, enabled=$2, list_urls=$3, selectors=$4, \
                 two_stage=$5, interval_minutes=$6, task_concurrency=$7, user_agent=$8, \
                 request_delay_ms=$9, proxy=$10, auto_link_check=$11, block_detection_config=$12, \
                 max_consecutive_failures=$13, next_run_at=$14, updated_at=$15 WHERE id=$16",
            )
            .bind(&merged.name)
            .bind(merged.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(merged.two_stage)
            .bind(merged.interval_minutes)
            .bind(merged.task_concurrency)
            .bind(merged.user_agent.as_deref())
            .bind(merged.request_delay_ms)
            .bind(merged.proxy.as_deref())
            .bind(merged.auto_link_check)
            .bind(merged.block_detection_config.as_deref())
            .bind(merged.max_consecutive_failures)
            .bind(next_run_at)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &merged.name))?;
        }
    }

    let task = fetch_task(&state, id).await?.ok_or_else(|| {
        AppError::Internal("更新后任务读取失败".into())
    })?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `DELETE /api/crawler/tasks/{id}?cascade_articles=true|false`
pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<DeleteParams>,
) -> Result<Json<Value>, AppError> {
    // 先确认存在
    let _ = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;

    let cascade = params.cascade_articles.unwrap_or(false);
    match &state.db {
        DbPool::Sqlite(pool) => {
            if cascade {
                // 级联：先删子表（外键 ON DELETE CASCADE 已配置，但 SQLite 默认不开 FK，显式删）
                sqlx::query(
                    "DELETE FROM crawler_article_images WHERE article_id IN \
                     (SELECT id FROM crawler_articles WHERE task_id = ?)",
                )
                .bind(id)
                .execute(pool)
                .await?;
                sqlx::query(
                    "DELETE FROM crawler_article_links WHERE article_id IN \
                     (SELECT id FROM crawler_articles WHERE task_id = ?)",
                )
                .bind(id)
                .execute(pool)
                .await?;
                sqlx::query("DELETE FROM crawler_articles WHERE task_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM crawler_tasks WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
            } else {
                // 不级联：文章 task_id SET NULL（FR-033）+ 硬删 task
                sqlx::query("UPDATE crawler_articles SET task_id = NULL WHERE task_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM crawler_tasks WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        DbPool::Postgres(pool) => {
            if cascade {
                sqlx::query(
                    "DELETE FROM crawler_article_images WHERE article_id IN \
                     (SELECT id FROM crawler_articles WHERE task_id = $1)",
                )
                .bind(id)
                .execute(pool)
                .await?;
                sqlx::query(
                    "DELETE FROM crawler_article_links WHERE article_id IN \
                     (SELECT id FROM crawler_articles WHERE task_id = $1)",
                )
                .bind(id)
                .execute(pool)
                .await?;
                sqlx::query("DELETE FROM crawler_articles WHERE task_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM crawler_tasks WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
            } else {
                sqlx::query("UPDATE crawler_articles SET task_id = NULL WHERE task_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM crawler_tasks WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    tracing::info!(target: "crawler", "Crawler task deleted: id={id} cascade={cascade}");
    Ok(Json(json!({ "success": true, "message": "任务已删除" })))
}

#[derive(Deserialize)]
pub struct DeleteParams {
    pub cascade_articles: Option<bool>,
}

/// `PUT /api/crawler/tasks/{id}/toggle` — 启用/停用
pub async fn toggle_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| AppError::BadRequest("缺少 enabled 字段".into()))?;
    let _ = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;

    let now = chrono::Utc::now().naive_utc();
    match &state.db {
        DbPool::Sqlite(pool) => {
            if enabled {
                // 启用：status 从 paused/auto_blocked → active + 清失败计数 + next_run_at = now
                sqlx::query(
                    "UPDATE crawler_tasks SET enabled=1, status='active', \
                     consecutive_failures=0, next_run_at=?, updated_at=? WHERE id=?",
                )
                .bind(now)
                .bind(now)
                .bind(id)
                .execute(pool)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE crawler_tasks SET enabled=0, status='paused', \
                     next_run_at=NULL, updated_at=? WHERE id=?",
                )
                .bind(now)
                .bind(id)
                .execute(pool)
                .await?;
            }
        }
        DbPool::Postgres(pool) => {
            if enabled {
                sqlx::query(
                    "UPDATE crawler_tasks SET enabled=TRUE, status='active', \
                     consecutive_failures=0, next_run_at=$1, updated_at=$2 WHERE id=$3",
                )
                .bind(now)
                .bind(now)
                .bind(id)
                .execute(pool)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE crawler_tasks SET enabled=FALSE, status='paused', \
                     next_run_at=NULL, updated_at=$1 WHERE id=$2",
                )
                .bind(now)
                .bind(id)
                .execute(pool)
                .await?;
            }
        }
    }
    let task = fetch_task(&state, id).await?.ok_or_else(|| {
        AppError::Internal("toggle 后任务读取失败".into())
    })?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `POST /api/crawler/tasks/{id}/run` — 立即运行（后台 spawn）
pub async fn run_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    // 确认存在
    let _ = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;
    // 确认调度器在跑（否则报 503 — 但我们 v1 不强制，只 warn）
    {
        let s = state.crawler_scheduler.read().await;
        if !s.running {
            tracing::warn!(target: "crawler", "Run task {id} called but scheduler not running");
        }
    }
    // 后台 spawn — engine::run_task 内部会 finalize_run 写历史
    let st = state.clone();
    tokio::spawn(async move {
        match crate::services::crawler::engine::run_task(id, &st).await {
            Ok(summary) => tracing::info!(
                target: "crawler",
                "Manual run task {id} done: status={} new={} failed={}",
                summary.status, summary.new_count, summary.failed_count
            ),
            Err(e) => tracing::warn!(target: "crawler", "Manual run task {id} error: {e}"),
        }
    });
    Ok(Json(json!({
        "success": true,
        "data": { "task_id": id, "started": true },
        "message": "任务已触发，请稍后查看历史记录"
    })))
}

#[derive(Deserialize)]
pub struct TestBody {
    pub limit: Option<i64>,
}

/// `POST /api/crawler/tasks/{id}/test` — 测试运行（不落库）
pub async fn test_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TestBody>,
) -> Result<Json<Value>, AppError> {
    let limit = body.limit.unwrap_or(3).clamp(1, 10) as usize;
    let preview = crate::services::crawler::engine::test_run(&state.db, id, limit)
        .await
        .map_err(AppError::BadRequest)?;
    Ok(Json(json!({ "success": true, "data": preview })))
}

/// `GET /api/crawler/tasks/{id}/export` — 导出 JSON 配置
pub async fn export_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let task = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;
    let input = decode_task_to_input(&task)?;
    let filename = format!(
        "crawler-task-{}-{}.json",
        sanitize_filename(&task.name),
        task.id
    );
    let body = serde_json::to_string_pretty(&input)
        .map_err(|e| AppError::Internal(format!("序列化失败: {e}")))?;
    Ok((
        axum::http::StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        body,
    )
        .into_response())
}

/// `POST /api/crawler/tasks/import` — 导入 JSON 配置
pub async fn import_task(
    State(state): State<AppState>,
    Json(body): Json<CrawlerTaskInput>,
) -> Result<Json<Value>, AppError> {
    // 与 create_task 同样的校验/插入逻辑，但显式语义
    body.validate().map_err(|e| {
        AppError::BadRequest(format!("导入配置校验失败: {e}"))
    })?;
    let now = chrono::Utc::now().naive_utc();
    let list_urls_json = body.list_urls_json();
    let selectors_json = body.selectors_json();
    let next_run_at = if body.enabled { Some(now) } else { None };

    let id: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, status, consecutive_failures, next_run_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 0, ?)",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(body.two_stage)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(next_run_at)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &body.name))?;
            r.last_insert_rowid()
        }
        DbPool::Postgres(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, status, consecutive_failures, next_run_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'active', 0, $15) \
                 RETURNING id",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&selectors_json)
            .bind(body.two_stage)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(next_run_at)
            .fetch_one(pool)
            .await
            .map_err(|e| map_unique_err(e, &body.name))?;
            let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
            id_val.as_i64().unwrap_or(0)
        }
    };

    let task = fetch_task(&state, id).await?.ok_or_else(|| {
        AppError::Internal("导入后任务读取失败".into())
    })?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `GET /api/crawler/templates` — 列出内置 + 自定义模板
pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let mut templates = builtin_templates();
    // 追加自定义模板（存储在 options 表的 crawler_custom_templates key 下）
    let custom = {
        let cache = state.option_cache.read().await;
        cache.get("crawler_custom_templates").cloned()
    };
    if let Some(json_str) = custom
        && let Ok(custom_list) =
            serde_json::from_str::<Vec<crate::services::crawler::templates::CrawlerTemplate>>(&json_str)
    {
        templates.extend(custom_list);
    }
    Ok(Json(json!({ "success": true, "data": templates })))
}

#[derive(Deserialize)]
pub struct SaveAsTemplateBody {
    pub name: String,
    /// 可选显示描述；未传则使用任务名
    pub description: Option<String>,
}

/// `POST /api/crawler/tasks/{id}/save-as-template`
pub async fn save_as_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<SaveAsTemplateBody>,
) -> Result<Json<Value>, AppError> {
    let task = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;
    let input = decode_task_to_input(&task)?;
    let template = crate::services::crawler::templates::CrawlerTemplate {
        key: format!("custom_{}", task.id),
        name: body.name.clone(),
        site_type: task.template_source.clone().unwrap_or_else(|| "resource".into()),
        description: body.description.unwrap_or_default(),
        config: input,
    };

    // 读 → 追加 → 写回 options.crawler_custom_templates
    let mut list: Vec<crate::services::crawler::templates::CrawlerTemplate> = {
        let cache = state.option_cache.read().await;
        cache
            .get("crawler_custom_templates")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    };
    // 同名覆盖
    list.retain(|t| t.name != template.name);
    list.push(template.clone());

    let serialized = serde_json::to_string(&list)
        .map_err(|e| AppError::Internal(format!("模板序列化失败: {e}")))?;
    {
        let pool = &state.db;
        let now = chrono::Utc::now().naive_utc();
        match pool {
            DbPool::Sqlite(p) => {
                sqlx::query(
                    "INSERT INTO options (key, value, updated_at) VALUES ('crawler_custom_templates', ?, ?) \
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                )
                .bind(&serialized)
                .bind(now)
                .execute(p)
                .await?;
            }
            DbPool::Postgres(p) => {
                sqlx::query(
                    "INSERT INTO options (key, value, updated_at) VALUES ('crawler_custom_templates', $1, $2) \
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                )
                .bind(&serialized)
                .bind(now)
                .execute(p)
                .await?;
            }
        }
        // 更新 OptionCache
        let mut cache = state.option_cache.write().await;
        cache.insert("crawler_custom_templates".into(), serialized);
    }
    Ok(Json(json!({ "success": true, "data": template })))
}

// ---------- 公共辅助 ----------

async fn fetch_task(state: &AppState, id: i64) -> Result<Option<CrawlerTask>, AppError> {
    let sql = "SELECT id, name, enabled, list_urls, selectors, two_stage, \
     interval_minutes, task_concurrency, user_agent, request_delay_ms, \
     proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
     template_source, status, consecutive_failures, last_run_at, next_run_at, \
     created_at, updated_at FROM crawler_tasks WHERE id = ?";
    let sql_pg = sql.replace("WHERE id = ?", "WHERE id = $1");
    Ok(match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerTask>(sql).bind(id).fetch_optional(pool).await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerTask>(&sql_pg).bind(id).fetch_optional(pool).await?
        }
    })
}

/// 把 DB 行（list_urls/selectors 是 JSON 字符串）解码为 CrawlerTaskInput
fn decode_task_to_input(t: &CrawlerTask) -> Result<CrawlerTaskInput, AppError> {
    let list_urls: Vec<String> = serde_json::from_str(&t.list_urls)
        .map_err(|e| AppError::Internal(format!("list_urls 解析失败: {e}")))?;
    let selectors = serde_json::from_str(&t.selectors)
        .map_err(|e| AppError::Internal(format!("selectors 解析失败: {e}")))?;
    Ok(CrawlerTaskInput {
        name: t.name.clone(),
        enabled: t.enabled,
        list_urls,
        selectors,
        two_stage: t.two_stage,
        interval_minutes: t.interval_minutes,
        task_concurrency: t.task_concurrency,
        user_agent: t.user_agent.clone(),
        request_delay_ms: t.request_delay_ms,
        proxy: t.proxy.clone(),
        auto_link_check: t.auto_link_check,
        block_detection_config: t.block_detection_config.clone(),
        max_consecutive_failures: t.max_consecutive_failures,
        template_source: t.template_source.clone(),
    })
}

fn map_unique_err(e: sqlx::Error, name: &str) -> AppError {
    let msg = e.to_string();
    if msg.contains("UNIQUE") && msg.contains("name") {
        AppError::BadRequest(format!("任务名 '{name}' 已存在"))
    } else {
        AppError::Database(e)
    }
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ---------- WHERE 子句构建（双库占位符分别生成） ----------

struct WhereSql {
    /// SQLite 用 `?` 占位
    sqlite: String,
    /// Postgres 用 `$1..$N` 占位
    postgres: String,
}

fn build_where(keyword: &str, status: Option<&str>, enabled: Option<bool>, _unused: bool) -> WhereSql {
    let mut clauses: Vec<&'static str> = vec![];
    if !keyword.is_empty() {
        clauses.push("(name LIKE ? OR template_source LIKE ?)");
    }
    if status.is_some() {
        clauses.push("status = ?");
    }
    if enabled.is_some() {
        if enabled.unwrap() {
            clauses.push("enabled = 1");
        } else {
            clauses.push("enabled = 0");
        }
    }
    if clauses.is_empty() {
        return WhereSql {
            sqlite: String::new(),
            postgres: String::new(),
        };
    }
    let joined = clauses.join(" AND ");
    let sqlite = format!("WHERE {joined}");
    // Postgres: 把每个 ? 重写为 $N（注意 LIKE 子句有两个 ?）
    let mut pg_idx = 1usize;
    let mut pg = String::from("WHERE ");
    let mut first = true;
    for c in clauses {
        if !first {
            pg.push_str(" AND ");
        }
        first = false;
        // 替换该子句中的 ?
        let mut clause_pg = c.to_string();
        while let Some(pos) = clause_pg.find('?') {
            clause_pg = format!("{}${pg_idx}{}", &clause_pg[..pos], &clause_pg[pos + 1..]);
            pg_idx += 1;
        }
        // enabled=1/0 无占位，原样追加
        pg.push_str(&clause_pg);
    }
    WhereSql { sqlite, postgres: pg }
}

fn pg_filter_count(keyword: &str, status: Option<&str>, enabled: Option<bool>) -> usize {
    let mut n = 0usize;
    if !keyword.is_empty() {
        n += 2; // name + template_source
    }
    if status.is_some() {
        n += 1;
    }
    // enabled=1/0 无占位
    let _ = enabled;
    n
}

// ════════════════════════════════════════════════════════════════════════════
// Phase 4 US2 — 文章端点（T035-T039）
// ════════════════════════════════════════════════════════════════════════════

/// 文章列表查询参数
#[derive(Deserialize)]
pub struct ArticleListParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub task_id: Option<i64>,
    pub source_type: Option<String>,
    pub category: Option<String>,
    pub crawled_after: Option<String>,
    pub crawled_before: Option<String>,
    pub keyword: Option<String>,
}

/// 构造文章列表的 WHERE 子句（sqlite + postgres 同结构，统一 `?` / `$N` 占位）。
/// 返回 (sql_fragment, param_count)
fn build_article_where(
    task_id: Option<i64>,
    source_type: Option<&str>,
    category: Option<&str>,
    crawled_after: Option<&str>,
    crawled_before: Option<&str>,
    keyword: &str,
) -> (String, usize) {
    let mut clauses: Vec<String> = Vec::new();
    let mut n = 0usize;
    if task_id.is_some() {
        n += 1;
        clauses.push(format!("a.task_id = {}", placeholder(n)));
    }
    if source_type.is_some() {
        n += 1;
        clauses.push(format!("a.source_type = {}", placeholder(n)));
    }
    if category.is_some() {
        n += 1;
        clauses.push(format!("a.category = {}", placeholder(n)));
    }
    if crawled_after.is_some() {
        n += 1;
        clauses.push(format!("a.crawled_at >= {}", placeholder(n)));
    }
    if crawled_before.is_some() {
        n += 1;
        clauses.push(format!("a.crawled_at <= {}", placeholder(n)));
    }
    if !keyword.is_empty() {
        n += 2;
        clauses.push(format!(
            "(a.title LIKE {} OR a.content LIKE {})",
            placeholder(n - 1),
            placeholder(n)
        ));
    }
    if clauses.is_empty() {
        (String::new(), 0)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), n)
    }
}

/// 返回当前数据库类型的占位符（sqlite=`?`, postgres=`$N`）。
/// 调用方需在外层 match 中确定 db_kind 后传入；这里用 thread_local 不合适，
/// 因此我们把占位符生成拆成两个分支：调用方根据 DbPool 直接构造 SQL。
fn placeholder(_n: usize) -> String {
    // 此函数仅用于 sqlite 分支；postgres 分支请使用 pg_placeholder(n)
    "?".to_string()
}

/// `GET /api/crawler/articles` — 文章列表（聚合子表 count + 首图）
pub async fn list_articles(
    State(state): State<AppState>,
    Query(params): Query<ArticleListParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * page_size;

    let keyword = params.keyword.as_deref().unwrap_or("").trim();
    let source_type = params
        .source_type
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let category = params
        .category
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let crawled_after = params
        .crawled_after
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let crawled_before = params
        .crawled_before
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let items: Vec<CrawlerArticleListItem> = match &state.db {
        DbPool::Sqlite(pool) => {
            let (where_sql, _) = build_article_where(
                params.task_id,
                source_type.as_deref(),
                category.as_deref(),
                crawled_after.as_deref(),
                crawled_before.as_deref(),
                keyword,
            );
            let sql = format!(
                "SELECT a.id, a.task_id, a.source_type, a.title, a.category,
                   (SELECT im.file_id FROM crawler_article_images im
                      WHERE im.article_id = a.id AND im.status = 'uploaded'
                      ORDER BY im.id LIMIT 1) AS thumbnail,
                   (SELECT COUNT(*) FROM crawler_article_links l
                      WHERE l.article_id = a.id AND l.link_type = 'pan') AS pan_link_count,
                   (SELECT COUNT(*) FROM crawler_article_links l
                      WHERE l.article_id = a.id AND l.link_type = 'direct') AS direct_link_count,
                   (SELECT COUNT(*) FROM crawler_article_images im
                      WHERE im.article_id = a.id) AS image_count,
                   a.is_edited, a.crawled_at
                 FROM crawler_articles a
                 {where_sql}
                 ORDER BY a.crawled_at DESC, a.id DESC
                 LIMIT ? OFFSET ?"
            );
            let mut q = sqlx::query_as::<_, CrawlerArticleListItem>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = source_type.as_deref() {
                q = q.bind(s);
            }
            if let Some(c) = category.as_deref() {
                q = q.bind(c);
            }
            if let Some(t) = crawled_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = crawled_before.as_deref() {
                q = q.bind(t);
            }
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            q = q.bind(page_size).bind(offset);
            q.fetch_all(pool).await?
        }
        DbPool::Postgres(pool) => {
            let (where_sql, mut n) = build_article_where_pg(
                params.task_id,
                source_type.as_deref(),
                category.as_deref(),
                crawled_after.as_deref(),
                crawled_before.as_deref(),
                keyword,
            );
            n += 2;
            let sql = format!(
                "SELECT a.id, a.task_id, a.source_type, a.title, a.category,
                   (SELECT im.file_id FROM crawler_article_images im
                      WHERE im.article_id = a.id AND im.status = 'uploaded'
                      ORDER BY im.id LIMIT 1) AS thumbnail,
                   (SELECT COUNT(*) FROM crawler_article_links l
                      WHERE l.article_id = a.id AND l.link_type = 'pan') AS pan_link_count,
                   (SELECT COUNT(*) FROM crawler_article_links l
                      WHERE l.article_id = a.id AND l.link_type = 'direct') AS direct_link_count,
                   (SELECT COUNT(*) FROM crawler_article_images im
                      WHERE im.article_id = a.id) AS image_count,
                   a.is_edited, a.crawled_at
                 FROM crawler_articles a
                 {where_sql}
                 ORDER BY a.crawled_at DESC, a.id DESC
                 LIMIT ${n} OFFSET ${m}",
                n = n,
                m = n + 1
            );
            let mut q = sqlx::query_as::<_, CrawlerArticleListItem>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = source_type.as_deref() {
                q = q.bind(s);
            }
            if let Some(c) = category.as_deref() {
                q = q.bind(c);
            }
            if let Some(t) = crawled_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = crawled_before.as_deref() {
                q = q.bind(t);
            }
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            q = q.bind(page_size).bind(offset);
            q.fetch_all(pool).await?
        }
    };

    // total
    let total: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let (where_sql, _) = build_article_where(
                params.task_id,
                source_type.as_deref(),
                category.as_deref(),
                crawled_after.as_deref(),
                crawled_before.as_deref(),
                keyword,
            );
            let sql = format!("SELECT COUNT(*) FROM crawler_articles a {where_sql}");
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = source_type.as_deref() {
                q = q.bind(s);
            }
            if let Some(c) = category.as_deref() {
                q = q.bind(c);
            }
            if let Some(t) = crawled_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = crawled_before.as_deref() {
                q = q.bind(t);
            }
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let (where_sql, _) = build_article_where_pg(
                params.task_id,
                source_type.as_deref(),
                category.as_deref(),
                crawled_after.as_deref(),
                crawled_before.as_deref(),
                keyword,
            );
            let sql = format!("SELECT COUNT(*) FROM crawler_articles a {where_sql}");
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = source_type.as_deref() {
                q = q.bind(s);
            }
            if let Some(c) = category.as_deref() {
                q = q.bind(c);
            }
            if let Some(t) = crawled_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = crawled_before.as_deref() {
                q = q.bind(t);
            }
            if !keyword.is_empty() {
                q = q.bind(format!("%{keyword}%")).bind(format!("%{keyword}%"));
            }
            q.fetch_one(pool).await?
        }
    };

    Ok(Json(json!({
        "success": true,
        "data": {
            "list": items,
            "pagination": { "page": page, "page_size": page_size, "total": total }
        }
    })))
}

/// Postgres 分支的 WHERE 子句构造（与 sqlite 版结构对称）
fn build_article_where_pg(
    task_id: Option<i64>,
    source_type: Option<&str>,
    category: Option<&str>,
    crawled_after: Option<&str>,
    crawled_before: Option<&str>,
    keyword: &str,
) -> (String, usize) {
    let mut clauses: Vec<String> = Vec::new();
    let mut n = 0usize;
    if task_id.is_some() {
        n += 1;
        clauses.push(format!("a.task_id = ${n}"));
    }
    if source_type.is_some() {
        n += 1;
        clauses.push(format!("a.source_type = ${n}"));
    }
    if category.is_some() {
        n += 1;
        clauses.push(format!("a.category = ${n}"));
    }
    if crawled_after.is_some() {
        n += 1;
        clauses.push(format!("a.crawled_at >= ${n}"));
    }
    if crawled_before.is_some() {
        n += 1;
        clauses.push(format!("a.crawled_at <= ${n}"));
    }
    if !keyword.is_empty() {
        n += 2;
        clauses.push(format!(
            "(a.title LIKE ${m} OR a.content LIKE ${n})",
            m = n - 1,
            n = n
        ));
    }
    if clauses.is_empty() {
        (String::new(), 0)
    } else {
        (format!("WHERE {}", clauses.join(" AND ")), n)
    }
}

/// `GET /api/crawler/articles/:id` — 文章详情（含 links + images + task_name）
pub async fn get_article_detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<CrawlerArticleDetail>, AppError> {
    // 1. 主表
    let article: CrawlerArticle = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerArticle>("SELECT * FROM crawler_articles WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerArticle>("SELECT * FROM crawler_articles WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("文章 {id} 不存在")))?;

    // 2. links
    let links: Vec<CrawlerArticleLink> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerArticleLink>(
                "SELECT * FROM crawler_article_links WHERE article_id = ? ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerArticleLink>(
                "SELECT * FROM crawler_article_links WHERE article_id = $1 ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
    };

    // 3. images
    let images: Vec<CrawlerArticleImage> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerArticleImage>(
                "SELECT * FROM crawler_article_images WHERE article_id = ? ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerArticleImage>(
                "SELECT * FROM crawler_article_images WHERE article_id = $1 ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
    };

    // 4. task_name（即使 task_id NULL 也回填快照）
    let task_name: Option<String> = if let Some(tid) = article.task_id {
        match &state.db {
            DbPool::Sqlite(pool) => {
                sqlx::query_scalar::<_, Option<String>>(
                    "SELECT name FROM crawler_tasks WHERE id = ?",
                )
                .bind(tid)
                .fetch_optional(pool)
                .await?
                .flatten()
            }
            DbPool::Postgres(pool) => {
                sqlx::query_scalar::<_, Option<String>>(
                    "SELECT name FROM crawler_tasks WHERE id = $1",
                )
                .bind(tid)
                .fetch_optional(pool)
                .await?
                .flatten()
            }
        }
    } else {
        None
    };

    Ok(Json(CrawlerArticleDetail {
        article,
        links,
        images,
        task_name,
    }))
}

/// `PUT /api/crawler/articles/:id` — 更新文章（title/content/category/tags），写 is_edited=true
#[derive(Deserialize)]
pub struct UpdateArticleBody {
    pub title: Option<String>,
    pub content: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
}

pub async fn update_article(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateArticleBody>,
) -> Result<Json<Value>, AppError> {
    let affected = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_articles
                 SET title = COALESCE(?, title),
                     content = COALESCE(?, content),
                     category = COALESCE(?, category),
                     tags = COALESCE(?, tags),
                     is_edited = 1,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
            )
            .bind(body.title)
            .bind(body.content)
            .bind(body.category)
            .bind(body.tags)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_articles
                 SET title = COALESCE($1, title),
                     content = COALESCE($2, content),
                     category = COALESCE($3, category),
                     tags = COALESCE($4, tags),
                     is_edited = TRUE,
                     updated_at = CURRENT_TIMESTAMP
                 WHERE id = $5",
            )
            .bind(body.title)
            .bind(body.content)
            .bind(body.category)
            .bind(body.tags)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
        }
    };
    if affected == 0 {
        return Err(AppError::NotFound(format!("文章 {id} 不存在")));
    }
    Ok(Json(json!({ "id": id, "updated": affected })))
}

/// `DELETE /api/crawler/articles/:id` — 删除文章（级联子表）
pub async fn delete_article(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let (links_del, imgs_del, art_del) = match &state.db {
        DbPool::Sqlite(pool) => {
            let l = sqlx::query("DELETE FROM crawler_article_links WHERE article_id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            let i = sqlx::query("DELETE FROM crawler_article_images WHERE article_id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            let a = sqlx::query("DELETE FROM crawler_articles WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            (l, i, a)
        }
        DbPool::Postgres(pool) => {
            let l = sqlx::query("DELETE FROM crawler_article_links WHERE article_id = $1")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            let i = sqlx::query("DELETE FROM crawler_article_images WHERE article_id = $1")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            let a = sqlx::query("DELETE FROM crawler_articles WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected();
            (l, i, a)
        }
    };
    Ok(Json(json!({
        "id": id,
        "articles_deleted": art_del,
        "links_deleted": links_del,
        "images_deleted": imgs_del,
    })))
}

/// `POST /api/crawler/articles/batch-delete` — 批量删除
#[derive(Deserialize)]
pub struct BatchDeleteBody {
    pub ids: Vec<i64>,
}

pub async fn batch_delete_articles(
    State(state): State<AppState>,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<Value>, AppError> {
    if body.ids.is_empty() {
        return Ok(Json(json!({ "deleted": 0 })));
    }
    let mut total: u64 = 0;
    for id in &body.ids {
        match &state.db {
            DbPool::Sqlite(pool) => {
                let _ = sqlx::query("DELETE FROM crawler_article_links WHERE article_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await;
                let _ = sqlx::query("DELETE FROM crawler_article_images WHERE article_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await;
                total += sqlx::query("DELETE FROM crawler_articles WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
            }
            DbPool::Postgres(pool) => {
                let _ = sqlx::query("DELETE FROM crawler_article_links WHERE article_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await;
                let _ = sqlx::query("DELETE FROM crawler_article_images WHERE article_id = $1")
                    .bind(id)
                    .execute(pool)
                    .await;
                total += sqlx::query("DELETE FROM crawler_articles WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
            }
        }
    }
    Ok(Json(json!({ "deleted": total, "requested": body.ids.len() })))
}

/// `POST /api/crawler/articles/:id/images/:image_id/retry` — 重置图片重试计数
pub async fn retry_image(
    State(state): State<AppState>,
    Path((id, image_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, AppError> {
    let affected = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images
                 SET retry_count = 0, status = 'pending', last_error = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ? AND article_id = ?",
            )
            .bind(image_id)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_article_images
                 SET retry_count = 0, status = 'pending', last_error = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE id = $1 AND article_id = $2",
            )
            .bind(image_id)
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected()
        }
    };
    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "图片 {image_id}（文章 {id}）不存在"
        )));
    }
    Ok(Json(json!({ "id": image_id, "article_id": id, "reset": true })))
}

/// `POST /api/crawler/articles/:id/links/check` — 调用 LinkChecker 检测网盘链接
pub async fn check_article_links(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    // 1. 取出该文章所有网盘链接（link_type='pan'）
    let pan_links: Vec<CrawlerArticleLink> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerArticleLink>(
                "SELECT * FROM crawler_article_links WHERE article_id = ? AND link_type = 'pan' ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerArticleLink>(
                "SELECT * FROM crawler_article_links WHERE article_id = $1 AND link_type = 'pan' ORDER BY id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
    };

    if pan_links.is_empty() {
        return Ok(Json(json!({ "article_id": id, "checked": 0, "note": "无网盘链接" })));
    }

    // 2. 解析 LinkChecker（PanCheck 未配置则全 unknown 不报错）
    let checker = crate::services::link_checker::resolve_checker(&state.option_cache).await?;

    let now = chrono::Utc::now().naive_utc();
    let mut checked = 0usize;

    if let Some(checker) = checker {
        let urls: Vec<String> = pan_links.iter().map(|l| l.url.clone()).collect();
        let verdicts = checker.check(&urls).await?;
        for v in &verdicts {
            // 找到对应链接（按 url 匹配；重复 url 全部更新）
            let status_str = v.status.as_str();
            let reason = v.fail_reason.as_deref();
            for link in &pan_links {
                if link.url == v.url {
                    checked += 1;
                    match &state.db {
                        DbPool::Sqlite(pool) => {
                            let _ = sqlx::query(
                                "UPDATE crawler_article_links
                                 SET validity_status = ?, validity_reason = ?, last_checked_at = ?, updated_at = CURRENT_TIMESTAMP
                                 WHERE id = ?",
                            )
                            .bind(status_str)
                            .bind(reason)
                            .bind(now)
                            .bind(link.id)
                            .execute(pool)
                            .await;
                        }
                        DbPool::Postgres(pool) => {
                            let _ = sqlx::query(
                                "UPDATE crawler_article_links
                                 SET validity_status = $1, validity_reason = $2, last_checked_at = $3, updated_at = CURRENT_TIMESTAMP
                                 WHERE id = $4",
                            )
                            .bind(status_str)
                            .bind(reason)
                            .bind(now)
                            .bind(link.id)
                            .execute(pool)
                            .await;
                        }
                    }
                }
            }
        }
    } else {
        // PanCheck 未配置 → 全部标 unknown，不报错
        for link in &pan_links {
            checked += 1;
            match &state.db {
                DbPool::Sqlite(pool) => {
                    let _ = sqlx::query(
                        "UPDATE crawler_article_links
                         SET validity_status = 'unknown', validity_reason = NULL, last_checked_at = ?, updated_at = CURRENT_TIMESTAMP
                         WHERE id = ?",
                    )
                    .bind(now)
                    .bind(link.id)
                    .execute(pool)
                    .await;
                }
                DbPool::Postgres(pool) => {
                    let _ = sqlx::query(
                        "UPDATE crawler_article_links
                         SET validity_status = 'unknown', validity_reason = NULL, last_checked_at = $1, updated_at = CURRENT_TIMESTAMP
                         WHERE id = $2",
                    )
                    .bind(now)
                    .bind(link.id)
                    .execute(pool)
                    .await;
                }
            }
        }
        return Ok(Json(json!({
            "article_id": id,
            "checked": checked,
            "note": "PanCheck 未配置，全部标记为 unknown"
        })));
    }

    Ok(Json(json!({ "article_id": id, "checked": checked })))
}

// ════════════════════════════════════════════════════════════════════════════
// Phase 5 US3 — 历史与统计端点（T045-T046）
// ════════════════════════════════════════════════════════════════════════════

/// 历史列表查询参数
#[derive(Deserialize)]
pub struct HistoryListParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub task_id: Option<i64>,
    pub status: Option<String>,
    pub started_after: Option<String>,
    pub started_before: Option<String>,
}

/// `GET /api/crawler/histories` — 历史列表
pub async fn list_histories(
    State(state): State<AppState>,
    Query(params): Query<HistoryListParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 200);
    let offset = (page - 1) * page_size;

    let status = params
        .status
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all")
        .map(String::from);
    let started_after = params
        .started_after
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);
    let started_before = params
        .started_before
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let rows: Vec<CrawlerRunHistory> = match &state.db {
        DbPool::Sqlite(pool) => {
            let mut sql = String::from("SELECT * FROM crawler_run_histories WHERE 1=1");
            if params.task_id.is_some() {
                sql.push_str(" AND task_id = ?");
            }
            if status.is_some() {
                sql.push_str(" AND status = ?");
            }
            if started_after.is_some() {
                sql.push_str(" AND started_at >= ?");
            }
            if started_before.is_some() {
                sql.push_str(" AND started_at <= ?");
            }
            sql.push_str(" ORDER BY started_at DESC, id DESC LIMIT ? OFFSET ?");

            let mut q = sqlx::query_as::<_, CrawlerRunHistory>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            if let Some(t) = started_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = started_before.as_deref() {
                q = q.bind(t);
            }
            q.bind(page_size).bind(offset).fetch_all(pool).await?
        }
        DbPool::Postgres(pool) => {
            // 动态构造 $N 占位
            let mut sql = String::from("SELECT * FROM crawler_run_histories WHERE 1=1");
            let mut n = 0usize;
            if params.task_id.is_some() {
                n += 1;
                sql.push_str(&format!(" AND task_id = ${n}"));
            }
            if status.is_some() {
                n += 1;
                sql.push_str(&format!(" AND status = ${n}"));
            }
            if started_after.is_some() {
                n += 1;
                sql.push_str(&format!(" AND started_at >= ${n}"));
            }
            if started_before.is_some() {
                n += 1;
                sql.push_str(&format!(" AND started_at <= ${n}"));
            }
            n += 1;
            let limit_n = n;
            n += 1;
            let offset_n = n;
            sql.push_str(&format!(
                " ORDER BY started_at DESC, id DESC LIMIT ${limit_n} OFFSET ${offset_n}"
            ));

            let mut q = sqlx::query_as::<_, CrawlerRunHistory>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            if let Some(t) = started_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = started_before.as_deref() {
                q = q.bind(t);
            }
            q.bind(page_size).bind(offset).fetch_all(pool).await?
        }
    };

    // total
    let total: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let mut sql = String::from("SELECT COUNT(*) FROM crawler_run_histories WHERE 1=1");
            if params.task_id.is_some() {
                sql.push_str(" AND task_id = ?");
            }
            if status.is_some() {
                sql.push_str(" AND status = ?");
            }
            if started_after.is_some() {
                sql.push_str(" AND started_at >= ?");
            }
            if started_before.is_some() {
                sql.push_str(" AND started_at <= ?");
            }
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            if let Some(t) = started_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = started_before.as_deref() {
                q = q.bind(t);
            }
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let mut sql = String::from("SELECT COUNT(*) FROM crawler_run_histories WHERE 1=1");
            let mut n = 0usize;
            if params.task_id.is_some() {
                n += 1;
                sql.push_str(&format!(" AND task_id = ${n}"));
            }
            if status.is_some() {
                n += 1;
                sql.push_str(&format!(" AND status = ${n}"));
            }
            if started_after.is_some() {
                n += 1;
                sql.push_str(&format!(" AND started_at >= ${n}"));
            }
            if started_before.is_some() {
                n += 1;
                sql.push_str(&format!(" AND started_at <= ${n}"));
            }
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            if let Some(s) = status.as_deref() {
                q = q.bind(s);
            }
            if let Some(t) = started_after.as_deref() {
                q = q.bind(t);
            }
            if let Some(t) = started_before.as_deref() {
                q = q.bind(t);
            }
            q.fetch_one(pool).await?
        }
    };

    Ok(Json(json!({
        "success": true,
        "data": {
            "list": rows,
            "pagination": { "page": page, "page_size": page_size, "total": total }
        }
    })))
}

/// `GET /api/crawler/histories/:id` — 历史详情
pub async fn get_history_detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<CrawlerRunHistoryDetail>, AppError> {
    let row: CrawlerRunHistory = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, CrawlerRunHistory>("SELECT * FROM crawler_run_histories WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, CrawlerRunHistory>("SELECT * FROM crawler_run_histories WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("历史 {id} 不存在")))?;

    // blocked_response_excerpt 列尚未在 schema 中持久化，留空（前端兼容）
    Ok(Json(CrawlerRunHistoryDetail {
        history: row,
        blocked_response_excerpt: None,
    }))
}

/// 历史统计查询参数
#[derive(Deserialize)]
pub struct HistoryStatsParams {
    pub task_id: Option<i64>,
    pub days: Option<i64>,
}

/// `GET /api/crawler/histories/stats` — 聚合统计（仪表盘告警用）
pub async fn get_history_stats(
    State(state): State<AppState>,
    Query(params): Query<HistoryStatsParams>,
) -> Result<Json<Value>, AppError> {
    let days = params.days.unwrap_or(7).max(1);
    let since = chrono::Utc::now().naive_utc() - chrono::Duration::days(days);

    // helper: 拼接 WHERE 子句
    let (where_sqlite, where_pg, has_task, has_since) = (
        {
            let mut s = String::from("WHERE started_at >= ?");
            if params.task_id.is_some() {
                s.push_str(" AND task_id = ?");
            }
            s
        },
        {
            let mut s = String::from("WHERE started_at >= $1");
            if params.task_id.is_some() {
                s.push_str(" AND task_id = $2");
            }
            s
        },
        params.task_id.is_some(),
        true,
    );
    let _ = has_since;

    // 状态聚合
    let (total, success, partial, failed, blocked): (i64, i64, i64, i64, i64) = match &state.db {
        DbPool::Sqlite(pool) => {
            let sql = format!(
                "SELECT \
                 COUNT(*) AS total, \
                 SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS success, \
                 SUM(CASE WHEN status='partial' THEN 1 ELSE 0 END) AS partial, \
                 SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS failed, \
                 SUM(CASE WHEN status='blocked' THEN 1 ELSE 0 END) AS blocked \
                 FROM crawler_run_histories {where_sqlite}"
            );
            let mut q = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(&sql);
            q = q.bind(since);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let sql = format!(
                "SELECT \
                 COUNT(*) AS total, \
                 SUM(CASE WHEN status='success' THEN 1 ELSE 0 END) AS success, \
                 SUM(CASE WHEN status='partial' THEN 1 ELSE 0 END) AS partial, \
                 SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS failed, \
                 SUM(CASE WHEN status='blocked' THEN 1 ELSE 0 END) AS blocked \
                 FROM crawler_run_histories {where_pg}"
            );
            let mut q = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(&sql);
            q = q.bind(since);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            q.fetch_one(pool).await?
        }
    };

    // block_breakdown（按 block_type 分组）
    let block_breakdown: std::collections::HashMap<String, i64> = match &state.db {
        DbPool::Sqlite(pool) => {
            let sql = format!(
                "SELECT block_type, COUNT(*) FROM crawler_run_histories \
                 {where_sqlite} AND status='blocked' AND block_type IS NOT NULL \
                 GROUP BY block_type"
            );
            let mut q = sqlx::query_as::<_, (Option<String>, i64)>(&sql);
            q = q.bind(since);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            let rows = q.fetch_all(pool).await?;
            rows.into_iter()
                .filter_map(|(k, v)| k.map(|k| (k, v)))
                .collect()
        }
        DbPool::Postgres(pool) => {
            let sql = format!(
                "SELECT block_type, COUNT(*) FROM crawler_run_histories \
                 {where_pg} AND status='blocked' AND block_type IS NOT NULL \
                 GROUP BY block_type"
            );
            let mut q = sqlx::query_as::<_, (Option<String>, i64)>(&sql);
            q = q.bind(since);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            let rows = q.fetch_all(pool).await?;
            rows.into_iter()
                .filter_map(|(bt, c)| bt.map(|b| (b, c)))
                .collect()
        }
    };

    let _ = has_task;

    // last_run_at
    let last_run_at: Option<chrono::NaiveDateTime> = match &state.db {
        DbPool::Sqlite(pool) => {
            let mut q = sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(
                "SELECT MAX(started_at) FROM crawler_run_histories WHERE 1=1",
            );
            if let Some(tid) = params.task_id {
                let sql = "SELECT MAX(started_at) FROM crawler_run_histories WHERE task_id = ?";
                q = sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(sql).bind(tid);
            }
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let sql = if params.task_id.is_some() {
                "SELECT MAX(started_at) FROM crawler_run_histories WHERE task_id = $1"
            } else {
                "SELECT MAX(started_at) FROM crawler_run_histories WHERE 1=1"
            };
            let mut q = sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(sql);
            if let Some(tid) = params.task_id {
                q = q.bind(tid);
            }
            q.fetch_one(pool).await?
        }
    };

    // auto_blocked_tasks 数
    let auto_blocked_tasks: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_tasks WHERE status='auto_blocked'",
            )
            .fetch_one(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_tasks WHERE status='auto_blocked'",
            )
            .fetch_one(pool)
            .await?
        }
    };

    let stats = CrawlerHistoryStats {
        total_runs: total,
        success,
        partial,
        failed,
        blocked,
        block_breakdown,
        last_run_at,
        auto_blocked_tasks,
    };

    Ok(Json(json!({ "success": true, "data": stats })))
}
