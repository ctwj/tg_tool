use crate::errors::AppError;
use crate::state::AppState;
use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn list_files(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let files: Vec<crate::models::file::FileRecord> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, filename, uploader_id, link, created_at FROM files ORDER BY id DESC")
                .fetch_all(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, filename, uploader_id, link, created_at FROM files ORDER BY id DESC")
                .fetch_all(pool).await?
        }
    };
    Ok(Json(json!({ "success": true, "data": { "list": files } })))
}

pub async fn upload_file(
    State(state): State<AppState>,
    axum::extract::Extension(user): axum::extract::Extension<crate::models::user::User>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    // Ensure uploads directory exists
    let upload_dir = std::path::Path::new("./uploads");
    if !upload_dir.exists() {
        std::fs::create_dir_all(upload_dir)
            .map_err(|e| AppError::Internal(format!("创建上传目录失败: {e}")))?;
    }

    while let Some(field) = multipart.next_field().await
        .map_err(|e| AppError::BadRequest(format!("读取上传数据失败: {e}")))?
    {
        let filename = field.file_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let data = field.bytes().await
            .map_err(|e| AppError::BadRequest(format!("读取文件内容失败: {e}")))?;

        // Generate unique filename to avoid collision
        let unique_name = format!("{}_{}", &uuid::Uuid::new_v4().to_string()[..8], filename);
        let filepath = upload_dir.join(&unique_name);

        std::fs::write(&filepath, &data)
            .map_err(|e| AppError::Internal(format!("保存文件失败: {e}")))?;

        let link = format!("/files/download/{}", unique_name);

        match &state.db {
            crate::state::DbPool::Sqlite(pool) => {
                sqlx::query("INSERT INTO files (filename, uploader_id, link) VALUES (?, ?, ?)")
                    .bind(&filename).bind(user.id).bind(&link)
                    .execute(pool).await?;
            }
            crate::state::DbPool::Postgres(pool) => {
                sqlx::query("INSERT INTO files (filename, uploader_id, link) VALUES ($1, $2, $3)")
                    .bind(&filename).bind(user.id).bind(&link)
                    .execute(pool).await?;
            }
        }
        tracing::info!("File uploaded: {} ({} bytes)", filename, data.len());
    }
    Ok(Json(json!({ "success": true, "message": "文件上传成功" })))
}

pub async fn delete_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    // Look up file record to get filename for disk deletion
    let file: Option<crate::models::file::FileRecord> = match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query_as("SELECT id, filename, uploader_id, link, created_at FROM files WHERE id = ?")
                .bind(id).fetch_optional(pool).await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as("SELECT id, filename, uploader_id, link, created_at FROM files WHERE id = $1")
                .bind(id).fetch_optional(pool).await?
        }
    };
    let file = match file {
        Some(f) => f,
        None => return Err(AppError::NotFound("文件不存在".into())),
    };

    // Try to delete from disk (ignore error if file not found on disk)
    if let Some(link) = &file.link {
        let disk_name = link.strip_prefix("/files/download/").unwrap_or(link);
        let filepath = std::path::Path::new("./uploads").join(disk_name);
        let _ = std::fs::remove_file(filepath);
    }

    // Delete from database
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM files WHERE id = ?")
                .bind(id).execute(pool).await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM files WHERE id = $1")
                .bind(id).execute(pool).await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "文件已删除" })))
}

pub async fn download_file(
    Path(filename): Path<String>,
) -> Result<Response, AppError> {
    let filepath = std::path::Path::new("./uploads").join(&filename);
    if !filepath.exists() {
        return Err(AppError::NotFound("文件不存在".into()));
    }

    let data = tokio::fs::read(&filepath).await
        .map_err(|e| AppError::Internal(format!("读取文件失败: {e}")))?;

    // Guess content type from extension
    let content_type = mime_guess::from_path(&filepath).first_or_octet_stream();

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.as_ref().to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        Body::from(data),
    ).into_response())
}
