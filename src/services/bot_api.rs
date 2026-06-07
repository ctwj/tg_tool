// Bot API HTTP 封装 — sendPhoto、getFile、getMe

use crate::errors::AppError;
use reqwest::multipart;
use serde::Deserialize;
use std::time::Duration;

/// Bot API 响应中的 getMe 结果
#[derive(Debug, Clone, Deserialize)]
pub struct BotInfo {
    pub id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

/// Telegram Bot API 通用响应包装
#[derive(Debug, Deserialize)]
struct BotApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<i32>,
    parameters: Option<BotApiParameters>,
}

#[derive(Debug, Deserialize)]
struct BotApiParameters {
    retry_after: Option<i64>,
}

/// sendPhoto 响应中的 PhotoSize
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PhotoSize {
    file_id: String,
    width: i64,
    height: i64,
}

/// sendPhoto 响应中的 Message
#[derive(Debug, Deserialize)]
struct BotMessage {
    photo: Option<Vec<PhotoSize>>,
}

/// getFile 响应
#[derive(Debug, Deserialize)]
struct FileResult {
    file_path: Option<String>,
}

fn bot_api_url(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

fn file_download_url(token: &str, file_path: &str) -> String {
    format!("https://api.telegram.org/file/bot{token}/{file_path}")
}

/// 构建带可选代理的 reqwest Client
fn build_client(proxy_url: Option<&str>) -> Result<reqwest::Client, AppError> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    if let Some(proxy) = proxy_url {
        if !proxy.is_empty() {
            // Bot API 使用 HTTPS，需要 HTTP/HTTPS 代理
            if proxy.starts_with("socks5://") || proxy.starts_with("socks5h://") {
                // reqwest 原生支持 socks5
                builder = builder.proxy(
                    reqwest::Proxy::all(proxy)
                        .map_err(|e| AppError::Internal(format!("代理配置失败: {e}")))?,
                );
            } else {
                builder = builder.proxy(
                    reqwest::Proxy::all(proxy)
                        .map_err(|e| AppError::Internal(format!("代理配置失败: {e}")))?,
                );
            }
        }
    }

    builder
        .build()
        .map_err(|e| AppError::Internal(format!("创建 HTTP 客户端失败: {e}")))
}

/// 调用 Bot API getMe 验证 token 有效性
pub async fn validate_token(token: &str, proxy_url: Option<&str>) -> Result<BotInfo, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "getMe");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Bot API 请求失败: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("读取响应失败: {e}")))?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::BadRequest("Bot Token 无效".to_string()));
    }

    let api_resp: BotApiResponse<BotInfo> =
        serde_json::from_str(&body).map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))?;

    if !api_resp.ok {
        let desc = api_resp.description.unwrap_or_default();
        return Err(AppError::BadRequest(format!("Token 验证失败: {desc}")));
    }

    api_resp
        .result
        .ok_or_else(|| AppError::Internal("Bot API 响应缺少 result".to_string()))
}

/// 通过 Bot API sendPhoto 发送图片到群组，返回最大的 photo file_id
pub async fn send_photo(
    token: &str,
    chat_id: &str,
    photo_bytes: Vec<u8>,
    caption: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<String, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "sendPhoto");

    let file_part = multipart::Part::bytes(photo_bytes)
        .file_name("photo.jpg")
        .mime_str("image/jpeg")
        .map_err(|e| AppError::Internal(format!("MIME 设置失败: {e}")))?;

    let mut form = multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part("photo", file_part);

    if let Some(cap) = caption {
        // caption 上限 1024 字符，截断（按字符边界安全截断）
        let truncated: String = cap.chars().take(1024).collect();
        form = form.text("caption", truncated);
    }

    let resp = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("sendPhoto 请求失败: {e}")))?;

    handle_bot_response::<BotMessage>(resp, "sendPhoto").await.map(|msg| {
        // 取最大的图片 file_id（最后一个是最大尺寸）
        msg.photo
            .and_then(|sizes| sizes.last().map(|s| s.file_id.clone()))
            .ok_or_else(|| AppError::Internal("sendPhoto 响应无 photo 数据".to_string()))
    })?
}

/// 通过 Bot API getFile + 文件下载获取图片二进制数据
pub async fn get_file(
    token: &str,
    file_id: &str,
    proxy_url: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    let client = build_client(proxy_url)?;

    // Step 1: getFile 获取 file_path
    let url = bot_api_url(token, "getFile");
    let resp = client
        .get(&url)
        .query(&[("file_id", file_id)])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("getFile 请求失败: {e}")))?;

    let file_result = handle_bot_response::<FileResult>(resp, "getFile").await?;

    let file_path = file_result
        .file_path
        .ok_or_else(|| AppError::Internal("getFile 响应无 file_path".to_string()))?;

    // Step 2: 下载文件
    let download_url = file_download_url(token, &file_path);
    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("文件下载失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "文件下载失败: status={}, body={}",
            status, body
        )));
    }

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| AppError::Internal(format!("读取文件数据失败: {e}")))
}

/// 处理 Bot API 响应，包括 FLOOD_WAIT 重试
async fn handle_bot_response<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    method: &str,
) -> Result<T, AppError> {
    let status = resp.status();

    // 429 FLOOD_WAIT
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let body = resp.text().await.unwrap_or_default();
        let api_resp: BotApiResponse<T> = serde_json::from_str(&body).unwrap_or(BotApiResponse {
            ok: false,
            result: None,
            description: None,
            error_code: None,
            parameters: None,
        });
        let retry_after = api_resp
            .parameters
            .and_then(|p| p.retry_after)
            .unwrap_or(5);
        return Err(AppError::Internal(format!(
            "FLOOD_WAIT: {method} 请求频率过高，需等待 {retry_after} 秒"
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("读取响应失败: {e}")))?;

    let api_resp: BotApiResponse<T> =
        serde_json::from_str(&body).map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))?;

    if !api_resp.ok {
        let desc = api_resp.description.unwrap_or_default();
        let code = api_resp.error_code.unwrap_or(0);
        return Err(AppError::Internal(format!(
            "Bot API {method} 错误: code={code}, desc={desc}"
        )));
    }

    api_resp
        .result
        .ok_or_else(|| AppError::Internal(format!("Bot API {method} 响应缺少 result")))
}

/// Bot 群组信息（用于下拉选择）
#[derive(Debug, Clone, serde::Serialize)]
pub struct BotChat {
    pub id: i64,
    pub title: String,
    pub chat_type: String,
}

/// getChat 响应
#[derive(Debug, Deserialize)]
struct ChatInfo {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    title: Option<String>,
    username: Option<String>,
    first_name: Option<String>,
}

/// 验证 Bot 是否有权限访问指定 chat_id，返回聊天信息
pub async fn validate_chat(
    token: &str,
    chat_id: &str,
    proxy_url: Option<&str>,
) -> Result<BotChat, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "getChat");

    let resp = client
        .get(&url)
        .query(&[("chat_id", chat_id)])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("getChat 请求失败: {e}")))?;

    let chat_info = handle_bot_response::<ChatInfo>(resp, "getChat").await?;

    let title = chat_info
        .title
        .or(chat_info.username)
        .or(chat_info.first_name)
        .unwrap_or_else(|| format!("Chat {}", chat_info.id));

    Ok(BotChat {
        id: chat_info.id,
        title,
        chat_type: chat_info.chat_type,
    })
}

/// getUpdates 响应中的 Chat 信息
#[derive(Debug, Deserialize)]
struct UpdateChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    title: Option<String>,
    username: Option<String>,
    first_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateMessage {
    chat: UpdateChat,
}

#[derive(Debug, Deserialize)]
struct BotUpdate {
    message: Option<UpdateMessage>,
    #[serde(rename = "my_chat_member")]
    my_chat_member: Option<serde_json::Value>,
}

/// 获取 Bot 所在的群组/频道列表（通过 getUpdates 提取最近活跃的聊天）
pub async fn get_bot_chats(
    token: &str,
    proxy_url: Option<&str>,
) -> Result<Vec<BotChat>, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "getUpdates");

    // 获取最近的 updates（不确认消费，用 offset=-1 不影响其他 bot 使用）
    let resp = client
        .get(&url)
        .query(&[("limit", "100")])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("getUpdates 请求失败: {e}")))?;

    let updates: BotApiResponse<Vec<BotUpdate>> = handle_bot_response_raw(resp).await?;

    let updates = updates.result.unwrap_or_default();

    // 从 updates 中提取去重的群组/频道
    let mut seen = std::collections::HashSet::new();
    let mut chats = Vec::new();

    for update in updates.into_iter().rev() {
        // 从 message 中提取
        if let Some(msg) = update.message {
            let chat = msg.chat;
            // 只保留 group、supergroup、channel
            if chat.chat_type == "group"
                || chat.chat_type == "supergroup"
                || chat.chat_type == "channel"
            {
                if seen.insert(chat.id) {
                    let title = chat
                        .title
                        .or(chat.username)
                        .or(chat.first_name)
                        .unwrap_or_else(|| format!("Chat {}", chat.id));
                    chats.push(BotChat {
                        id: chat.id,
                        title,
                        chat_type: chat.chat_type,
                    });
                }
            }
        }

        // 从 my_chat_member 中提取（Bot 被添加到群组时的事件）
        if let Some(member_update) = update.my_chat_member {
            if let Some(chat_obj) = member_update.get("chat") {
                let chat_id = chat_obj.get("id").and_then(|v| v.as_i64());
                let chat_type = chat_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let chat_title = chat_obj
                    .get("title")
                    .or(chat_obj.get("username"))
                    .and_then(|v| v.as_str());

                if let Some(id) = chat_id {
                    if (chat_type == "group"
                        || chat_type == "supergroup"
                        || chat_type == "channel")
                        && seen.insert(id)
                    {
                        chats.push(BotChat {
                            id,
                            title: chat_title
                                .unwrap_or(&format!("Chat {}", id))
                                .to_string(),
                            chat_type: chat_type.to_string(),
                        });
                    }
                }
            }
        }
    }

    // 如果 getUpdates 没有返回任何群组，尝试通过常见的 chat ID 验证
    // （用户可能还没在群组中发过消息，但 Bot 已被添加）
    Ok(chats)
}

/// 不解析 result 的原始响应处理（用于 getUpdates 等可能返回空数组的场景）
async fn handle_bot_response_raw<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<BotApiResponse<T>, AppError> {
    let status = resp.status();

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let _body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "FLOOD_WAIT: 请求频率过高"
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("读取响应失败: {e}")))?;

    serde_json::from_str(&body)
        .map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))
}
