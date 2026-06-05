use crate::errors::AppError;
use crate::services::image_proxy;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;

/// GET /api/images/{photo_id}
/// 使用第一个活跃客户端下载图片
pub async fn get_image(
    Path(photo_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let (data, content_type, etag) = image_proxy::serve_image(None, &photo_id, &state).await?;
    build_image_response(data, content_type, etag)
}

/// GET /api/images/{client_id}/{photo_id}
/// 使用指定客户端下载图片
pub async fn get_image_with_client(
    Path((client_id, photo_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let (data, content_type, etag) =
        image_proxy::serve_image(Some(&client_id), &photo_id, &state).await?;
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
