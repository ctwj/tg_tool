// 直链上传本地中转（feature 047 US3 — FR-008）
// 下载直链到 PAN_STAGING_DIR，流式写入（磁盘不足时写入失败即报错），任务完成后清理

use std::path::{Path, PathBuf};

use crate::errors::AppError;

const DOWNLOAD_TIMEOUT_SECS: u64 = 1800; // 大文件下载超时 30 分钟

/// 从 URL 提取文件名，兜底 task-{id}
pub fn extract_filename(url: &str, task_id: i64) -> String {
    let path_part = url.split('?').next().unwrap_or(url);
    // 取 scheme://host 之后的 path；只有 host（无 path）则兜底
    let after_scheme = path_part.split("://").nth(1).unwrap_or(path_part);
    let path = match after_scheme.find('/') {
        Some(i) => &after_scheme[i + 1..],
        None => return format!("task-{task_id}"),
    };
    let decoded = urldecode(path);
    let name = decoded.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        format!("task-{task_id}")
    } else {
        name.to_string()
    }
}

/// 简易 URL 解码（仅 %XX 与 +），避免引入新依赖
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// 下载直链到本地中转目录，返回本地文件路径（流式写入，磁盘不足时写入失败报错）
pub async fn download_to_staging(
    url: &str,
    task_id: i64,
    staging_dir: &Path,
) -> Result<PathBuf, AppError> {
    tokio::fs::create_dir_all(staging_dir)
        .await
        .map_err(|e| AppError::Internal(format!("创建中转目录失败: {e}")))?;

    let filename = extract_filename(url, task_id);
    let path = staging_dir.join(format!("{task_id}_{filename}"));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP 客户端构建失败: {e}")))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("下载失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "下载失败: HTTP {}",
            resp.status()
        )));
    }

    use futures::StreamExt;
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| AppError::Internal(format!("创建中转文件失败: {e}")))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Internal(format!("下载流读取失败: {e}")))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::Internal(format!("写入中转失败（可能磁盘不足）: {e}")))?;
    }
    file.flush()
        .await
        .map_err(|e| AppError::Internal(format!("中转文件 flush 失败: {e}")))?;

    tracing::info!("直链已下载到中转: {:?}", path);
    Ok(path)
}

/// 清理中转文件（不存在视为成功）
pub async fn cleanup(path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("清理中转文件失败 {:?}: {e}", path);
    }
}

use tokio::io::AsyncWriteExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_filename_plain() {
        assert_eq!(
            extract_filename("https://example.com/files/movie.mp4", 1),
            "movie.mp4"
        );
    }

    #[test]
    fn test_extract_filename_with_query() {
        assert_eq!(
            extract_filename("https://example.com/a.txt?token=xxx&x=1", 2),
            "a.txt"
        );
    }

    #[test]
    fn test_extract_filename_url_encoded() {
        assert_eq!(
            extract_filename("https://example.com/%E4%B8%AD%E6%96%87.zip", 3),
            "中文.zip"
        );
    }

    #[test]
    fn test_extract_filename_fallback() {
        assert_eq!(extract_filename("https://example.com/", 5), "task-5");
        assert_eq!(extract_filename("https://example.com", 6), "task-6");
    }

    #[tokio::test]
    async fn test_cleanup_nonexistent_no_panic() {
        cleanup(&PathBuf::from("/nonexistent/tgtool-staging-test-xxx")).await;
    }
}
