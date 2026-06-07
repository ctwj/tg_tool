use crate::errors::AppError;
use crate::state::AppState;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Instant;

/// 校验 photo_id 格式：仅允许数字和下划线，长度 1-50，防止路径遍历
pub fn validate_photo_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > 50 {
        return Err(AppError::BadRequest("无效的图片 ID".into()));
    }
    if !id.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return Err(AppError::BadRequest("无效的图片 ID".into()));
    }
    Ok(())
}

/// 检查缓存文件是否在 TTL 内有效
fn is_cache_valid(path: &Path, ttl_days: u64) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let elapsed = modified.elapsed().unwrap_or_default();
    elapsed < std::time::Duration::from_secs(ttl_days * 86400)
}

/// 从 option_cache 读取缓存 TTL（天数），默认 7 天
async fn get_cache_ttl(state: &AppState) -> u64 {
    let cache = state.option_cache.read().await;
    cache
        .get("ImageCacheTTL")
        .and_then(|v| v.parse().ok())
        .unwrap_or(7)
}

/// 计算数据的 ETag（SHA256 hex）
fn compute_etag(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 主入口：根据 photo_id 和可选 client_id 提供图片数据
/// 新流程：本地缓存 → image_mappings 查 file_id → Bot API getFile 下载
/// 返回 (图片二进制数据, content_type, etag)
pub async fn serve_image(
    _client_id: Option<&str>,
    photo_id: &str,
    state: &AppState,
) -> Result<(Vec<u8>, String, String), AppError> {
    // 1. 校验 photo_id
    validate_photo_id(photo_id)?;

    let ttl_days = get_cache_ttl(state).await;
    let cache_dir = &state.image_cache_dir;
    let cache_path = cache_dir.join(format!("{}.jpg", photo_id));

    // 2. 检查本地缓存
    if cache_path.exists() && is_cache_valid(&cache_path, ttl_days) {
        let data = tokio::fs::read(&cache_path)
            .await
            .map_err(|e| AppError::Internal(format!("读取缓存失败: {e}")))?;
        let etag = compute_etag(&data);
        tracing::debug!("缓存命中: {}", photo_id);
        return Ok((data, "image/jpeg".to_string(), etag));
    }

    // 3. 检查是否已有下载任务进行中（防重复下载）
    if state.inflight_downloads.contains_key(photo_id) {
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if cache_path.exists() {
                let data = tokio::fs::read(&cache_path)
                    .await
                    .map_err(|e| AppError::Internal(format!("读取缓存失败: {e}")))?;
                let etag = compute_etag(&data);
                return Ok((data, "image/jpeg".to_string(), etag));
            }
        }
        return Err(AppError::Internal("图片下载超时".into()));
    }

    // 4. 标记为下载中
    state
        .inflight_downloads
        .insert(photo_id.to_string(), Instant::now());

    // 5. 通过 Bot API 下载
    let result = download_via_bot_api(state, photo_id, &cache_path).await;

    // 6. 移除下载标记
    state.inflight_downloads.remove(photo_id);

    result
}

/// 通过 Bot API getFile 下载图片
async fn download_via_bot_api(
    state: &AppState,
    photo_id: &str,
    cache_path: &Path,
) -> Result<(Vec<u8>, String, String), AppError> {
    // 读取图床配置
    let (bot_token, proxy_url) = {
        let cache = state.option_cache.read().await;
        let bot_id = cache.get("ImageBotId").cloned().unwrap_or_default();

        if bot_id.is_empty() {
            return Err(AppError::Internal(
                "请先配置图床 Bot 和图床群组".to_string(),
            ));
        }

        let token = get_bot_token(state, &bot_id).await?;
        let proxy = cache
            .get("http_proxy_url")
            .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
            .or_else(|| {
                cache
                    .get("proxy_url")
                    .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
            });

        (token, proxy)
    };

    // 查 image_mappings 获取 file_id
    let file_id = get_file_id_for_remote_id(&state.db, photo_id).await?;
    let file_id = match file_id {
        Some(fid) => fid,
        None => {
            // 没有映射记录，可能是图片还未转发
            return Err(AppError::NotFound(
                "图片尚未转发到图床，请等待转发队列处理".to_string(),
            ));
        }
    };

    // 通过 Bot API getFile 下载
    let data = crate::services::bot_api::get_file(
        &bot_token,
        &file_id,
        proxy_url.as_deref(),
    )
    .await?;

    if data.is_empty() {
        return Err(AppError::NotFound("图片数据为空".to_string()));
    }

    // 写入本地缓存
    if let Err(e) = tokio::fs::write(cache_path, &data).await {
        tracing::warn!("写入图片缓存失败: {e}");
    }

    let etag = compute_etag(&data);
    tracing::info!(
        "图片通过 Bot API 下载完成: {} ({} bytes)",
        photo_id,
        data.len()
    );
    Ok((data, "image/jpeg".to_string(), etag))
}

/// 从 clients 表获取 Bot Token
async fn get_bot_token(state: &AppState, bot_id: &str) -> Result<String, AppError> {
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT token FROM clients WHERE id = ?")
                    .bind(bot_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
        crate::state::DbPool::Postgres(pool) => {
            let row: Option<(Option<String>,)> =
                sqlx::query_as("SELECT token FROM clients WHERE id = $1")
                    .bind(bot_id)
                    .fetch_optional(pool)
                    .await?;
            row.and_then(|r| r.0)
        }
    }
    .ok_or_else(|| AppError::NotFound(format!("Bot 客户端不存在: {bot_id}")))
}

/// 查询 image_mappings 表获取 file_id
async fn get_file_id_for_remote_id(
    db: &crate::state::DbPool,
    remote_id: &str,
) -> Result<Option<String>, AppError> {
    match db {
        crate::state::DbPool::Sqlite(pool) => {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT file_id FROM image_mappings WHERE remote_id = ?")
                    .bind(remote_id)
                    .fetch_optional(pool)
                    .await?;
            Ok(row.map(|r| r.0))
        }
        crate::state::DbPool::Postgres(pool) => {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT file_id FROM image_mappings WHERE remote_id = $1")
                    .bind(remote_id)
                    .fetch_optional(pool)
                    .await?;
            Ok(row.map(|r| r.0))
        }
    }
}
