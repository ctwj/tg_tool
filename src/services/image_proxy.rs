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

/// 获取指定 client_id 的 Telegram 客户端
async fn get_client_by_id(
    state: &AppState,
    client_id: &str,
) -> Result<grammers_client::Client, AppError> {
    let clients = state.tg_clients.read().await;
    clients
        .get(client_id)
        .and_then(|e| {
            if e.status == "active" {
                e.client.clone()
            } else {
                None
            }
        })
        .ok_or_else(|| AppError::NotFound(format!("客户端 {client_id} 不可用")))
}

/// 获取第一个活跃的 Telegram 客户端
async fn find_first_active_client(state: &AppState) -> Result<grammers_client::Client, AppError> {
    let clients = state.tg_clients.read().await;
    clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::Internal("没有可用的客户端".into()))
}

/// 从 collector_histories 表查找 photo_id 对应的 (channel_id,)
async fn find_channel_for_photo(state: &AppState, photo_id: &str) -> Result<Option<i64>, AppError> {
    let row: Option<(i64,)> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT channel_id FROM collector_histories WHERE remote_id = ? LIMIT 1")
                .bind(photo_id)
                .fetch_optional(pool)
                .await
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT channel_id FROM collector_histories WHERE remote_id = $1 LIMIT 1",
            )
            .bind(photo_id)
            .fetch_optional(pool)
            .await
        }
    }
    .map_err(|e| AppError::Internal(format!("查询图片频道失败: {e}")))?;
    Ok(row.map(|(ch,)| ch))
}

/// 在指定频道中查找包含目标 photo_id 的消息 Media
async fn find_photo_in_channel(
    client: &grammers_client::Client,
    photo_id: &str,
    channel_id: i64,
) -> Result<Option<grammers_client::types::Media>, AppError> {
    // 解析频道 PackedChat
    let mut dialogs = client.iter_dialogs();
    let mut target_packed = None;
    while let Ok(Some(dialog)) = dialogs.next().await {
        if dialog.chat().id() == channel_id {
            target_packed = Some(dialog.chat().pack());
            break;
        }
    }
    let packed = match target_packed {
        Some(p) => p,
        None => return Ok(None),
    };

    // 在频道中搜索包含目标 photo_id 的消息（最多搜索 100 条）
    let mut messages = client.iter_messages(packed);
    let mut searched = 0u32;
    while let Ok(Some(msg)) = messages.next().await {
        searched += 1;
        if let Some(grammers_client::types::Media::Photo(photo)) = msg.media()
            && format!("{}", photo.id()) == photo_id
        {
            return Ok(msg.media());
        }
        if searched >= 100 {
            break;
        }
    }
    Ok(None)
}

/// 计算数据的 ETag（SHA256 hex）
fn compute_etag(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 主入口：根据 photo_id 和可选 client_id 提供图片数据
/// 返回 (图片二进制数据, content_type, etag)
pub async fn serve_image(
    client_id: Option<&str>,
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

    // 5. 执行下载
    let result = download_and_cache(client_id, photo_id, state, &cache_path).await;

    // 6. 移除下载标记
    state.inflight_downloads.remove(photo_id);

    result
}

/// 从 Telegram 下载图片并缓存到本地
async fn download_and_cache(
    client_id: Option<&str>,
    photo_id: &str,
    state: &AppState,
    cache_path: &Path,
) -> Result<(Vec<u8>, String, String), AppError> {
    // 获取客户端：指定 client_id 或第一个活跃客户端
    let client = match client_id {
        Some(id) => get_client_by_id(state, id).await?,
        None => find_first_active_client(state).await?,
    };

    // 从 DB 查找 photo_id 对应的频道
    let channel_id = find_channel_for_photo(state, photo_id)
        .await?
        .ok_or_else(|| AppError::NotFound("图片不存在".into()))?;

    // 在该频道中查找 photo_id 对应的消息
    let media = find_photo_in_channel(&client, photo_id, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("图片不存在".into()))?;

    // 使用 iter_download 流式下载
    let downloadable = grammers_client::types::Downloadable::Media(media);
    let mut data = Vec::new();
    let mut download = client.iter_download(&downloadable);
    while let Some(chunk) = download
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("下载图片失败: {e}")))?
    {
        data.extend_from_slice(&chunk);
    }

    if data.is_empty() {
        return Err(AppError::NotFound("图片不存在".into()));
    }

    // 写入本地缓存
    if let Err(e) = tokio::fs::write(cache_path, &data).await {
        tracing::warn!("写入图片缓存失败: {e}");
    }

    let etag = compute_etag(&data);
    tracing::info!(
        "图片下载完成并缓存: {} ({} bytes, client: {})",
        photo_id,
        data.len(),
        client_id.unwrap_or("default")
    );
    Ok((data, "image/jpeg".to_string(), etag))
}
