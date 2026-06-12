// 资源管理 HTTP handlers — 提取触发、列表查询、详情/编辑/删除、统计、提取配置

use crate::errors::AppError;
use crate::models::extracted_resource::UpdateExtractedResource;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct ResourceQueryParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub category: Option<String>,
    pub link_status: Option<String>,
}

#[derive(Deserialize)]
pub struct ExtractParams {
    pub batch_size: Option<i64>,
}

/// POST /api/resources/extract/{history_id} — 单条记录资源提取
pub async fn extract_single(
    State(state): State<AppState>,
    Path(history_id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let dry_run = body
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let extract_mode = {
        let cache = state.option_cache.read().await;
        body.get("extract_mode")
            .and_then(|v| v.as_str())
            .unwrap_or(
                cache
                    .get("push_extract_mode")
                    .map(|s| s.as_str())
                    .unwrap_or("rule"),
            )
            .to_string()
    };

    let result = crate::services::resource::extract_single_record(
        &state.db,
        &state.option_cache,
        history_id,
        dry_run,
        extract_mode,
    )
    .await?;

    // 非 dry_run 时，尝试将含图片的资源入队转发
    if !dry_run
        && let Some(resources) = result
            .get("data")
            .and_then(|d| d.get("resources"))
            .and_then(|r| r.as_array())
    {
        for res in resources {
            // 从提取结果中获取标题和链接，从采集记录中已有 remote_id
            let title = res.get("title").and_then(|v| v.as_str());
            let link = res.get("url").and_then(|v| v.as_str());
            // 获取 remote_id 和 channel_id/message_id
            let (remote_id, ch_id, msg_id) = {
                match &state.db {
                    crate::state::DbPool::Sqlite(pool) => {
                        let row: Option<(Option<String>, Option<i64>, Option<i64>)> = sqlx::query_as(
                                "SELECT remote_id, channel_id, message_id FROM collector_histories WHERE id = ?",
                            )
                            .bind(history_id)
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten();
                        match row {
                            Some((rid, cid, mid)) => (rid, cid, mid),
                            None => continue,
                        }
                    }
                    crate::state::DbPool::Postgres(pool) => {
                        let row: Option<(Option<String>, Option<i64>, Option<i64>)> = sqlx::query_as(
                                "SELECT remote_id, channel_id, message_id FROM collector_histories WHERE id = $1",
                            )
                            .bind(history_id)
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten();
                        match row {
                            Some((rid, cid, mid)) => (rid, cid, mid),
                            None => continue,
                        }
                    }
                }
            };

            if let Some(ref rid) = remote_id
                && !rid.is_empty()
            {
                let desc = res.get("description").and_then(|v| v.as_str());
                if let Err(e) = crate::services::forward_queue::enqueue(
                    &state, rid, ch_id, msg_id, title, desc, link,
                )
                .await
                {
                    tracing::warn!("图片转发入队失败: {e}");
                }
                // 一个 remote_id 只需入队一次
                break;
            }
        }
    }

    Ok(Json(result))
}

/// POST /api/resources/extract — 触发资源提取
pub async fn extract_resources(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let batch_size = body
        .get("batch_size")
        .and_then(|v| v.as_i64())
        .unwrap_or(1000);

    let result = crate::services::resource::trigger_extraction(&state, batch_size).await;

    // 持久化提取历史（成功/失败均记录）
    let (status, scanned, extracted, skipped, errors, msg) = match &result {
        Ok(r) => (
            "success",
            r.total_scanned,
            r.extracted,
            r.skipped,
            r.errors,
            None,
        ),
        Err(e) => ("failed", 0i64, 0i64, 0i64, 0i64, Some(e.to_string())),
    };
    tracing::info!(
        "Manual extract result: status={status}, scanned={scanned}, extracted={extracted}, skipped={skipped}, errors={errors}"
    );
    match crate::services::extract_history::insert(
        &state.db,
        status,
        scanned,
        extracted,
        skipped,
        errors,
        msg.as_deref(),
    )
    .await
    {
        Ok(()) => tracing::info!("Extract history record inserted (manual)"),
        Err(e) => tracing::error!("写入提取历史失败 (manual): {e}"),
    }

    let result = result?;

    Ok(Json(json!({
        "success": true,
        "data": result
    })))
}

/// GET /api/resources — 资源列表（分页 + 筛选）
pub async fn list_resources(
    State(state): State<AppState>,
    Query(params): Query<ResourceQueryParams>,
) -> Result<Json<Value>, AppError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 100);
    let status = params.status.as_deref();
    let category = params.category.as_deref();
    let link_status = params.link_status.as_deref();

    let result = crate::services::resource::list_resources(
        &state.db,
        page,
        page_size,
        status,
        category,
        link_status,
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "data": {
            "list": result.list,
            "pagination": result.pagination,
        }
    })))
}

/// GET /api/resources/{id} — 获取单条资源
pub async fn get_resource(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let resource = crate::services::resource::get_resource(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": resource })))
}

/// GET /api/resources/{id}/detail — 获取资源详情（含原始消息，用于提取对比）
pub async fn get_resource_detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let detail = crate::services::resource::get_resource_with_raw(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "data": detail })))
}

/// PUT /api/resources/{id} — 编辑资源
pub async fn update_resource(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateExtractedResource>,
) -> Result<Json<Value>, AppError> {
    crate::services::resource::update_resource(&state.db, id, &body).await?;
    Ok(Json(json!({ "success": true, "message": "资源已更新" })))
}

/// DELETE /api/resources/{id} — 删除资源
pub async fn delete_resource(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    crate::services::resource::delete_resource(&state.db, id).await?;
    Ok(Json(json!({ "success": true, "message": "资源已删除" })))
}

/// POST /api/resources/{id}/push — 单条资源推送
///
/// 复用与 `/push/trigger` 完全相同的推送配置（push_api_url + push_auth_type + 模板等），
/// 对指定 id 的资源发起一次推送请求，**修改 is_pushed = true**，
/// 在 push_histories 留痕（batch_id 以 `single_` 前缀，便于区分）。
pub async fn push_resource(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let result =
        crate::services::resource::push_single_resource(&state.db, &state.option_cache, id).await?;

    let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("");
    match status {
        "success" => Ok(Json(json!({ "success": true, "data": result }))),
        "config_error" => Ok(Json(json!({
            "success": false,
            "message": result.get("message").cloned().unwrap_or_default(),
            "data": { "missing": result.get("missing").cloned().unwrap_or_default() }
        }))),
        // status == "failed" 或其他：推送发出但 API 返回错误，仍返回详情供排查
        _ => Ok(Json(json!({
            "success": false,
            "message": result.get("message").cloned().unwrap_or_default(),
            "data": result,
        }))),
    }
}

/// POST /api/resources/{id}/check-link — 单条资源链接检测（Story4「检测」按钮）
pub async fn check_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let ignore_cache = body
        .get("ignore_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let resource = crate::services::resource::get_resource(&state.db, id).await?;
    let (status, details) = crate::services::link_check::check_resource_links(
        &state.db,
        &state.option_cache,
        &resource,
        ignore_cache,
    )
    .await?;
    let details_json: Vec<Value> = details
        .into_iter()
        .map(|(u, s)| json!({ "url": u, "status": s.as_str() }))
        .collect();
    Ok(Json(json!({
        "success": true,
        "data": { "link_status": status, "details": details_json },
    })))
}

/// GET /api/resources/stats — 资源统计
pub async fn get_resource_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let stats = crate::services::resource::get_resource_stats(&state.db).await?;
    Ok(Json(json!({ "success": true, "data": stats })))
}

/// PUT /api/push/extract-config — 更新提取配置
pub async fn update_extract_config(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    // 需要保存的配置键映射
    let config_keys = [
        "extract_mode",
        "auto_extract",
        "extract_interval",
        "ai_endpoints",
        "ai_prompt",
        "ai_use_proxy",
        "ai_concurrency",
    ];

    if let Some(obj) = body.as_object() {
        let mut cache = state.option_cache.write().await;
        for (key, value) in obj {
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // 只保存预定义的配置键
            let config_key = if config_keys.contains(&key.as_str()) {
                format!("push_{}", key)
            } else {
                continue;
            };

            match &state.db {
                crate::state::DbPool::Sqlite(pool) => {
                    sqlx::query("INSERT OR REPLACE INTO options (key, value) VALUES (?, ?)")
                        .bind(&config_key)
                        .bind(&val_str)
                        .execute(pool)
                        .await?;
                }
                crate::state::DbPool::Postgres(pool) => {
                    sqlx::query(
                        "INSERT INTO options (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2",
                    )
                    .bind(&config_key)
                    .bind(&val_str)
                    .execute(pool)
                    .await?;
                }
            }
            cache.insert(config_key, val_str);
        }
    }

    // 根据配置启停提取调度器
    let cache = state.option_cache.read().await;
    let auto_extract = cache.get("push_auto_extract").cloned().unwrap_or_default();
    let extract_interval: u64 = cache
        .get("push_extract_interval")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    drop(cache);

    if auto_extract == "1" || auto_extract.eq_ignore_ascii_case("true") {
        crate::services::scheduler::update_extract_scheduler(
            state.extract_scheduler.clone(),
            extract_interval,
            state.clone(),
        )
        .await;
    } else {
        crate::services::scheduler::stop_extract_scheduler(state.extract_scheduler.clone()).await;
    }

    // 返回调度器状态（下次执行时间）
    let sched = state.extract_scheduler.read().await;
    let next_run = if sched.running {
        let elapsed = sched
            .last_run_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let remaining = sched.interval_minutes * 60;
        let next_secs = remaining.saturating_sub(elapsed);
        Some(chrono::Local::now() + chrono::Duration::seconds(next_secs as i64))
    } else {
        None
    };
    drop(sched);

    Ok(Json(json!({
        "success": true,
        "message": "提取配置已更新",
        "next_run_at": next_run.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
    })))
}
