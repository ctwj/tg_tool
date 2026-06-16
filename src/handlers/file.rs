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

/// 文件上传/下载根目录（feature 028 SEC-004 限定目录）
const UPLOAD_DIR: &str = "./uploads";

/// 校验 filename 规范化后位于 UPLOAD_DIR 内，返回安全路径（feature 028 SEC-004）。
/// 拒绝 `..`、绝对路径、符号链接逃逸、空字节 → Err；合法文件 → Ok(规范路径)。
pub fn ensure_within_upload_dir(filename: &str) -> Result<std::path::PathBuf, AppError> {
    // 空字节注入防护
    if filename.contains('\0') {
        return Err(AppError::BadRequest("非法文件名".into()));
    }
    let upload_dir = std::path::Path::new(UPLOAD_DIR);
    let canon_upload = upload_dir
        .canonicalize()
        .map_err(|_| AppError::Internal("上传目录不可用".into()))?;
    let filepath = upload_dir.join(filename);
    // canonicalize 解析 ..、符号链接、绝对路径；文件不存在时用 components 过滤兜底
    let resolved = match filepath.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut clean = std::path::PathBuf::new();
            for comp in filepath.components() {
                match comp {
                    std::path::Component::Normal(n) => clean.push(n),
                    std::path::Component::CurDir => {}
                    _ => return Err(AppError::Forbidden("非法文件路径".into())),
                }
            }
            canon_upload.join(clean)
        }
    };
    if !resolved.starts_with(&canon_upload) {
        return Err(AppError::Forbidden("非法文件路径".into()));
    }
    Ok(resolved)
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn list_files(
    State(state): State<AppState>,
    Query(_params): Query<PaginationParams>,
) -> Result<Json<Value>, AppError> {
    let files: Vec<crate::models::file::FileRecord> =
        match &state.db {
            crate::state::DbPool::Sqlite(pool) => sqlx::query_as(
                "SELECT id, filename, uploader_id, link, created_at FROM files ORDER BY id DESC",
            )
            .fetch_all(pool)
            .await?,
            crate::state::DbPool::Postgres(pool) => sqlx::query_as(
                "SELECT id, filename, uploader_id, link, created_at FROM files ORDER BY id DESC",
            )
            .fetch_all(pool)
            .await?,
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

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("读取上传数据失败: {e}")))?
    {
        let filename = field
            .file_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("读取文件内容失败: {e}")))?;

        // feature 028 SEC-004：清洗 filename（剥离目录/..），杜绝穿越进入 unique_name
        let safe_name = std::path::Path::new(&filename)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());
        // Generate unique filename to avoid collision
        let unique_name = format!("{}_{}", &uuid::Uuid::new_v4().to_string()[..8], safe_name);
        let filepath = upload_dir.join(&unique_name);
        // 校验存储路径在 uploads 内
        ensure_within_upload_dir(&unique_name)?;

        std::fs::write(&filepath, &data)
            .map_err(|e| AppError::Internal(format!("保存文件失败: {e}")))?;

        let link = format!("/files/download/{}", unique_name);

        match &state.db {
            crate::state::DbPool::Sqlite(pool) => {
                sqlx::query("INSERT INTO files (filename, uploader_id, link) VALUES (?, ?, ?)")
                    .bind(&filename)
                    .bind(user.id)
                    .bind(&link)
                    .execute(pool)
                    .await?;
            }
            crate::state::DbPool::Postgres(pool) => {
                sqlx::query("INSERT INTO files (filename, uploader_id, link) VALUES ($1, $2, $3)")
                    .bind(&filename)
                    .bind(user.id)
                    .bind(&link)
                    .execute(pool)
                    .await?;
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
            sqlx::query_as(
                "SELECT id, filename, uploader_id, link, created_at FROM files WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query_as(
                "SELECT id, filename, uploader_id, link, created_at FROM files WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
        }
    };
    let file = match file {
        Some(f) => f,
        None => return Err(AppError::NotFound("文件不存在".into())),
    };

    // Try to delete from disk (ignore error if file not found on disk)
    if let Some(link) = &file.link {
        let disk_name = link.strip_prefix("/files/download/").unwrap_or(link);
        // feature 028 SEC-004：校验路径在 uploads 内（防 link 含 ../ 逃逸）
        if let Ok(filepath) = ensure_within_upload_dir(disk_name) {
            let _ = std::fs::remove_file(filepath);
        }
    }

    // Delete from database
    match &state.db {
        crate::state::DbPool::Sqlite(pool) => {
            sqlx::query("DELETE FROM files WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        crate::state::DbPool::Postgres(pool) => {
            sqlx::query("DELETE FROM files WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(Json(json!({ "success": true, "message": "文件已删除" })))
}

pub async fn download_file(Path(filename): Path<String>) -> Result<Response, AppError> {
    // feature 028 SEC-004：校验路径在 uploads 内，拒穿越（../、绝对路径、符号链接、空字节）
    let filepath = ensure_within_upload_dir(&filename)?;
    if !filepath.exists() {
        return Err(AppError::NotFound("文件不存在".into()));
    }

    let data = tokio::fs::read(&filepath)
        .await
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
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_upload_dir_exists() {
        std::fs::create_dir_all(UPLOAD_DIR).ok();
    }

    /// feature 028 SEC-004：各类路径穿越必须被拒
    #[test]
    fn test_traversal_dotdot_rejected() {
        ensure_upload_dir_exists();
        assert!(ensure_within_upload_dir("../secret.txt").is_err());
        assert!(ensure_within_upload_dir("../../etc/passwd").is_err());
        assert!(ensure_within_upload_dir("../../.env").is_err());
        assert!(ensure_within_upload_dir("../data.db").is_err());
    }

    #[test]
    fn test_absolute_path_rejected() {
        ensure_upload_dir_exists();
        assert!(ensure_within_upload_dir("/etc/passwd").is_err());
        assert!(ensure_within_upload_dir("/Windows/system32/config/sam").is_err());
    }

    #[test]
    fn test_null_byte_rejected() {
        ensure_upload_dir_exists();
        assert!(ensure_within_upload_dir("file\0.txt").is_err());
    }

    /// 合法文件名（uploads 内）须通过（回归，SC-004）
    #[test]
    fn test_legitimate_filename_accepted() {
        ensure_upload_dir_exists();
        assert!(ensure_within_upload_dir("legit_file.txt").is_ok());
        assert!(ensure_within_upload_dir("sub/legit.txt").is_ok());
    }
}
