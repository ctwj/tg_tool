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

    let select_cols = "id, name, enabled, list_urls, two_stage, \
         interval_minutes, task_concurrency, user_agent, request_delay_ms, \
         proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
         template_source, pagination_selector, max_pages, max_pagination_depth, \
         force_full_collect, page_url_template, page_start, page_end, \
         status, consecutive_failures, last_run_at, next_run_at, \
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
    // enabled=true 立即可调度（next_run_at=now()）
    let next_run_at = if body.enabled { Some(now) } else { None };

    let id: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, max_pagination_depth, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 0, ?)",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：DB 列保留兼容，强制写入 true
            .bind(true)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(body.pagination_selector.as_deref())
            .bind(body.max_pages)
            .bind(body.max_pagination_depth)
            .bind(body.force_full_collect)
            .bind(&body.page_url_template)
            .bind(body.page_start)
            .bind(body.page_end)
            .bind(next_run_at)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &body.name))?;
            r.last_insert_rowid()
        }
        DbPool::Postgres(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, max_pagination_depth, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, 'active', 0, $21) \
                 RETURNING id",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：DB 列保留兼容，强制写入 true
            .bind(true)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(body.pagination_selector.as_deref())
            .bind(body.max_pages)
            .bind(body.max_pagination_depth)
            .bind(body.force_full_collect)
            .bind(&body.page_url_template)
            .bind(body.page_start)
            .bind(body.page_end)
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
    // 043：selectors 字段已移除（DB 列已删），字段配置通过 /field-nodes 独立 CRUD 管理
    // two_stage 已下线：忽略客户端传入，强制保持 true
    merged.two_stage = true;
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
    if let Some(s) = body.get("pagination_selector") {
        merged.pagination_selector = if s.is_null() {
            None
        } else {
            s.as_str().map(String::from)
        };
    }
    if let Some(mp) = body.get("max_pages").and_then(|v| v.as_i64()) {
        merged.max_pages = mp;
    }
    if let Some(d) = body.get("max_pagination_depth").and_then(|v| v.as_i64()) {
        merged.max_pagination_depth = d;
    }
    if let Some(b) = body.get("force_full_collect").and_then(|v| v.as_bool()) {
        merged.force_full_collect = b;
    }
    if let Some(t) = body.get("page_url_template").and_then(|v| v.as_str()) {
        merged.page_url_template = t.to_string();
    }
    if let Some(ps) = body.get("page_start").and_then(|v| v.as_i64()) {
        merged.page_start = ps;
    }
    if let Some(pe) = body.get("page_end").and_then(|v| v.as_i64()) {
        merged.page_end = pe;
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
    // 同步 status：enabled 字段变化时按 active/paused 对齐（与 toggle 接口语义一致），
    // 否则保留原 status（如 auto_blocked 不应被无关字段编辑抹掉）。
    // 修复 bug：旧版只更新 enabled 不同步 status，会产生 enabled=0+status=active 的矛盾态，
    // 进而使调度 SQL `WHERE enabled=1 AND status='active'` 永远扫不到该任务。
    let enabled_changed = merged.enabled != existing.enabled;
    let new_status: String = if enabled_changed {
        if merged.enabled {
            "active".to_string()
        } else {
            "paused".to_string()
        }
    } else {
        existing.status.clone()
    };

    let list_urls_json = merged.list_urls_json();
    match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_tasks SET name=?, enabled=?, list_urls=?, \
                 two_stage=?, interval_minutes=?, task_concurrency=?, user_agent=?, \
                 request_delay_ms=?, proxy=?, auto_link_check=?, block_detection_config=?, \
                 max_consecutive_failures=?, pagination_selector=?, max_pages=?, max_pagination_depth=?, \
                 force_full_collect=?, page_url_template=?, page_start=?, page_end=?, \
                 status=?, next_run_at=?, updated_at=? WHERE id=?",
            )
            .bind(&merged.name)
            .bind(merged.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：强制 true
            .bind(true)
            .bind(merged.interval_minutes)
            .bind(merged.task_concurrency)
            .bind(merged.user_agent.as_deref())
            .bind(merged.request_delay_ms)
            .bind(merged.proxy.as_deref())
            .bind(merged.auto_link_check)
            .bind(merged.block_detection_config.as_deref())
            .bind(merged.max_consecutive_failures)
            .bind(merged.pagination_selector.as_deref())
            .bind(merged.max_pages)
            .bind(merged.max_pagination_depth)
            .bind(merged.force_full_collect)
            .bind(&merged.page_url_template)
            .bind(merged.page_start)
            .bind(merged.page_end)
            .bind(&new_status)
            .bind(next_run_at)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| map_unique_err(e, &merged.name))?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_tasks SET name=$1, enabled=$2, list_urls=$3, \
                 two_stage=$4, interval_minutes=$5, task_concurrency=$6, user_agent=$7, \
                 request_delay_ms=$8, proxy=$9, auto_link_check=$10, block_detection_config=$11, \
                 max_consecutive_failures=$12, pagination_selector=$13, max_pages=$14, max_pagination_depth=$15, \
                 force_full_collect=$16, page_url_template=$17, page_start=$18, page_end=$19, \
                 status=$20, next_run_at=$21, updated_at=$22 WHERE id=$23",
            )
            .bind(&merged.name)
            .bind(merged.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：强制 true
            .bind(true)
            .bind(merged.interval_minutes)
            .bind(merged.task_concurrency)
            .bind(merged.user_agent.as_deref())
            .bind(merged.request_delay_ms)
            .bind(merged.proxy.as_deref())
            .bind(merged.auto_link_check)
            .bind(merged.block_detection_config.as_deref())
            .bind(merged.max_consecutive_failures)
            .bind(merged.pagination_selector.as_deref())
            .bind(merged.max_pages)
            .bind(merged.max_pagination_depth)
            .bind(merged.force_full_collect)
            .bind(&merged.page_url_template)
            .bind(merged.page_start)
            .bind(merged.page_end)
            .bind(&new_status)
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
    // 045：同一任务正在运行时拒绝重复触发（防止并发抓取）
    if crate::services::crawler::engine::is_task_running(&state.db, id).await {
        return Err(AppError::BadRequest("任务正在运行中，请等待当前运行完成".into()));
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

// 043：旧 test_selectors handler 与 TestSelectorsBody 已删除（直接取代路径）
// 新实现：POST /api/crawler/tasks/field-probe —— 参见 US1 T023（待 Phase 3 添加）

/// `GET /api/crawler/tasks/{id}/export` — 导出 JSON 配置
pub async fn export_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let task = fetch_task(&state, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("爬虫任务 {id} 不存在")))?;
    let mut input = decode_task_to_input(&task)?;
    // 查询字段树并转为可移植结构（id/task_id/parent_id 置 None），塞入导出 JSON
    let tree_model = fetch_field_tree_model(&state, id).await?;
    let portable = db_tree_to_portable_tree(&tree_model)
        .map_err(|e| AppError::Internal(format!("字段树导出失败: {e}")))?;
    input.field_tree = Some(portable);
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
    // 任务字段校验
    body.validate().map_err(|e| {
        AppError::BadRequest(format!("导入配置校验失败: {e}"))
    })?;
    // 字段树校验（若携带）：name 正则 + rule/mode 一致性 + 节点数上限（对齐 create_field_node）
    if let Some(tree) = body.field_tree.as_ref() {
        crate::services::crawler::templates::validate_field_tree(tree)
            .map_err(|e| AppError::BadRequest(format!("字段树校验失败: {e}")))?;
        if flatten_field_tree(tree).len() > 100 {
            return Err(AppError::BadRequest("字段节点总数上限 100".into()));
        }
    }
    let now = chrono::Utc::now().naive_utc();
    let list_urls_json = body.list_urls_json();
    let next_run_at = if body.enabled { Some(now) } else { None };

    let id: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;

            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, max_pagination_depth, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 0, ?)",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：DB 列保留兼容，强制写入 true
            .bind(true)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(body.pagination_selector.as_deref())
            .bind(body.max_pages)
            .bind(body.max_pagination_depth)
            .bind(body.force_full_collect)
            .bind(&body.page_url_template)
            .bind(body.page_start)
            .bind(body.page_end)
            .bind(next_run_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| map_unique_err_tx(e, &body.name))?;
            let new_id = r.last_insert_rowid();

            // 若携带字段树：事务内递归插入（父子关系由 children 嵌套重建，不依赖 DB id）
            if let Some(tree) = body.field_tree.as_ref() {
                insert_template_field_nodes_sqlite(&mut tx, new_id, tree).await?;
            }

            tx.commit()
                .await
                .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
            new_id
        }
        DbPool::Postgres(pool) => {
            let mut tx = pool
                .begin()
                .await
                .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;

            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, max_pagination_depth, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, 'active', 0, $21) \
                 RETURNING id",
            )
            .bind(&body.name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            // two_stage 已下线：DB 列保留兼容，强制写入 true
            .bind(true)
            .bind(body.interval_minutes)
            .bind(body.task_concurrency)
            .bind(body.user_agent.as_deref())
            .bind(body.request_delay_ms)
            .bind(body.proxy.as_deref())
            .bind(body.auto_link_check)
            .bind(body.block_detection_config.as_deref())
            .bind(body.max_consecutive_failures)
            .bind(body.template_source.as_deref())
            .bind(body.pagination_selector.as_deref())
            .bind(body.max_pages)
            .bind(body.max_pagination_depth)
            .bind(body.force_full_collect)
            .bind(&body.page_url_template)
            .bind(body.page_start)
            .bind(body.page_end)
            .bind(next_run_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| map_unique_err_tx(e, &body.name))?;
            let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
            let new_id = id_val.as_i64().unwrap_or(0);

            if let Some(tree) = body.field_tree.as_ref() {
                insert_template_field_nodes_pg(&mut tx, new_id, tree).await?;
            }

            tx.commit()
                .await
                .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
            new_id
        }
    };

    let task = fetch_task(&state, id).await?.ok_or_else(|| {
        AppError::Internal("导入后任务读取失败".into())
    })?;
    Ok(Json(json!({ "success": true, "data": task })))
}

/// `GET /api/crawler/templates` — 旧 042 模板列表（已废弃）
///
/// 043 取代路径：042 内置模板（generic_resource_site / discuz_forum / wordpress_blog）
/// 已删除，本端点暂时返回空数组。US1 T038 将重写为「字段树预置模板」端点
/// `GET /api/crawler/task-templates`。
pub async fn list_templates(
    State(_state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "success": true, "data": [] })))
}

// 043：旧 save_as_template handler 已删除（旧模板结构已删）。US1 T038 引入新模板机制。

/// `GET /api/crawler/task-templates` — 返回内置字段树预置模板列表
///
/// 响应：`{ success: true, data: [{ key, name, description, source_type, field_tree }] }`
pub async fn list_task_templates(
    State(_state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let data = crate::services::crawler::templates::builtin_templates();
    Ok(Json(json!({ "success": true, "data": data })))
}

/// `POST /api/crawler/tasks/from-template` 请求体
#[derive(Deserialize)]
pub struct FromTemplateBody {
    /// 模板 key（如 discuz_forum / wordpress_blog / generic_resource_site）
    pub template_key: String,
    /// 新任务名（必填）
    pub task_name: String,
    /// 列表页 URL（必填，单个）
    pub list_url: String,
    /// 是否启用（默认 false，由用户在 UI 上手动开启）
    #[serde(default)]
    pub enabled: bool,
}

/// `POST /api/crawler/tasks/from-template` — 基于模板创建任务
///
/// 事务内：
/// 1. INSERT `crawler_tasks`（task_name + list_url + template_source=template_key）
/// 2. 展开模板字段树，批量 INSERT `crawler_task_field_nodes`（含父子关系）
///
/// 响应：`{ success: true, data: { id, task: <CrawlerTask>, field_node_count } }`
pub async fn create_task_from_template(
    State(state): State<AppState>,
    Json(body): Json<FromTemplateBody>,
) -> Result<Json<Value>, AppError> {
    // 1. 参数基础校验
    let task_name = body.task_name.trim();
    if task_name.is_empty() {
        return Err(AppError::BadRequest("task_name 不能为空".into()));
    }
    let list_url = body.list_url.trim();
    if list_url.is_empty() {
        return Err(AppError::BadRequest("list_url 不能为空".into()));
    }
    // 任务名合法性沿用 CrawlerTaskInput::validate（避免重复实现）
    let probe_input = CrawlerTaskInput {
        name: task_name.to_string(),
        enabled: body.enabled,
        list_urls: vec![list_url.to_string()],
        two_stage: true,
        interval_minutes: 60,
        task_concurrency: 1,
        user_agent: None,
        request_delay_ms: 0,
        proxy: None,
        auto_link_check: false,
        block_detection_config: None,
        max_consecutive_failures: 3,
        template_source: Some(body.template_key.clone()),
        pagination_selector: None,
        max_pages: 0,
        max_pagination_depth: 10,
        force_full_collect: true,
        page_url_template: String::new(),
        page_start: 1,
        page_end: 0,
        field_tree: None,
    };
    probe_input.validate().map_err(AppError::BadRequest)?;

    // 2. 查找模板
    let template = crate::services::crawler::templates::find_template(&body.template_key)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "template_key='{}' 不存在（可用：discuz_forum / wordpress_blog / generic_resource_site）",
                body.template_key
            ))
        })?;

    // 3. 模板字段树再次校验（防御：编译期已验证，运行时二次确认）
    crate::services::crawler::templates::validate_field_tree(&template.field_tree)
        .map_err(AppError::Internal)?;

    let now = chrono::Utc::now().naive_utc();
    let next_run_at = if body.enabled { Some(now) } else { None };
    let list_urls_json = serde_json::to_string(&vec![list_url.to_string()])
        .map_err(|e| AppError::Internal(format!("list_urls 序列化失败: {e}")))?;

    // 4. 事务：INSERT 任务 + 批量 INSERT 字段节点（保留父子映射）
    let (task_id, node_count) = match &state.db {
        DbPool::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(|e| {
                AppError::Internal(format!("开启事务失败: {e}"))
            })?;

            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES (?, ?, ?, true, 60, NULL, NULL, NULL, NULL, false, NULL, NULL, \
                 ?, NULL, NULL, true, 'active', 0, ?)",
            )
            .bind(task_name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&body.template_key)
            .bind(next_run_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| map_unique_err_tx(e, task_name))?;
            let new_task_id = r.last_insert_rowid();

            let count = insert_template_field_nodes_sqlite(
                &mut tx,
                new_task_id,
                &template.field_tree,
            )
            .await?;

            tx.commit().await.map_err(|e| {
                AppError::Internal(format!("提交事务失败: {e}"))
            })?;
            (new_task_id, count)
        }
        DbPool::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(|e| {
                AppError::Internal(format!("开启事务失败: {e}"))
            })?;

            let r = sqlx::query(
                "INSERT INTO crawler_tasks (name, enabled, list_urls, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, proxy, \
                 auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, \
                 force_full_collect, page_url_template, page_start, page_end, \
                 status, consecutive_failures, next_run_at) \
                 VALUES ($1, $2, $3, true, 60, NULL, NULL, NULL, NULL, false, NULL, NULL, \
                 $4, NULL, NULL, true, 'active', 0, $5) RETURNING id",
            )
            .bind(task_name)
            .bind(body.enabled)
            .bind(&list_urls_json)
            .bind(&body.template_key)
            .bind(next_run_at)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| map_unique_err_tx(e, task_name))?;
            let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
            let new_task_id = id_val.as_i64().unwrap_or(0);

            let count = insert_template_field_nodes_pg(
                &mut tx,
                new_task_id,
                &template.field_tree,
            )
            .await?;

            tx.commit().await.map_err(|e| {
                AppError::Internal(format!("提交事务失败: {e}"))
            })?;
            (new_task_id, count)
        }
    };

    // 5. 读回任务并返回
    let task = fetch_task(&state, task_id)
        .await?
        .ok_or_else(|| AppError::Internal("刚插入的任务读取失败".into()))?;
    tracing::info!(
        target: "crawler",
        "Task created from template '{}': id={task_id} name={task_name} nodes={node_count}",
        body.template_key
    );
    Ok(Json(json!({
        "success": true,
        "data": {
            "id": task_id,
            "task": task,
            "field_node_count": node_count,
        }
    })))
}

/// 把模板字段树扁平化为待插入序列（BFS：根节点在前，子节点紧跟其父后）
///
/// 每项 = `(parent_index: Option<usize>, scope, &FieldTreeNode)`，
/// `parent_index` 指向 `flattened` 中自身的位置（即父节点在结果数组中的索引），
/// 真正的 `parent_id` 由调用方在插入后用此索引回查。
fn flatten_field_tree(
    tree: &crate::services::crawler::field_schema::FieldTree,
) -> Vec<(Option<usize>, crate::services::crawler::field_schema::Scope, &crate::services::crawler::field_schema::FieldTreeNode)> {
    use crate::services::crawler::field_schema::Scope;
    let mut out: Vec<(Option<usize>, Scope, &crate::services::crawler::field_schema::FieldTreeNode)> = Vec::new();

    // 用栈做 DFS：(parent_index_in_out, scope, node)
    // 我们需要「父在子之前」的顺序，所以用「先 push 父，再压子」的逆序栈
    let mut stack: Vec<(Option<usize>, Scope, &crate::services::crawler::field_schema::FieldTreeNode)> = Vec::new();
    // detail_page 先压（出栈晚），list_page 后压（出栈早）；具体顺序无所谓，只要父在子前
    for node in tree.detail_page.iter().rev() {
        stack.push((None, Scope::DetailPage, node));
    }
    for node in tree.list_page.iter().rev() {
        stack.push((None, Scope::ListPage, node));
    }

    while let Some((parent_index, scope, node)) = stack.pop() {
        let my_index = out.len();
        out.push((parent_index, scope, node));
        // 子节点逆序压栈，保证按定义顺序出栈
        for child in node.children.iter().rev() {
            stack.push((Some(my_index), scope, child));
        }
    }

    out
}

/// 把模板字段树展开到 SQLite 事务（迭代 BFS，按父→子顺序插入，parent_id 由 index 回查）
async fn insert_template_field_nodes_sqlite(
    executor: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: i64,
    tree: &crate::services::crawler::field_schema::FieldTree,
) -> Result<i64, AppError> {
    let flat = flatten_field_tree(tree);
    let mut ids: Vec<Option<i64>> = vec![None; flat.len()];

    for (idx, (parent_index, scope, node)) in flat.iter().enumerate() {
        let parent_id = parent_index.and_then(|pi| ids.get(pi).copied().flatten());
        let s = &node.spec;
        let (rule_json, pp_json) = serialize_node(&s.rule, &s.post_processors);
        let r = sqlx::query(
            "INSERT INTO crawler_task_field_nodes \
             (task_id, parent_id, scope, name, display_name, field_type, source_layer, extractor_mode, \
              rule_json, post_processors_json, script_index, sort_order, is_active, refresh_on_read) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(task_id)
        .bind(parent_id)
        .bind(scope.as_str())
        .bind(&s.name)
        .bind(&s.display_name)
        .bind(s.field_type.as_str())
        .bind(s.source_layer.as_str())
        .bind(s.extractor_mode.as_str())
        .bind(&rule_json)
        .bind(pp_json.as_deref())
        .bind(s.script_index)
        .bind(s.sort_order)
        .bind(s.is_active)
        .bind(s.refresh_on_read)
        .execute(&mut **executor)
        .await
        .map_err(map_field_node_unique_err)?;
        ids[idx] = Some(r.last_insert_rowid());
    }

    Ok(flat.len() as i64)
}

/// 把模板字段树展开到 Postgres 事务（迭代 BFS，按父→子顺序插入，parent_id 由 index 回查）
async fn insert_template_field_nodes_pg(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: i64,
    tree: &crate::services::crawler::field_schema::FieldTree,
) -> Result<i64, AppError> {
    let flat = flatten_field_tree(tree);
    let mut ids: Vec<Option<i64>> = vec![None; flat.len()];

    for (idx, (parent_index, scope, node)) in flat.iter().enumerate() {
        let parent_id = parent_index.and_then(|pi| ids.get(pi).copied().flatten());
        let s = &node.spec;
        let (rule_json, pp_json) = serialize_node(&s.rule, &s.post_processors);
        let r = sqlx::query(
            "INSERT INTO crawler_task_field_nodes \
             (task_id, parent_id, scope, name, display_name, field_type, source_layer, extractor_mode, \
              rule_json, post_processors_json, script_index, sort_order, is_active, refresh_on_read) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING id",
        )
        .bind(task_id)
        .bind(parent_id)
        .bind(scope.as_str())
        .bind(&s.name)
        .bind(&s.display_name)
        .bind(s.field_type.as_str())
        .bind(s.source_layer.as_str())
        .bind(s.extractor_mode.as_str())
        .bind(&rule_json)
        .bind(pp_json.as_deref())
        .bind(s.script_index)
        .bind(s.sort_order)
        .bind(s.is_active)
        .bind(s.refresh_on_read)
        .fetch_one(&mut **executor)
        .await
        .map_err(map_field_node_unique_err)?;
        let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
        ids[idx] = Some(id_val.as_i64().unwrap_or(0));
    }

    Ok(flat.len() as i64)
}

/// map_unique_err 的事务版（错误信息一致，仅签名差异在调用点）
fn map_unique_err_tx(e: sqlx::Error, name: &str) -> AppError {
    let msg = e.to_string();
    if msg.contains("UNIQUE") && msg.contains("name") {
        AppError::BadRequest(format!("任务名 '{name}' 已存在"))
    } else {
        AppError::Database(e)
    }
}

// ---------- 公共辅助 ----------

async fn fetch_task(state: &AppState, id: i64) -> Result<Option<CrawlerTask>, AppError> {
    let sql = "SELECT id, name, enabled, list_urls, two_stage, \
     interval_minutes, task_concurrency, user_agent, request_delay_ms, \
     proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
     template_source, pagination_selector, max_pages, max_pagination_depth, \
     force_full_collect, page_url_template, page_start, page_end, \
     status, consecutive_failures, last_run_at, next_run_at, \
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

/// 把 DB 行（list_urls 是 JSON 字符串）解码为 CrawlerTaskInput
fn decode_task_to_input(t: &CrawlerTask) -> Result<CrawlerTaskInput, AppError> {
    let list_urls: Vec<String> = serde_json::from_str(&t.list_urls)
        .map_err(|e| AppError::Internal(format!("list_urls 解析失败: {e}")))?;
    Ok(CrawlerTaskInput {
        name: t.name.clone(),
        enabled: t.enabled,
        list_urls,
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
        pagination_selector: t.pagination_selector.clone(),
        max_pages: t.max_pages,
        max_pagination_depth: t.max_pagination_depth,
        force_full_collect: t.force_full_collect,
        page_url_template: t.page_url_template.clone(),
        page_start: t.page_start,
        page_end: t.page_end,
        // field_tree 由 export_task 单独查询 crawler_task_field_nodes 填充；decode 仅还原任务行
        field_tree: None,
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
                   a.is_edited, a.crawled_at, a.extra_fields_json
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
                   a.is_edited, a.crawled_at, a.extra_fields_json
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
            "list": items.iter().map(|it| {
                let mut v = serde_json::to_value(it).unwrap_or_else(|_| json!({}));
                // 解析 extra_fields_json → extra_fields 对象，方便前端直接渲染
                if let Some(s) = it.extra_fields_json.as_deref()
                    && let Ok(parsed) = serde_json::from_str::<Value>(s) {
                        v["extra_fields"] = parsed;
                    }
                v
            }).collect::<Vec<_>>(),
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
) -> Result<Json<serde_json::Value>, AppError> {
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

    // 5. 字段值长表 + 统计（feature 043）
    let field_values: Vec<crate::models::crawler_field_value::ArticleFieldValueRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::crawler_field_value::ArticleFieldValueRow>(
                "SELECT * FROM crawler_article_field_values WHERE article_id = ? \
                 ORDER BY field_path, value_index, id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::crawler_field_value::ArticleFieldValueRow>(
                "SELECT * FROM crawler_article_field_values WHERE article_id = $1 \
                 ORDER BY field_path, value_index, id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
    };

    let field_stats = crate::models::crawler_field_value::aggregate_stats(&field_values);

    // 6. extra_fields：优先解析 extra_fields_json；为空时从 rows 即时聚合
    let extra_fields: Value = if let Some(json_str) = article.extra_fields_json.as_deref() {
        serde_json::from_str(json_str).unwrap_or_else(|_| build_extra_fields_from_rows(&field_values))
    } else {
        build_extra_fields_from_rows(&field_values)
    };

    Ok(Json(json!({
        "success": true,
        "data": CrawlerArticleDetail {
            article,
            links,
            images,
            task_name,
        },
        "extra_fields": extra_fields,
        "field_stats": field_stats,
        "field_values": field_values,
    })))
}

/// 从 field_values 行构建 extra_fields 嵌套对象（fallback：当 extra_fields_json 为空时）
///
/// 简化策略：把 field_path 的最后一段作为 key，多值合并为 JSON 数组，单值为字符串
fn build_extra_fields_from_rows(
    rows: &[crate::models::crawler_field_value::ArticleFieldValueRow],
) -> Value {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in rows {
        if !r.is_hit {
            continue;
        }
        let key = r
            .field_path
            .rsplit('/')
            .next()
            .unwrap_or(&r.field_path)
            .to_string();
        let v = r
            .value_text
            .clone()
            .map(Value::String)
            .unwrap_or_else(|| {
                r.value_number
                    .map(|n| serde_json::Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null))
                    .unwrap_or(Value::Null)
            });
        map.entry(key).or_default().push(v);
    }
    let mut out = serde_json::Map::new();
    for (k, mut vs) in map {
        if vs.len() == 1 {
            out.insert(k, vs.remove(0));
        } else {
            out.insert(k, Value::Array(vs));
        }
    }
    Value::Object(out)
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
    tracing::info!(
        target: "crawler::batch_delete",
        count = body.ids.len(),
        ids = ?body.ids,
        "batch_delete_articles: incoming"
    );
    if body.ids.is_empty() {
        return Ok(Json(json!({ "success": true, "data": { "deleted": 0, "requested": 0 } })));
    }
    let mut total: u64 = 0;
    for id in &body.ids {
        match &state.db {
            DbPool::Sqlite(pool) => {
                let links_n = sqlx::query("DELETE FROM crawler_article_links WHERE article_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
                let imgs_n = sqlx::query("DELETE FROM crawler_article_images WHERE article_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
                let art_n = sqlx::query("DELETE FROM crawler_articles WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map(|r| r.rows_affected())
                    .unwrap_or(0);
                tracing::info!(
                    target: "crawler::batch_delete",
                    id, art_n, links_n, imgs_n,
                    "row deleted"
                );
                total += art_n;
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
    Ok(Json(json!({ "success": true, "data": { "deleted": total, "requested": body.ids.len() } })))
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

// ============================================================================
// Feature 043 — Visual Field Configurator (US1 T022-T025)
// ============================================================================

use crate::models::crawler_field_library::{FieldLibraryRow, FieldLibraryCategory};
use crate::models::crawler_field_node::{FieldNodeRow, FieldNodeSpecView, FieldTree as FieldTreeModel, from_rows as field_tree_from_rows};
use crate::services::crawler::field_schema::{self, validate_name, validate_rule, ExtractorMode, Scope, SourceLayer};
use crate::services::crawler::probe::{self, ProbeRequest, ProbeResponse};
use crate::services::crawler::source_layer::{self, ProbeCategory, ProbeError};

// -------------------- T022: fetch-source --------------------

#[derive(Debug, Deserialize)]
pub struct FetchSourceRequest {
    pub url: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
}

/// `POST /api/crawler/tasks/fetch-source` — 抓取一个 URL 的 4 tab 源码素材
pub async fn fetch_source(
    State(_state): State<AppState>,
    Json(req): Json<FetchSourceRequest>,
) -> Result<Json<Value>, AppError> {
    let material = source_layer::fetch_source_material(
        &req.url,
        req.user_agent.as_deref(),
        req.proxy.as_deref(),
    )
    .await
    .map_err(map_probe_error_to_app)?;

    Ok(Json(json!({ "success": true, "data": material })))
}

// -------------------- T046: fetch-detail-sample (US3) --------------------

#[derive(Debug, Deserialize)]
pub struct FetchDetailSampleRequest {
    pub task_id: i64,
    /// 列表 URL（覆盖任务 list_urls 第一个）
    pub list_url: String,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
}

/// `POST /api/crawler/tasks/fetch-detail-sample` — US3 取详情页样本素材
///
/// 从任务的 list_page 字段树中找首个 URL 类字段，在 list_url 上求命中得到详情链接，
/// 再抓取该详情 URL，返回 `{ detail_url, material }`。
/// 找不到 URL 类字段或 0 命中 → ParentEmpty（contracts C2）。
pub async fn fetch_detail_sample(
    State(state): State<AppState>,
    Json(req): Json<FetchDetailSampleRequest>,
) -> Result<Json<Value>, AppError> {
    // 取任务的 list_page 字段树
    if fetch_task(&state, req.task_id).await?.is_none() {
        return Err(AppError::NotFound(format!("任务 {} 不存在", req.task_id)));
    }
    let rows: Vec<FieldNodeRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = ? AND scope = 'list_page' ORDER BY sort_order, id",
            )
            .bind(req.task_id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = $1 AND scope = 'list_page' ORDER BY sort_order, id",
            )
            .bind(req.task_id)
            .fetch_all(pool)
            .await?
        }
    };
    let tree_model = field_tree_from_rows(rows);
    let list_nodes = &tree_model.list_page;

    let (detail_url, material) = source_layer::fetch_detail_sample(
        list_nodes,
        &req.list_url,
        req.user_agent.as_deref(),
        req.proxy.as_deref(),
    )
    .await
    .map_err(map_probe_error_to_app)?;

    Ok(Json(json!({
        "success": true,
        "data": {
            "detail_url": detail_url,
            "material": material,
        }
    })))
}

/// 把 ProbeError 映射到 AppError（保持 category/message/hint 给前端消费）
fn map_probe_error_to_app(e: ProbeError) -> AppError {
    match e.category {
        ProbeCategory::InvalidRule => AppError::BadRequest(e.message),
        ProbeCategory::ZeroHits | ProbeCategory::ParentEmpty => AppError::BadRequest(e.message),
        ProbeCategory::UrlUnreachable => AppError::BadRequest(e.message),
        ProbeCategory::Http4xx5xx => AppError::BadRequest(format!("目标返回错误：{}", e.message)),
        ProbeCategory::Blocked => AppError::BadRequest(format!("目标反爬拦截：{}", e.message)),
    }
}

// -------------------- T023: field-probe --------------------

/// `POST /api/crawler/tasks/field-probe` — 字段验证探针
///
/// US2 T043 扩展：当请求体含 `parent_node_id` 时，先查
/// `crawler_task_field_nodes` 取父节点规则/source_layer/post_processors/script_index，
/// 填入 `req.parent_field` 后清空 `parent_node_id`，再调用 `run_probe`。
/// 父节点不存在或非同 task → 400 BadRequest；
/// 父节点 rule_json 解析失败 → 500；
/// 父字段 0 命中 → 由 run_probe 返回 `ParentEmpty`（contracts C2 错误分类）。
pub async fn field_probe(
    State(state): State<AppState>,
    Json(mut req): Json<ProbeRequest>,
) -> Result<Json<Value>, AppError> {
    // US2：解析 parent_node_id → 查父节点 → 填 parent_field
    if let Some(parent_node_id) = req.parent_node_id.take() {
        let parent_row: FieldNodeRow = fetch_field_node(&state, parent_node_id)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "父字段节点 {parent_node_id} 不存在"
                ))
            })?;
        let spec = parent_row.to_spec().map_err(|e| {
            AppError::Internal(format!("父节点 rule 解析失败: {e}"))
        })?;
        req.parent_field = Some(probe::ParentFieldDef {
            source_layer: spec.source_layer,
            rule: spec.rule,
            post_processors: spec.post_processors,
            script_index: spec.script_index,
        });
    }

    let resp: ProbeResponse = probe::run_probe(req).await.map_err(map_probe_error_to_app)?;
    Ok(Json(json!({ "success": true, "data": resp })))
}

// -------------------- T024: field-library --------------------

/// `GET /api/crawler/field-library` — 预置字段库（按 category 分组）
pub async fn list_field_library(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<FieldLibraryRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldLibraryRow>(
                "SELECT * FROM crawler_field_library ORDER BY category, sort_order, id",
            )
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldLibraryRow>(
                "SELECT * FROM crawler_field_library ORDER BY category, sort_order, id",
            )
            .fetch_all(pool)
            .await?
        }
    };

    // 分组（直接传 Vec，model 内部按 category 分桶）
    let grouped: Vec<FieldLibraryCategory> = crate::models::crawler_field_library::group_by_category(rows);

    Ok(Json(json!({ "success": true, "data": grouped })))
}

// -------------------- T025: field-tree CRUD --------------------

/// `GET /api/crawler/tasks/{id}/field-tree` — 返回任务字段树（list_page + detail_page 双根）
pub async fn get_field_tree(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    // 任务必须存在
    if fetch_task(&state, id).await?.is_none() {
        return Err(AppError::NotFound(format!("任务 {id} 不存在")));
    }

    let rows: Vec<FieldNodeRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = ? ORDER BY scope, sort_order, id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = $1 ORDER BY scope, sort_order, id",
            )
            .bind(id)
            .fetch_all(pool)
            .await?
        }
    };

    // 转换为应用层 spec 视图（解析 rule_json/post_processors_json）
    // 容错：单条节点解析失败时仍返回树，但标记该节点错误
    let tree_db: FieldTreeModel = field_tree_from_rows(rows);
    let tree_spec = db_tree_to_spec(&tree_db);

    Ok(Json(json!({ "success": true, "data": tree_spec })))
}

/// 把 DB 层 FieldTree 转为应用层 FieldTreeSpec（每节点附 spec，失败时 spec 字段为 null + error）
fn db_tree_to_spec(tree: &FieldTreeModel) -> Value {
    fn convert_node(node: &crate::models::crawler_field_node::FieldTreeNode) -> Value {
        let spec_view: Result<FieldNodeSpecView, String> = node.row.to_spec();
        let children: Vec<Value> = node.children.iter().map(convert_node).collect();
        match spec_view {
            Ok(spec) => json!({
                "spec": spec,
                "children": children,
            }),
            Err(err) => json!({
                "spec": null,
                "error": err,
                "row": node.row,
                "children": children,
            }),
        }
    }
    json!({
        "list_page": tree.list_page.iter().map(convert_node).collect::<Vec<_>>(),
        "detail_page": tree.detail_page.iter().map(convert_node).collect::<Vec<_>>(),
    })
}

/// 导出用：把 DB 层 FieldTree（含真实 id/task_id/parent_id）转为「可移植」应用层 FieldTree
/// —— id/task_id/parent_id 一律置 None（导入端靠 children 嵌套重建父子关系，本就忽略这些）。
/// 任一节点 rule_json/post_processors_json 解析失败 → 整体 Err（fail-fast，不静默丢节点）。
fn db_tree_to_portable_tree(tree: &FieldTreeModel) -> Result<field_schema::FieldTree, String> {
    use crate::models::crawler_field_node::FieldTreeNode as ModelNode;

    fn convert(node: &ModelNode) -> Result<field_schema::FieldTreeNode, String> {
        let v = node.row.to_spec()?;
        let spec = field_schema::FieldNodeSpec {
            id: None,
            task_id: None,
            parent_id: None,
            scope: v.scope,
            name: v.name,
            display_name: v.display_name,
            field_type: v.field_type,
            source_layer: v.source_layer,
            extractor_mode: v.extractor_mode,
            rule: v.rule,
            post_processors: v.post_processors,
            script_index: v.script_index,
            sort_order: v.sort_order,
            is_active: v.is_active,
            refresh_on_read: v.refresh_on_read,
        };
        let children = node
            .children
            .iter()
            .map(convert)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(field_schema::FieldTreeNode { spec, children })
    }

    let list_page = tree
        .list_page
        .iter()
        .map(convert)
        .collect::<Result<Vec<_>, _>>()?;
    let detail_page = tree
        .detail_page
        .iter()
        .map(convert)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(field_schema::FieldTree { list_page, detail_page })
}

/// 查询任务的字段节点并组装为 DB 层 FieldTree（list_page + detail_page 双根，含父子嵌套）。
/// export_task 与 get_field_tree 共用此查询。
async fn fetch_field_tree_model(state: &AppState, task_id: i64) -> Result<FieldTreeModel, AppError> {
    let rows: Vec<FieldNodeRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = ? ORDER BY scope, sort_order, id",
            )
            .bind(task_id)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = $1 ORDER BY scope, sort_order, id",
            )
            .bind(task_id)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(field_tree_from_rows(rows))
}

/// 字段节点 UNIQUE(task_id, scope, parent_id, name) 违例 → 友好 BadRequest；其他 DB 错误透传。
/// 导入事务回滚保证无脏数据。
fn map_field_node_unique_err(e: sqlx::Error) -> AppError {
    if e.to_string().contains("UNIQUE") {
        AppError::BadRequest("字段节点 name 在同 scope + 同 parent 下已存在".into())
    } else {
        AppError::Database(e)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateFieldNodeBody {
    pub parent_id: Option<i64>,
    pub scope: Scope,
    pub name: String,
    pub display_name: String,
    pub source_layer: SourceLayer,
    pub extractor_mode: ExtractorMode,
    /// 直接传 Rule（应用层 tagged enum）
    pub rule: field_schema::Rule,
    #[serde(default)]
    pub post_processors: Vec<field_schema::PostProcessor>,
    #[serde(default)]
    pub script_index: Option<i32>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true_bool")]
    pub is_active: bool,
    /// [feature 046] 仅 extractor_mode=script 时允许 true（validate_field_node_spec 强制）
    #[serde(default)]
    pub refresh_on_read: bool,
    /// field_type（默认按 SourceLayer 推断：url/url → url，image → image，其余 string）
    #[serde(default)]
    pub field_type: Option<field_schema::FieldType>,
}

fn default_true_bool() -> bool {
    true
}

/// `POST /api/crawler/tasks/{id}/field-nodes` — 新增字段节点
pub async fn create_field_node(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CreateFieldNodeBody>,
) -> Result<Json<Value>, AppError> {
    // 任务存在
    if fetch_task(&state, id).await?.is_none() {
        return Err(AppError::NotFound(format!("任务 {id} 不存在")));
    }

    // name 合法性
    validate_name(&body.name).map_err(AppError::BadRequest)?;

    // rule 与 mode 一致性：序列化 rule 再用 mode 反序列化验证
    let (mode_str, rule_inner_json) = field_schema::serialize_rule(&body.rule);
    if mode_str != body.extractor_mode.as_str() {
        return Err(AppError::BadRequest(format!(
            "extractor_mode({}) 与 rule.mode({}) 不一致",
            body.extractor_mode.as_str(),
            mode_str
        )));
    }
    validate_rule(body.extractor_mode, &rule_inner_json).map_err(AppError::BadRequest)?;

    // parent_id 校验：必须属于同任务 + 同 scope
    if let Some(pid) = body.parent_id {
        let parent = fetch_field_node(&state, pid).await?;
        let parent = parent.ok_or_else(|| AppError::BadRequest(format!("父节点 {pid} 不存在")))?;
        if parent.task_id != id {
            return Err(AppError::BadRequest("父节点不属于该任务".into()));
        }
        if parent.scope != body.scope.as_str() {
            return Err(AppError::BadRequest(format!(
                "父节点 scope={} 与子节点 scope={} 不一致",
                parent.scope,
                body.scope.as_str()
            )));
        }
    }

    // 节点数 ≤ 100
    let node_count = count_field_nodes(&state, id).await?;
    if node_count >= 100 {
        return Err(AppError::BadRequest(
            "字段节点总数已达上限（100），无法新增".into(),
        ));
    }

    // UNIQUE(task_id, scope, parent_id, name) — DB 会兜底，提前查可给出更友好错误
    if field_node_name_exists(&state, id, body.parent_id, body.scope.as_str(), &body.name).await? {
        return Err(AppError::BadRequest(format!(
            "同 scope + 同 parent 下已存在 name='{}' 的节点",
            body.name
        )));
    }

    let field_type = body
        .field_type
        .unwrap_or_else(|| infer_field_type(&body.source_layer, &body.extractor_mode));

    let (rule_json, pp_json) = serialize_node(&body.rule, &body.post_processors);

    // [feature 046] 字段树一致性校验：list_page + script 拒绝；refresh_on_read 仅 script 允许
    let spec_for_check = field_schema::FieldNodeSpec {
        id: None,
        task_id: Some(id),
        parent_id: body.parent_id,
        scope: body.scope,
        name: body.name.clone(),
        display_name: body.display_name.clone(),
        field_type,
        source_layer: body.source_layer,
        extractor_mode: body.extractor_mode,
        rule: body.rule.clone(),
        post_processors: body.post_processors.clone(),
        script_index: body.script_index,
        sort_order: body.sort_order,
        is_active: body.is_active,
        refresh_on_read: body.refresh_on_read,
    };
    field_schema::validate_field_node_spec(&spec_for_check).map_err(AppError::BadRequest)?;

    let new_id: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO crawler_task_field_nodes \
                 (task_id, parent_id, scope, name, display_name, field_type, source_layer, extractor_mode, \
                  rule_json, post_processors_json, script_index, sort_order, is_active, refresh_on_read) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(id)
            .bind(body.parent_id)
            .bind(body.scope.as_str())
            .bind(&body.name)
            .bind(&body.display_name)
            .bind(field_type.as_str())
            .bind(body.source_layer.as_str())
            .bind(body.extractor_mode.as_str())
            .bind(&rule_json)
            .bind(pp_json.as_deref())
            .bind(body.script_index)
            .bind(body.sort_order)
            .bind(body.is_active)
            .bind(body.refresh_on_read)
            .fetch_one(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO crawler_task_field_nodes \
                 (task_id, parent_id, scope, name, display_name, field_type, source_layer, extractor_mode, \
                  rule_json, post_processors_json, script_index, sort_order, is_active, refresh_on_read) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING id",
            )
            .bind(id)
            .bind(body.parent_id)
            .bind(body.scope.as_str())
            .bind(&body.name)
            .bind(&body.display_name)
            .bind(field_type.as_str())
            .bind(body.source_layer.as_str())
            .bind(body.extractor_mode.as_str())
            .bind(&rule_json)
            .bind(pp_json.as_deref())
            .bind(body.script_index)
            .bind(body.sort_order)
            .bind(body.is_active)
            .bind(body.refresh_on_read)
            .fetch_one(pool)
            .await?
        }
    };

    Ok(Json(json!({
        "success": true,
        "data": { "id": new_id, "task_id": id }
    })))
}

/// `PUT /api/crawler/tasks/{id}/field-nodes/{node_id}` — 更新字段节点
pub async fn update_field_node(
    State(state): State<AppState>,
    Path((id, node_id)): Path<(i64, i64)>,
    Json(body): Json<CreateFieldNodeBody>,
) -> Result<Json<Value>, AppError> {
    let existing = fetch_field_node(&state, node_id).await?
        .ok_or_else(|| AppError::NotFound(format!("节点 {node_id} 不存在")))?;
    if existing.task_id != id {
        return Err(AppError::BadRequest("节点不属于该任务".into()));
    }

    validate_name(&body.name).map_err(AppError::BadRequest)?;

    let (mode_str, rule_inner_json) = field_schema::serialize_rule(&body.rule);
    if mode_str != body.extractor_mode.as_str() {
        return Err(AppError::BadRequest(format!(
            "extractor_mode({}) 与 rule.mode({}) 不一致",
            body.extractor_mode.as_str(),
            mode_str
        )));
    }
    validate_rule(body.extractor_mode, &rule_inner_json).map_err(AppError::BadRequest)?;

    // scope/parent_id 不能跨 scope 变更（保持父子一致）
    if let Some(pid) = body.parent_id {
        let parent = fetch_field_node(&state, pid).await?
            .ok_or_else(|| AppError::BadRequest(format!("父节点 {pid} 不存在")))?;
        if parent.task_id != id {
            return Err(AppError::BadRequest("父节点不属于该任务".into()));
        }
        if parent.scope != body.scope.as_str() {
            return Err(AppError::BadRequest("父节点 scope 与子节点 scope 不一致".into()));
        }
    }
    // 自引用检查
    if body.parent_id == Some(node_id) {
        return Err(AppError::BadRequest("节点不能以自己为父".into()));
    }

    // UNIQUE 检查（排除自己）
    if field_node_name_exists_exclude(&state, id, body.parent_id, body.scope.as_str(), &body.name, node_id).await? {
        return Err(AppError::BadRequest(format!(
            "同 scope + 同 parent 下已存在 name='{}' 的节点",
            body.name
        )));
    }

    let field_type = body
        .field_type
        .unwrap_or_else(|| infer_field_type(&body.source_layer, &body.extractor_mode));
    let (rule_json, pp_json) = serialize_node(&body.rule, &body.post_processors);

    // [feature 046] 字段树一致性校验：list_page + script 拒绝；refresh_on_read 仅 script 允许
    let spec_for_check = field_schema::FieldNodeSpec {
        id: Some(node_id),
        task_id: Some(id),
        parent_id: body.parent_id,
        scope: body.scope,
        name: body.name.clone(),
        display_name: body.display_name.clone(),
        field_type,
        source_layer: body.source_layer,
        extractor_mode: body.extractor_mode,
        rule: body.rule.clone(),
        post_processors: body.post_processors.clone(),
        script_index: body.script_index,
        sort_order: body.sort_order,
        is_active: body.is_active,
        refresh_on_read: body.refresh_on_read,
    };
    field_schema::validate_field_node_spec(&spec_for_check).map_err(AppError::BadRequest)?;

    let updated: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_task_field_nodes SET \
                 parent_id = ?, scope = ?, name = ?, display_name = ?, field_type = ?, \
                 source_layer = ?, extractor_mode = ?, rule_json = ?, post_processors_json = ?, \
                 script_index = ?, sort_order = ?, is_active = ?, refresh_on_read = ?, \
                 updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ?",
            )
            .bind(body.parent_id)
            .bind(body.scope.as_str())
            .bind(&body.name)
            .bind(&body.display_name)
            .bind(field_type.as_str())
            .bind(body.source_layer.as_str())
            .bind(body.extractor_mode.as_str())
            .bind(&rule_json)
            .bind(pp_json.as_deref())
            .bind(body.script_index)
            .bind(body.sort_order)
            .bind(body.is_active)
            .bind(body.refresh_on_read)
            .bind(node_id)
            .execute(pool)
            .await?
            .rows_affected() as i64
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_task_field_nodes SET \
                 parent_id = $1, scope = $2, name = $3, display_name = $4, field_type = $5, \
                 source_layer = $6, extractor_mode = $7, rule_json = $8, post_processors_json = $9, \
                 script_index = $10, sort_order = $11, is_active = $12, refresh_on_read = $13, \
                 updated_at = CURRENT_TIMESTAMP \
                 WHERE id = $14",
            )
            .bind(body.parent_id)
            .bind(body.scope.as_str())
            .bind(&body.name)
            .bind(&body.display_name)
            .bind(field_type.as_str())
            .bind(body.source_layer.as_str())
            .bind(body.extractor_mode.as_str())
            .bind(&rule_json)
            .bind(pp_json.as_deref())
            .bind(body.script_index)
            .bind(body.sort_order)
            .bind(body.is_active)
            .bind(body.refresh_on_read)
            .bind(node_id)
            .execute(pool)
            .await?
            .rows_affected() as i64
        }
    };

    if updated == 0 {
        return Err(AppError::NotFound(format!("节点 {node_id} 不存在")));
    }

    Ok(Json(json!({ "success": true, "data": { "id": node_id } })))
}

#[derive(Debug, Deserialize)]
pub struct ReorderBody {
    pub parent_id: Option<i64>,
    pub scope: Scope,
    pub ordered_ids: Vec<i64>,
}

/// `PUT /api/crawler/tasks/{id}/field-nodes/reorder` — 批量更新 sort_order
pub async fn reorder_field_nodes(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReorderBody>,
) -> Result<Json<Value>, AppError> {
    if body.ordered_ids.is_empty() {
        return Ok(Json(json!({ "success": true, "data": { "updated": 0 } })));
    }
    let mut updated = 0i64;
    for (sort_order, node_id) in body.ordered_ids.iter().enumerate() {
        let n = match &state.db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "UPDATE crawler_task_field_nodes SET sort_order = ?, updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ? AND task_id = ? AND scope = ? AND \
                     (parent_id IS ? OR (parent_id = ? AND ? IS NOT NULL))",
                )
                .bind(sort_order as i32)
                .bind(node_id)
                .bind(id)
                .bind(body.scope.as_str())
                .bind(body.parent_id)
                .bind(body.parent_id)
                .bind(body.parent_id)
                .execute(pool)
                .await?
                .rows_affected() as i64
            }
            DbPool::Postgres(pool) => {
                sqlx::query(
                    "UPDATE crawler_task_field_nodes SET sort_order = $1, updated_at = CURRENT_TIMESTAMP \
                     WHERE id = $2 AND task_id = $3 AND scope = $4 AND \
                     (parent_id IS $5 OR (parent_id = $6 AND $5 IS NOT NULL))",
                )
                .bind(sort_order as i32)
                .bind(node_id)
                .bind(id)
                .bind(body.scope.as_str())
                .bind(body.parent_id)
                .bind(body.parent_id)
                .bind(body.parent_id)
                .execute(pool)
                .await?
                .rows_affected() as i64
            }
        };
        updated += n;
    }
    Ok(Json(json!({ "success": true, "data": { "updated": updated } })))
}

/// `DELETE /api/crawler/tasks/{id}/field-nodes/{node_id}` — 删除节点（级联删子孙）
///
/// 由于 DB 外键 `ON DELETE CASCADE` 设置在 parent_id 自引用上，
/// 删除父节点时数据库会自动级联删除所有子孙。这里返回 children 计数（不含自身）。
pub async fn delete_field_node(
    State(state): State<AppState>,
    Path((id, node_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, AppError> {
    // 先统计子节点数（删除前）
    let child_count: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            // SQLite 递归 CTE 找所有后代
            sqlx::query_scalar::<_, i64>(
                "WITH RECURSIVE descendants(id) AS (\
                   SELECT id FROM crawler_task_field_nodes WHERE parent_id = ? \
                   UNION ALL \
                   SELECT n.id FROM crawler_task_field_nodes n JOIN descendants d ON n.parent_id = d.id\
                 ) SELECT COUNT(*) FROM descendants",
            )
            .bind(node_id)
            .fetch_one(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar::<_, i64>(
                "WITH RECURSIVE descendants(id) AS (\
                   SELECT id FROM crawler_task_field_nodes WHERE parent_id = $1 \
                   UNION ALL \
                   SELECT n.id FROM crawler_task_field_nodes n JOIN descendants d ON n.parent_id = d.id\
                 ) SELECT COUNT(*) FROM descendants",
            )
            .bind(node_id)
            .fetch_one(pool)
            .await?
        }
    };

    let deleted: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM crawler_task_field_nodes WHERE id = ? AND task_id = ?")
                .bind(node_id)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected() as i64
        }
        DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM crawler_task_field_nodes WHERE id = $1 AND task_id = $2")
                .bind(node_id)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected() as i64
        }
    };

    if deleted == 0 {
        return Err(AppError::NotFound(format!("节点 {node_id} 不存在")));
    }

    Ok(Json(json!({
        "success": true,
        "data": { "deleted_children": child_count }
    })))
}

// -------------------- field-node 辅助 --------------------

async fn fetch_field_node(
    state: &AppState,
    node_id: i64,
) -> Result<Option<FieldNodeRow>, AppError> {
    Ok(match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldNodeRow>("SELECT * FROM crawler_task_field_nodes WHERE id = ?")
                .bind(node_id)
                .fetch_optional(pool)
                .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldNodeRow>("SELECT * FROM crawler_task_field_nodes WHERE id = $1")
                .bind(node_id)
                .fetch_optional(pool)
                .await?
        }
    })
}

async fn count_field_nodes(state: &AppState, task_id: i64) -> Result<i64, AppError> {
    Ok(match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes WHERE task_id = ?",
            )
            .bind(task_id)
            .fetch_one(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes WHERE task_id = $1",
            )
            .bind(task_id)
            .fetch_one(pool)
            .await?
        }
    })
}

async fn field_node_name_exists(
    state: &AppState,
    task_id: i64,
    parent_id: Option<i64>,
    scope: &str,
    name: &str,
) -> Result<bool, AppError> {
    let n: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            let q = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes \
                 WHERE task_id = ? AND scope = ? AND name = ? AND \
                 (parent_id IS ? OR (parent_id = ? AND ? IS NOT NULL))",
            )
            .bind(task_id)
            .bind(scope)
            .bind(name)
            .bind(parent_id)
            .bind(parent_id)
            .bind(parent_id);
            q.fetch_one(pool).await?
        }
        DbPool::Postgres(pool) => {
            let q = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes \
                 WHERE task_id = $1 AND scope = $2 AND name = $3 AND \
                 (parent_id IS $4 OR (parent_id = $5 AND $4 IS NOT NULL))",
            )
            .bind(task_id)
            .bind(scope)
            .bind(name)
            .bind(parent_id)
            .bind(parent_id);
            q.fetch_one(pool).await?
        }
    };
    Ok(n > 0)
}

async fn field_node_name_exists_exclude(
    state: &AppState,
    task_id: i64,
    parent_id: Option<i64>,
    scope: &str,
    name: &str,
    exclude_id: i64,
) -> Result<bool, AppError> {
    let n: i64 = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes \
                 WHERE task_id = ? AND scope = ? AND name = ? AND id != ? AND \
                 (parent_id IS ? OR (parent_id = ? AND ? IS NOT NULL))",
            )
            .bind(task_id)
            .bind(scope)
            .bind(name)
            .bind(exclude_id)
            .bind(parent_id)
            .bind(parent_id)
            .bind(parent_id)
            .fetch_one(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM crawler_task_field_nodes \
                 WHERE task_id = $1 AND scope = $2 AND name = $3 AND id != $4 AND \
                 (parent_id IS $5 OR (parent_id = $6 AND $5 IS NOT NULL))",
            )
            .bind(task_id)
            .bind(scope)
            .bind(name)
            .bind(exclude_id)
            .bind(parent_id)
            .bind(parent_id)
            .fetch_one(pool)
            .await?
        }
    };
    Ok(n > 0)
}

/// 序列化 rule + post_processors 落库
fn serialize_node(
    rule: &field_schema::Rule,
    post_processors: &[field_schema::PostProcessor],
) -> (String, Option<String>) {
    let (_mode, rule_inner) = field_schema::serialize_rule(rule);
    let pp_json = if post_processors.is_empty() {
        None
    } else {
        Some(serde_json::to_string(post_processors).unwrap_or_else(|_| "[]".to_string()))
    };
    (rule_inner, pp_json)
}

/// 按 SourceLayer + Mode 推断 FieldType（用户没传 field_type 时）
fn infer_field_type(layer: &SourceLayer, _mode: &ExtractorMode) -> field_schema::FieldType {
    match layer {
        SourceLayer::Url => field_schema::FieldType::Url,
        _ => field_schema::FieldType::String,
    }
}

// ============================================================================
// T058 — 字段命中率统计（FR-027, contracts C7）
// ============================================================================

/// `GET /api/crawler/tasks/{id}/field-stats?days=30` 查询参数
#[derive(Debug, Deserialize)]
pub struct FieldStatsParams {
    /// 统计窗口（天），默认 30，最小 1
    pub days: Option<i64>,
}

/// 单字段聚合行（DB 查询结果）
#[derive(Debug, sqlx::FromRow)]
struct FieldStatRow {
    field_node_id: Option<i64>,
    field_path: String,
    field_name: Option<String>,
    field_display_name: Option<String>,
    total_articles: i64,
    hit_articles: i64,
}

/// 将 hit_rate 映射到状态：≥0.80 healthy / 0.10~0.80 degraded / <0.10 stale_warning
fn classify_status(hit_rate: f64) -> &'static str {
    if hit_rate >= 0.80 {
        "healthy"
    } else if hit_rate >= 0.10 {
        "degraded"
    } else {
        "stale_warning"
    }
}

/// `GET /api/crawler/tasks/{id}/field-stats?days=30`
///
/// 聚合 `crawler_article_field_values` 近 N 天命中率，按字段返回
/// `{ field_node_id, field_path, field_name, total_articles, hit_articles, hit_rate, status }`。
pub async fn get_task_field_stats(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<FieldStatsParams>,
) -> Result<Json<Value>, AppError> {
    // 任务必须存在
    if fetch_task(&state, id).await?.is_none() {
        return Err(AppError::NotFound(format!("任务 {id} 不存在")));
    }

    let days = params.days.unwrap_or(30).max(1);
    let since = chrono::Utc::now().naive_utc() - chrono::Duration::days(days);

    // 注意：bare `fav.is_hit` 在 SQLite（整数 0/1）与 Postgres（boolean）下均可作为 truthy 使用。
    // COUNT(DISTINCT article_id) 在多值字段（同 article 多 value_index）下仍按文章维度去重。
    let rows: Vec<FieldStatRow> = match &state.db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldStatRow>(
                "SELECT fav.field_node_id AS field_node_id, \
                        fav.field_path     AS field_path, \
                        fn.name            AS field_name, \
                        fn.display_name    AS field_display_name, \
                        COUNT(DISTINCT fav.article_id) AS total_articles, \
                        COUNT(DISTINCT CASE WHEN fav.is_hit THEN fav.article_id END) AS hit_articles \
                 FROM crawler_article_field_values fav \
                 JOIN crawler_articles a ON fav.article_id = a.id \
                 LEFT JOIN crawler_task_field_nodes fn ON fav.field_node_id = fn.id \
                 WHERE a.task_id = ? AND fav.created_at >= ? \
                 GROUP BY fav.field_node_id, fav.field_path, fn.name, fn.display_name \
                 ORDER BY fav.field_path",
            )
            .bind(id)
            .bind(since)
            .fetch_all(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldStatRow>(
                "SELECT fav.field_node_id AS field_node_id, \
                        fav.field_path     AS field_path, \
                        fn.name            AS field_name, \
                        fn.display_name    AS field_display_name, \
                        COUNT(DISTINCT fav.article_id) AS total_articles, \
                        COUNT(DISTINCT CASE WHEN fav.is_hit THEN fav.article_id END) AS hit_articles \
                 FROM crawler_article_field_values fav \
                 JOIN crawler_articles a ON fav.article_id = a.id \
                 LEFT JOIN crawler_task_field_nodes fn ON fav.field_node_id = fn.id \
                 WHERE a.task_id = $1 AND fav.created_at >= $2 \
                 GROUP BY fav.field_node_id, fav.field_path, fn.name, fn.display_name \
                 ORDER BY fav.field_path",
            )
            .bind(id)
            .bind(since)
            .fetch_all(pool)
            .await?
        }
    };

    let stats: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let total = r.total_articles.max(0) as f64;
            let hits = r.hit_articles.max(0) as f64;
            let hit_rate = if total > 0.0 { hits / total } else { 0.0 };
            json!({
                "field_node_id": r.field_node_id,
                "field_path": r.field_path,
                "field_name": r.field_name,
                "field_display_name": r.field_display_name,
                "total_articles": r.total_articles,
                "hit_articles": r.hit_articles,
                "hit_rate": (hit_rate * 100.0).round() / 100.0,
                "status": classify_status(hit_rate),
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "data": {
            "window_days": days,
            "stats": stats,
        }
    })))
}

// ============================================================================
// [feature 046 US4] 手动刷新文章字段（仅 script 模式 + admin 权限）
// ============================================================================

/// POST `/api/crawler/articles/:article_id/fields/:field_name/refresh`
///
/// 强制重跑指定脚本字段（force_refresh=Some(true)），写回 final_value。
/// 鉴权：admin_guard 已保证 role >= 10。
pub async fn refresh_article_field(
    State(state): State<AppState>,
    Path((article_id, field_name)): Path<(i64, String)>,
) -> Result<Json<Value>, AppError> {
    use crate::services::crawler::refresh;

    let db = state.db.clone();
    // http_client：复用任务级 UA/proxy。get_article_field_for_use 内部会加载 task ua/proxy。
    // 为简化实现 + 复用连接池，此处 None（refresh 函数自身会在需要时加载 task UA/proxy）；
    // 但 ctx.fetch 仍需要外部 client，故这里也尝试构造一个简单 client 作为兜底。
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok();

    let result = refresh::get_article_field_for_use(
        article_id,
        &field_name,
        Some(true),
        &db,
        client.as_ref(),
    )
    .await
    .map_err(|e| match e {
        refresh::RefreshError::ArticleNotFound { article_id } => {
            AppError::NotFound(format!("文章 {article_id} 不存在"))
        }
        refresh::RefreshError::FieldNodeNotFound { field_name, .. } => {
            AppError::NotFound(format!("字段 {field_name} 不存在"))
        }
        refresh::RefreshError::NotScriptField { field_name, mode } => AppError::BadRequest(
            format!("字段 {field_name} 不是 script 模式（mode={mode}），不可刷新"),
        ),
        refresh::RefreshError::ScriptFailed { category, message } => AppError::Internal(format!(
            "脚本求值失败 [{category}]: {message}"
        )),
        other => AppError::Internal(other.to_string()),
    })?;

    Ok(Json(json!({
        "data": {
            "old_value": result.old_value,
            "new_value": result.new_value,
            "duration_ms": result.duration_ms,
        }
    })))
}

#[cfg(test)]
mod field_stats_tests {
    use super::*;

    #[test]
    fn classify_healthy() {
        assert_eq!(classify_status(0.80), "healthy");
        assert_eq!(classify_status(0.95), "healthy");
        assert_eq!(classify_status(1.00), "healthy");
    }

    #[test]
    fn classify_degraded() {
        assert_eq!(classify_status(0.10), "degraded");
        assert_eq!(classify_status(0.50), "degraded");
        assert_eq!(classify_status(0.79), "degraded");
    }

    #[test]
    fn classify_stale_warning() {
        assert_eq!(classify_status(0.0), "stale_warning");
        assert_eq!(classify_status(0.04), "stale_warning");
        assert_eq!(classify_status(0.09), "stale_warning");
    }

    #[test]
    fn classify_boundary_0_80_is_healthy() {
        // 0.80 含于 healthy，0.7999 含于 degraded
        assert_eq!(classify_status(0.80), "healthy");
        assert_eq!(classify_status(0.7999), "degraded");
    }

    #[test]
    fn classify_boundary_0_10_is_degraded() {
        // 0.10 含于 degraded，0.099 含于 stale_warning
        assert_eq!(classify_status(0.10), "degraded");
        assert_eq!(classify_status(0.099), "stale_warning");
    }
}
