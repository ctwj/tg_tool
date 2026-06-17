use crate::errors::AppError;
use crate::services::image_proxy;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;

/// GET /api/images/{id}
/// 按 Bot file_id 直接下载图片（photo_id 路由已移除，统一走 file_id）
pub async fn get_image(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let (data, content_type, etag) = image_proxy::serve_image_by_file_id(&id, &state).await?;
    build_image_response(data, content_type, etag)
}

/// GET /api/images/file/{file_id}
/// 直接按 Bot file_id 下载图片（跳过 image_mappings 查询）
pub async fn get_image_by_file_id(
    Path(file_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let (data, content_type, etag) = image_proxy::serve_image_by_file_id(&file_id, &state).await?;
    build_image_response(data, content_type, etag)
}

fn build_image_response(
    data: Vec<u8>,
    content_type: String,
    etag: String,
) -> Result<Response, AppError> {
    let body = axum::body::Body::from(data);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &content_type)
        .header(header::CACHE_CONTROL, "public, max-age=2592000")
        .header(header::ETAG, format!("\"{}\"", etag))
        .body(body)
        .map_err(|e| AppError::Internal(format!("构建响应失败: {e}")))
}
