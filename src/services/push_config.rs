// 推送配置 CRUD + 按配置推送业务逻辑

use crate::errors::AppError;
use crate::models::push_config::PushConfigWithCollectorCount;
use crate::state::DbPool;

/// 获取推送配置列表（含关联采集器数量）
pub async fn list_configs(db: &DbPool) -> Result<Vec<PushConfigWithCollectorCount>, AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            let configs = sqlx::query_as::<_, PushConfigWithCollectorCount>(
                "SELECT pc.*, \
                 (SELECT COUNT(*) FROM push_config_collectors pcc WHERE pcc.push_config_id = pc.id) AS collector_count \
                 FROM push_configs pc ORDER BY pc.id DESC",
            )
            .fetch_all(pool)
            .await?;
            Ok(configs)
        }
        DbPool::Postgres(pool) => {
            let configs = sqlx::query_as::<_, PushConfigWithCollectorCount>(
                "SELECT pc.*, \
                 (SELECT COUNT(*) FROM push_config_collectors pcc WHERE pcc.push_config_id = pc.id) AS collector_count \
                 FROM push_configs pc ORDER BY pc.id DESC",
            )
            .fetch_all(pool)
            .await?;
            Ok(configs)
        }
    }
}

/// 创建推送配置（含数据源采集器关联）
pub async fn create_config(
    db: &DbPool,
    name: &str,
    api_url: &str,
    api_token: Option<&str>,
    target: &str,
    auth_type: &str,
    auth_key: &str,
    http_method: &str,
    body_template: Option<&str>,
    custom_headers: &str,
    batch_size: i64,
    data_source_type: &str,
    collector_ids: &[i64],
    auto_push: bool,
    push_interval: i64,
    link_check_before_push: bool,
) -> Result<i64, AppError> {
    if name.is_empty() {
        return Err(AppError::BadRequest("配置名称不能为空".into()));
    }
    if api_url.is_empty() {
        return Err(AppError::BadRequest("API 地址不能为空".into()));
    }

    let dst = if data_source_type.is_empty() {
        "all"
    } else {
        data_source_type
    };

    match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "INSERT INTO push_configs (name, api_url, api_token, target, auth_type, auth_key, http_method, body_template, custom_headers, batch_size, data_source_type, auto_push, push_interval, link_check_before_push) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(name)
            .bind(api_url)
            .bind(api_token)
            .bind(target)
            .bind(auth_type)
            .bind(auth_key)
            .bind(http_method)
            .bind(body_template)
            .bind(custom_headers)
            .bind(batch_size)
            .bind(dst)
            .bind(auto_push)
            .bind(push_interval)
            .bind(link_check_before_push)
            .execute(pool)
            .await?;

            let config_id = result.last_insert_rowid();

            // 写入采集器关联
            if dst == "selected" {
                for &cid in collector_ids {
                    sqlx::query(
                        "INSERT OR IGNORE INTO push_config_collectors (push_config_id, collector_id) VALUES (?, ?)",
                    )
                    .bind(config_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
            }

            Ok(config_id)
        }
        DbPool::Postgres(pool) => {
            let config_id: i64 = sqlx::query_scalar(
                "INSERT INTO push_configs (name, api_url, api_token, target, auth_type, auth_key, http_method, body_template, custom_headers, batch_size, data_source_type, auto_push, push_interval, link_check_before_push) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
                 RETURNING id",
            )
            .bind(name)
            .bind(api_url)
            .bind(api_token)
            .bind(target)
            .bind(auth_type)
            .bind(auth_key)
            .bind(http_method)
            .bind(body_template)
            .bind(custom_headers)
            .bind(batch_size)
            .bind(dst)
            .bind(auto_push)
            .bind(push_interval)
            .bind(link_check_before_push)
            .fetch_one(pool)
            .await?;

            if dst == "selected" {
                for &cid in collector_ids {
                    sqlx::query(
                        "INSERT INTO push_config_collectors (push_config_id, collector_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    )
                    .bind(config_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
            }

            Ok(config_id)
        }
    }
}

/// 更新推送配置（collector_ids 存在时全量替换关联）
pub async fn update_config(
    db: &DbPool,
    config_id: i64,
    body: &serde_json::Value,
) -> Result<(), AppError> {
    // 读取现有配置
    let existing = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = ?",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = $1",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
    };

    let existing = existing.ok_or_else(|| AppError::NotFound("推送配置不存在".into()))?;

    // 合并更新字段
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.name);
    let api_url = body
        .get("api_url")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.api_url);
    let api_token = body
        .get("api_token")
        .and_then(|v| v.as_str())
        .or(existing.api_token.as_deref());
    let target = body
        .get("target")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.target);
    let auth_type = body
        .get("auth_type")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.auth_type);
    let auth_key = body
        .get("auth_key")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.auth_key);
    let http_method = body
        .get("http_method")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.http_method);
    let body_template = body
        .get("body_template")
        .and_then(|v| v.as_str())
        .or(existing.body_template.as_deref());
    let custom_headers = body
        .get("custom_headers")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.custom_headers);
    let batch_size = body
        .get("batch_size")
        .and_then(|v| v.as_i64())
        .unwrap_or(existing.batch_size);
    let data_source_type = body
        .get("data_source_type")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.data_source_type);
    let auto_push = body
        .get("auto_push")
        .and_then(|v| v.as_bool())
        .unwrap_or(existing.auto_push);
    let push_interval = body
        .get("push_interval")
        .and_then(|v| v.as_i64())
        .unwrap_or(existing.push_interval);
    let link_check_before_push = body
        .get("link_check_before_push")
        .and_then(|v| v.as_bool())
        .unwrap_or(existing.link_check_before_push);

    match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "UPDATE push_configs SET name=?, api_url=?, api_token=?, target=?, auth_type=?, auth_key=?, http_method=?, body_template=?, custom_headers=?, batch_size=?, data_source_type=?, auto_push=?, push_interval=?, link_check_before_push=?, updated_at=CURRENT_TIMESTAMP WHERE id=?",
            )
            .bind(name)
            .bind(api_url)
            .bind(api_token)
            .bind(target)
            .bind(auth_type)
            .bind(auth_key)
            .bind(http_method)
            .bind(body_template)
            .bind(custom_headers)
            .bind(batch_size)
            .bind(data_source_type)
            .bind(auto_push)
            .bind(push_interval)
            .bind(link_check_before_push)
            .bind(config_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
        DbPool::Postgres(pool) => {
            let result = sqlx::query(
                "UPDATE push_configs SET name=$1, api_url=$2, api_token=$3, target=$4, auth_type=$5, auth_key=$6, http_method=$7, body_template=$8, custom_headers=$9, batch_size=$10, data_source_type=$11, auto_push=$12, push_interval=$13, link_check_before_push=$14, updated_at=CURRENT_TIMESTAMP WHERE id=$15",
            )
            .bind(name)
            .bind(api_url)
            .bind(api_token)
            .bind(target)
            .bind(auth_type)
            .bind(auth_key)
            .bind(http_method)
            .bind(body_template)
            .bind(custom_headers)
            .bind(batch_size)
            .bind(data_source_type)
            .bind(auto_push)
            .bind(push_interval)
            .bind(link_check_before_push)
            .bind(config_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
    }

    // 如果传了 collector_ids，全量替换关联
    if let Some(ids) = body.get("collector_ids").and_then(|v| v.as_array()) {
        let collector_ids: Vec<i64> = ids.iter().filter_map(|v| v.as_i64()).collect();

        // 先删旧的
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query("DELETE FROM push_config_collectors WHERE push_config_id = ?")
                    .bind(config_id)
                    .execute(pool)
                    .await?;
                for &cid in &collector_ids {
                    sqlx::query(
                        "INSERT OR IGNORE INTO push_config_collectors (push_config_id, collector_id) VALUES (?, ?)",
                    )
                    .bind(config_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
            }
            DbPool::Postgres(pool) => {
                sqlx::query("DELETE FROM push_config_collectors WHERE push_config_id = $1")
                    .bind(config_id)
                    .execute(pool)
                    .await?;
                for &cid in &collector_ids {
                    sqlx::query(
                        "INSERT INTO push_config_collectors (push_config_id, collector_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    )
                    .bind(config_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
            }
        }
    }

    Ok(())
}

/// 删除推送配置（级联删关联，push_histories 保留但 config_id 置 NULL）
pub async fn delete_config(db: &DbPool, config_id: i64) -> Result<(), AppError> {
    // 先清除 push_histories 的关联
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query("UPDATE push_histories SET push_config_id = NULL WHERE push_config_id = ?")
                .bind(config_id)
                .execute(pool)
                .await?;
            // 关联表由外键 ON DELETE CASCADE 自动清理
            let result = sqlx::query("DELETE FROM push_configs WHERE id = ?")
                .bind(config_id)
                .execute(pool)
                .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE push_histories SET push_config_id = NULL WHERE push_config_id = $1",
            )
            .bind(config_id)
            .execute(pool)
            .await?;
            let result = sqlx::query("DELETE FROM push_configs WHERE id = $1")
                .bind(config_id)
                .execute(pool)
                .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
    }
    Ok(())
}

/// 切换推送配置启用/禁用
pub async fn toggle_config(db: &DbPool, config_id: i64) -> Result<(), AppError> {
    match db {
        DbPool::Sqlite(pool) => {
            let result = sqlx::query(
                "UPDATE push_configs SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(config_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
        DbPool::Postgres(pool) => {
            let result = sqlx::query(
                "UPDATE push_configs SET is_active = NOT is_active, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
            )
            .bind(config_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                return Err(AppError::NotFound("推送配置不存在".into()));
            }
        }
    }
    Ok(())
}

/// 复制推送配置（含采集器关联）
pub async fn duplicate_config(db: &DbPool, config_id: i64) -> Result<i64, AppError> {
    let existing = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = ?",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = $1",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
    };

    let existing = existing.ok_or_else(|| AppError::NotFound("推送配置不存在".into()))?;

    let new_name = format!("{}(副本)", existing.name);

    // 创建副本
    let new_id = create_config(
        db,
        &new_name,
        &existing.api_url,
        existing.api_token.as_deref(),
        &existing.target,
        &existing.auth_type,
        &existing.auth_key,
        &existing.http_method,
        existing.body_template.as_deref(),
        &existing.custom_headers,
        existing.batch_size,
        &existing.data_source_type,
        &[], // collector_ids 在后面单独处理
        existing.auto_push,
        existing.push_interval,
        existing.link_check_before_push,
    )
    .await?;

    // 复制采集器关联
    let collector_ids: Vec<i64> = match db {
        DbPool::Sqlite(pool) => {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT collector_id FROM push_config_collectors WHERE push_config_id = ?",
            )
            .bind(config_id)
            .fetch_all(pool)
            .await?;
            rows.into_iter().map(|r| r.0).collect()
        }
        DbPool::Postgres(pool) => {
            let rows: Vec<(i64,)> = sqlx::query_as(
                "SELECT collector_id FROM push_config_collectors WHERE push_config_id = $1",
            )
            .bind(config_id)
            .fetch_all(pool)
            .await?;
            rows.into_iter().map(|r| r.0).collect()
        }
    };

    if !collector_ids.is_empty() && existing.data_source_type == "selected" {
        for &cid in &collector_ids {
            match db {
                DbPool::Sqlite(pool) => {
                    sqlx::query(
                        "INSERT OR IGNORE INTO push_config_collectors (push_config_id, collector_id) VALUES (?, ?)",
                    )
                    .bind(new_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
                DbPool::Postgres(pool) => {
                    sqlx::query(
                        "INSERT INTO push_config_collectors (push_config_id, collector_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    )
                    .bind(new_id)
                    .bind(cid)
                    .execute(pool)
                    .await?;
                }
            }
        }
    }

    Ok(new_id)
}

/// 按推送配置执行推送 — 查询该配置数据源范围内未推送的资源，执行推送
pub async fn push_for_config(
    db: &DbPool,
    option_cache: &crate::state::OptionCache,
    config_id: i64,
    batch_size_override: Option<i64>,
) -> Result<serde_json::Value, AppError> {
    // 1. 读取配置
    let config = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = ? AND is_active = 1",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = $1 AND is_active = TRUE",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
    };

    let config = config.ok_or_else(|| AppError::NotFound("推送配置不存在或已禁用".into()))?;

    if config.api_url.is_empty() {
        return Err(AppError::BadRequest("该配置的 API 地址为空".into()));
    }

    let batch_size = batch_size_override.unwrap_or(config.batch_size);

    // 2. 查询该配置数据源范围内的未推送资源（含 img_forward_status；图片转存过滤改由 Rust 分类）
    let resources: Vec<crate::models::extracted_resource::ExtractedResource> = match db {
        DbPool::Sqlite(pool) => {
            if config.data_source_type == "all" {
                sqlx::query_as(
                    "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                     (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status \
                     FROM extracted_resources er \
                     WHERE er.is_pushed = 0 \
                     AND NOT EXISTS (SELECT 1 FROM resource_push_status rps WHERE rps.resource_id = er.id AND rps.push_config_id = ? AND rps.status = 'pushed') \
                     LIMIT ?"
                )
                .bind(config_id)
                .bind(batch_size)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                     (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status \
                     FROM extracted_resources er \
                     JOIN collector_histories ch ON er.collector_history_id = ch.id \
                     JOIN push_config_collectors pcc ON pcc.collector_id = ch.collector_id \
                     WHERE pcc.push_config_id = ? \
                     AND NOT EXISTS (SELECT 1 FROM resource_push_status rps WHERE rps.resource_id = er.id AND rps.push_config_id = ? AND rps.status = 'pushed') \
                     LIMIT ?"
                )
                .bind(config_id)
                .bind(config_id)
                .bind(batch_size)
                .fetch_all(pool)
                .await?
            }
        }
        DbPool::Postgres(pool) => {
            if config.data_source_type == "all" {
                sqlx::query_as(
                    "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                     (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status \
                     FROM extracted_resources er \
                     WHERE er.is_pushed = FALSE \
                     AND NOT EXISTS (SELECT 1 FROM resource_push_status rps WHERE rps.resource_id = er.id AND rps.push_config_id = $1 AND rps.status = 'pushed') \
                     LIMIT $2"
                )
                .bind(config_id)
                .bind(batch_size)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
                     (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status \
                     FROM extracted_resources er \
                     JOIN collector_histories ch ON er.collector_history_id = ch.id \
                     JOIN push_config_collectors pcc ON pcc.collector_id = ch.collector_id \
                     WHERE pcc.push_config_id = $1 \
                     AND NOT EXISTS (SELECT 1 FROM resource_push_status rps WHERE rps.resource_id = er.id AND rps.push_config_id = $2 AND rps.status = 'pushed') \
                     LIMIT $3"
                )
                .bind(config_id)
                .bind(config_id)
                .bind(batch_size)
                .fetch_all(pool)
                .await?
            }
        }
    };

    if resources.is_empty() {
        return Ok(serde_json::json!({
            "status": "success",
            "message": "没有需要推送的资源",
            "processed_count": 0
        }));
    }

    // 3. 有效性分类：图片未转存 / 链接失效 跳过（FR-001/FR-003/FR-006）
    //    若配置关闭「推送前链接检测」，则跳过 LinkChecker 调用，仅过滤图片未转存
    let classify = if config.link_check_before_push {
        match crate::services::link_check::classify_resources(db, option_cache, &resources).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("资源有效性分类失败，降级为全部尝试推送: {e}");
                crate::services::link_check::ClassifyResult {
                    valid: resources.clone(),
                    skipped: Vec::new(),
                }
            }
        }
    } else {
        tracing::info!(
            "推送配置 id={} 关闭了推送前链接检测，跳过 LinkChecker 调用",
            config_id
        );
        crate::services::link_check::classify_without_link_check(&resources)
    };
    let skipped_image = classify.skipped_image_count();
    let skipped_link = classify.skipped_link_count();
    let valid = &classify.valid;
    let skipped_json = serde_json::json!({
        "image_not_forwarded": skipped_image,
        "link_invalid": skipped_link,
        "total": skipped_image + skipped_link,
    });

    // 4. 批次 ID
    let target_label = if config.target.is_empty() {
        "default"
    } else {
        &config.target
    };
    let batch_id = format!("batch_{}_{}", target_label, chrono::Utc::now().timestamp());

    if valid.is_empty() {
        crate::services::resource::record_push_history_with_skips(
            db,
            &batch_id,
            &config.target,
            "success",
            0,
            skipped_image as i64,
            skipped_link as i64,
            "没有可推送的有效资源",
            None,
            Some(config_id),
            &classify.skipped,
        )
        .await?;
        return Ok(serde_json::json!({
            "status": "no_valid_resources",
            "processed_count": 0,
            "batch_id": batch_id,
            "skipped": skipped_json,
        }));
    }

    let resource_count = valid.len();
    // 读取图床域名 — 推送时把 img 字段（photo_id）拼接为完整图床 URL
    let image_domain = {
        let cache = option_cache.read().await;
        cache.get("TelegramImageDomain").cloned()
    };
    let result = crate::services::resource::build_and_send_push_with_params(
        valid,
        &config.api_url,
        config.api_token.as_deref().unwrap_or(""),
        &config.target,
        &config.auth_type,
        &config.auth_key,
        &config.http_method,
        config.body_template.as_deref().unwrap_or(""),
        &config.custom_headers,
        image_domain.as_deref(),
    )
    .await;

    match result {
        Ok((status_code, body, is_success, _request_info)) => {
            if is_success {
                for r in valid {
                    crate::services::resource::mark_resource_pushed(db, r.id).await?;
                }
                insert_push_status_batch(db, valid, config_id, "pushed").await?;

                crate::services::resource::record_push_history_with_skips(
                    db,
                    &batch_id,
                    &config.target,
                    "success",
                    resource_count as i64,
                    skipped_image as i64,
                    skipped_link as i64,
                    "推送成功",
                    None,
                    Some(config_id),
                    &classify.skipped,
                )
                .await?;

                Ok(serde_json::json!({
                    "status": "success",
                    "processed_count": resource_count,
                    "batch_id": batch_id,
                    "skipped": skipped_json,
                }))
            } else {
                crate::services::resource::record_push_history_with_skips(
                    db,
                    &batch_id,
                    &config.target,
                    "failed",
                    resource_count as i64,
                    skipped_image as i64,
                    skipped_link as i64,
                    &format!("API返回错误: {}", status_code),
                    Some(&body),
                    Some(config_id),
                    &classify.skipped,
                )
                .await?;
                Err(AppError::Internal(format!(
                    "推送API返回错误: status={}, body={}",
                    status_code, body
                )))
            }
        }
        Err(e) => {
            crate::services::resource::record_push_history_with_skips(
                db,
                &batch_id,
                &config.target,
                "failed",
                0,
                skipped_image as i64,
                skipped_link as i64,
                "推送请求失败",
                Some(&e.to_string()),
                Some(config_id),
                &classify.skipped,
            )
            .await?;
            Err(AppError::Internal(format!("推送请求失败: {e}")))
        }
    }
}

/// 批量插入 resource_push_status
async fn insert_push_status_batch(
    db: &DbPool,
    resources: &[crate::models::extracted_resource::ExtractedResource],
    config_id: i64,
    status: &str,
) -> Result<(), AppError> {
    for r in resources {
        match db {
            DbPool::Sqlite(pool) => {
                sqlx::query(
                    "INSERT OR IGNORE INTO resource_push_status (resource_id, push_config_id, status) VALUES (?, ?, ?)",
                )
                .bind(r.id)
                .bind(config_id)
                .bind(status)
                .execute(pool)
                .await?;
            }
            DbPool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO resource_push_status (resource_id, push_config_id, status) VALUES ($1, $2, $3) ON CONFLICT (resource_id, push_config_id) DO NOTHING",
                )
                .bind(r.id)
                .bind(config_id)
                .bind(status)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

/// 按推送配置维度批量链接检测（FR-010 双通道 ch2，仅检测不推送）。
/// 检测该配置数据源范围内未推送资源的链接，结果写入缓存供推送/列表复用。
pub async fn check_links_for_config(
    db: &DbPool,
    option_cache: &crate::state::OptionCache,
    config_id: i64,
    ignore_cache: bool,
) -> Result<serde_json::Value, AppError> {
    use crate::services::link_checker::LinkStatus;

    let config = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = ?",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, crate::models::push_config::PushConfig>(
                "SELECT * FROM push_configs WHERE id = $1",
            )
            .bind(config_id)
            .fetch_optional(pool)
            .await?
        }
    };
    let config = config.ok_or_else(|| AppError::NotFound("推送配置不存在".into()))?;

    const CAP: i64 = 1000;
    let cols = "er.id, er.collector_history_id, er.title, er.url, er.description, er.category, er.tags, er.img, er.source, er.extra, er.extract_mode, er.is_pushed, er.is_edited, er.created_at, er.updated_at, \
         (SELECT ft.status FROM forward_tasks ft WHERE ft.remote_id = er.img ORDER BY ft.id DESC LIMIT 1) AS img_forward_status";
    let resources: Vec<crate::models::extracted_resource::ExtractedResource> = match db {
        DbPool::Sqlite(pool) => {
            if config.data_source_type == "all" {
                sqlx::query_as(&format!(
                    "SELECT {cols} FROM extracted_resources er WHERE er.is_pushed = 0 LIMIT ?"
                ))
                .bind(CAP)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as(&format!(
                    "SELECT {cols} FROM extracted_resources er \
                     JOIN collector_histories ch ON er.collector_history_id = ch.id \
                     JOIN push_config_collectors pcc ON pcc.collector_id = ch.collector_id \
                     WHERE pcc.push_config_id = ? AND er.is_pushed = 0 LIMIT ?"
                ))
                .bind(config_id)
                .bind(CAP)
                .fetch_all(pool)
                .await?
            }
        }
        DbPool::Postgres(pool) => {
            if config.data_source_type == "all" {
                sqlx::query_as(&format!(
                    "SELECT {cols} FROM extracted_resources er WHERE er.is_pushed = FALSE LIMIT $1"
                ))
                .bind(CAP)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as(&format!(
                    "SELECT {cols} FROM extracted_resources er \
                     JOIN collector_histories ch ON er.collector_history_id = ch.id \
                     JOIN push_config_collectors pcc ON pcc.collector_id = ch.collector_id \
                     WHERE pcc.push_config_id = $1 AND er.is_pushed = FALSE LIMIT $2"
                ))
                .bind(config_id)
                .bind(CAP)
                .fetch_all(pool)
                .await?
            }
        }
    };

    let urls: Vec<String> = resources
        .iter()
        .flat_map(|r| crate::services::link_check::split_resource_urls(r.url.as_deref()))
        .collect();
    let statuses =
        crate::services::link_check::check_urls(db, option_cache, &urls, ignore_cache).await?;

    let mut valid_count = 0i64;
    let mut invalid_count = 0i64;
    let mut pending_count = 0i64;
    let mut invalid_resources = Vec::new();
    for r in &resources {
        let rus = crate::services::link_check::split_resource_urls(r.url.as_deref());
        if rus
            .iter()
            .any(|u| statuses.get(u) == Some(&LinkStatus::Pending))
        {
            pending_count += 1;
        }
        match crate::services::link_check::aggregate_link_status(r, &statuses) {
            "valid" => valid_count += 1,
            "invalid" => {
                invalid_count += 1;
                let inv: Vec<String> = rus
                    .iter()
                    .filter(|u| statuses.get(*u) == Some(&LinkStatus::Invalid))
                    .cloned()
                    .collect();
                invalid_resources.push(serde_json::json!({
                    "resource_id": r.id,
                    "title": r.title,
                    "urls_invalid": inv,
                }));
            }
            _ => {}
        }
    }

    Ok(serde_json::json!({
        "checked_count": resources.len(),
        "valid_count": valid_count,
        "invalid_count": invalid_count,
        "pending_count": pending_count,
        "invalid_resources": invalid_resources,
    }))
}
