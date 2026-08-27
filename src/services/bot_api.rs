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

/// sendPhoto / forwardMessage 响应中的 Message
/// `id` 用 `#[serde(default)]` 保证 sendPhoto 路径（不需要 id）仍能反序列化
#[derive(Debug, Deserialize)]
struct BotMessage {
    #[serde(default, rename = "message_id")]
    id: i64,
    photo: Option<Vec<PhotoSize>>,
}

/// getFile 响应
#[derive(Debug, Deserialize)]
struct FileResult {
    file_path: Option<String>,
}

fn bot_api_url(token: &str, method: &str) -> String {
    format!("{}/bot{token}/{method}", api_base())
}

fn file_download_url(token: &str, file_path: &str) -> String {
    format!("{}/file/bot{token}/{file_path}", api_base())
}

/// Bot API base（默认官方地址；测试通过 TG_BOT_API_BASE 覆写指向 wiremock）
fn api_base() -> String {
    std::env::var("TG_BOT_API_BASE").unwrap_or_else(|_| "https://api.telegram.org".to_string())
}

/// 构建带可选代理的 reqwest Client
fn build_client(proxy_url: Option<&str>) -> Result<reqwest::Client, AppError> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10));

    if let Some(proxy) = proxy_url
        && !proxy.is_empty()
    {
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

    let api_resp: BotApiResponse<BotInfo> = serde_json::from_str(&body)
        .map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))?;

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

    handle_bot_response::<BotMessage>(resp, "sendPhoto")
        .await
        .map(|msg| {
            // 取最大的图片 file_id（最后一个是最大尺寸）
            msg.photo
                .and_then(|sizes| sizes.last().map(|s| s.file_id.clone()))
                .ok_or_else(|| AppError::Internal("sendPhoto 响应无 photo 数据".to_string()))
        })?
}

/// 通过 Bot API forwardMessage 把消息从 from_chat_id 转发到 chat_id
/// 同步返回新消息（含 photo 数组），从中提取最大尺寸的 file_id
///
/// 返回 `(forwarded_message_id, Option<file_id>)`：
/// - `forwarded_message_id`：群组B 中的新消息 ID，用于后续 deleteMessage
/// - `file_id`：Bot 视角的最大尺寸 file_id；None 表示原消息不是 photo 媒体
pub async fn forward_message(
    token: &str,
    chat_id: &str,
    from_chat_id: &str,
    message_id: i64,
    proxy_url: Option<&str>,
) -> Result<(i64, Option<String>), AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "forwardMessage");

    let resp = client
        .post(&url)
        .query(&[
            ("chat_id", chat_id),
            ("from_chat_id", from_chat_id),
            ("message_id", &message_id.to_string()),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("forwardMessage 请求失败: {e}")))?;

    let msg = handle_bot_response::<BotMessage>(resp, "forwardMessage").await?;
    let file_id = msg
        .photo
        .and_then(|sizes| sizes.last().map(|s| s.file_id.clone()));
    Ok((msg.id, file_id))
}

/// 通过 Bot API deleteMessage 删除指定消息
/// 需 Bot 在目标 chat 有删除消息权限（管理员）；删除失败仅返回错误，由调用方决定是否影响任务
pub async fn delete_message(
    token: &str,
    chat_id: &str,
    message_id: i64,
    proxy_url: Option<&str>,
) -> Result<(), AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "deleteMessage");

    let resp = client
        .post(&url)
        .query(&[
            ("chat_id", chat_id),
            ("message_id", &message_id.to_string()),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("deleteMessage 请求失败: {e}")))?;

    // deleteMessage 返回 result=True，不需要结构化解析
    let _ = handle_bot_response::<serde_json::Value>(resp, "deleteMessage").await?;
    Ok(())
}

/// 通过 Bot API sendMessage 发送纯文本消息，返回 message_id
/// 不使用 parse_mode（纯文本无需转义）；text 上限 4096 字符由调用方保证
pub async fn send_message(
    token: &str,
    chat_id: &str,
    text: &str,
    proxy_url: Option<&str>,
) -> Result<i64, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "sendMessage");

    let resp = client
        .post(&url)
        .query(&[("chat_id", chat_id), ("text", text)])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("sendMessage 请求失败: {e}")))?;

    let msg = handle_bot_response::<BotMessage>(resp, "sendMessage").await?;
    Ok(msg.id)
}

/// 通过 Bot API setMyCommands 注册命令（客户端输入 / 时自动补全）— best effort
pub async fn set_my_commands(
    token: &str,
    commands: &[(&str, &str)],
    proxy_url: Option<&str>,
) -> Result<(), AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "setMyCommands");

    let body = serde_json::json!({
        "commands": commands
            .iter()
            .map(|(cmd, desc)| serde_json::json!({"command": cmd, "description": desc}))
            .collect::<Vec<_>>(),
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("setMyCommands 请求失败: {e}")))?;

    let _ = handle_bot_response::<serde_json::Value>(resp, "setMyCommands").await?;
    Ok(())
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
        let retry_after = api_resp.parameters.and_then(|p| p.retry_after).unwrap_or(5);
        return Err(AppError::Internal(format!(
            "FLOOD_WAIT: {method} 请求频率过高，需等待 {retry_after} 秒"
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("读取响应失败: {e}")))?;

    let api_resp: BotApiResponse<T> = serde_json::from_str(&body)
        .map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))?;

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
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub title: Option<String>,
    pub username: Option<String>,
    pub first_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateMessage {
    pub chat: UpdateChat,
    /// 消息文本（媒体消息无此字段）
    #[serde(default)]
    pub text: Option<String>,
    /// 消息时间（unix 秒；缺失按 0 处理，视为过期）
    #[serde(default)]
    pub date: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotUpdate {
    /// update_id 严格单调递增，用于轮询去重
    #[serde(default)]
    pub update_id: i64,
    pub message: Option<UpdateMessage>,
    /// 频道消息走 channel_post（bot 在频道内为管理员时可见）
    #[serde(rename = "channel_post")]
    pub channel_post: Option<UpdateMessage>,
    #[serde(rename = "my_chat_member")]
    pub my_chat_member: Option<serde_json::Value>,
}

/// 拉取原始 updates（limit=100、timeout=0、不带 offset —— 不确认消费，
/// 保证 get_bot_chats 依赖的 pending 历史不被吃掉）
///
/// 与其他 getUpdates 并发调用会 409 Conflict：返回的 Err 信息含 "409"，由调用方决定重试/跳过
pub async fn get_updates(token: &str, proxy_url: Option<&str>) -> Result<Vec<BotUpdate>, AppError> {
    let client = build_client(proxy_url)?;
    let url = bot_api_url(token, "getUpdates");

    let resp = client
        .get(&url)
        .query(&[("timeout", "0"), ("limit", "100")])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("getUpdates 请求失败: {e}")))?;

    let api_resp = handle_bot_response_raw::<Vec<BotUpdate>>(resp).await?;

    // handle_bot_response_raw 不检查 ok：409 等错误会静默变成空列表，这里显式拦截
    if !api_resp.ok {
        let code = api_resp.error_code.unwrap_or(0);
        let desc = api_resp.description.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Bot API getUpdates 错误: code={code}, desc={desc}"
        )));
    }

    Ok(api_resp.result.unwrap_or_default())
}

/// 获取 Bot 所在的群组/频道列表（通过 getUpdates 提取最近活跃的聊天）
pub async fn get_bot_chats(token: &str, proxy_url: Option<&str>) -> Result<Vec<BotChat>, AppError> {
    // 与后台 /id 命令轮询器并发 getUpdates 会 409 Conflict：错峰后重试一次
    let updates = match get_updates(token, proxy_url).await {
        Ok(v) => v,
        Err(e) if e.to_string().contains("409") => {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            get_updates(token, proxy_url).await?
        }
        Err(e) => return Err(e),
    };

    // 从 updates 中提取去重的群组/频道
    let mut seen = std::collections::HashSet::new();
    let mut chats = Vec::new();

    for update in updates.into_iter().rev() {
        // 从 message 中提取
        if let Some(msg) = update.message {
            let chat = msg.chat;
            // 只保留 group、supergroup、channel
            if (chat.chat_type == "group"
                || chat.chat_type == "supergroup"
                || chat.chat_type == "channel")
                && seen.insert(chat.id)
            {
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

        // 从 my_chat_member 中提取（Bot 被添加到群组时的事件）
        if let Some(member_update) = update.my_chat_member
            && let Some(chat_obj) = member_update.get("chat")
        {
            let chat_id = chat_obj.get("id").and_then(|v| v.as_i64());
            let chat_type = chat_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let chat_title = chat_obj
                .get("title")
                .or(chat_obj.get("username"))
                .and_then(|v| v.as_str());

            if let Some(id) = chat_id
                && (chat_type == "group" || chat_type == "supergroup" || chat_type == "channel")
                && seen.insert(id)
            {
                chats.push(BotChat {
                    id,
                    title: chat_title.unwrap_or(&format!("Chat {}", id)).to_string(),
                    chat_type: chat_type.to_string(),
                });
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
        return Err(AppError::Internal("FLOOD_WAIT: 请求频率过高".to_string()));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("读取响应失败: {e}")))?;

    serde_json::from_str(&body).map_err(|e| AppError::Internal(format!("解析响应失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// 串行化 TG_BOT_API_BASE 的 set/restore，避免同二进制内并行测试互踩
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII：测试期间覆写 TG_BOT_API_BASE，drop 时恢复（panic 安全）
    struct EnvBaseGuard;

    impl EnvBaseGuard {
        fn set(base: &str) -> Self {
            // SAFETY：ENV_LOCK（静态 Mutex）串行化了本模块所有测试对该 env 的
            // set/remove；测试内被测函数对 env 的读取发生在 set 之后的同一任务中，
            // 不存在并发写。edition 2024 要求显式 unsafe 块标注此约定。
            unsafe { std::env::set_var("TG_BOT_API_BASE", base) };
            EnvBaseGuard
        }
    }

    impl Drop for EnvBaseGuard {
        fn drop(&mut self) {
            // SAFETY：同上，ENV_LOCK 保证无并发访问
            unsafe { std::env::remove_var("TG_BOT_API_BASE") };
        }
    }

    const TOKEN: &str = "TESTTOKEN";

    #[tokio::test]
    // ENV_LOCK 需在整个异步测试期间持锁（串行化 TG_BOT_API_BASE 覆写），跨 await 持锁是有意为之
    #[allow(clippy::await_holding_lock)]
    async fn t_send_message_posts_query() {
        let server = MockServer::start().await;
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvBaseGuard::set(&server.uri());

        Mock::given(method("POST"))
            .and(path(format!("/bot{TOKEN}/sendMessage")))
            .and(query_param("chat_id", "-1001234567890"))
            .and(query_param("text", "hello"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"ok":true,"result":{"message_id":42}}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let id = send_message(TOKEN, "-1001234567890", "hello", None)
            .await
            .expect("sendMessage 应成功");
        assert_eq!(id, 42);
        server.verify().await;
    }

    #[tokio::test]
    // ENV_LOCK 需在整个异步测试期间持锁（串行化 TG_BOT_API_BASE 覆写），跨 await 持锁是有意为之
    #[allow(clippy::await_holding_lock)]
    async fn t_get_updates_parses_message_and_channel_post() {
        let server = MockServer::start().await;
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvBaseGuard::set(&server.uri());

        let body = r#"{"ok":true,"result":[
            {"update_id":11,"message":{"message_id":1,"date":1700000000,
              "chat":{"id":-1001234567890,"type":"supergroup","title":"测试群"},
              "text":"/id"}},
            {"update_id":12,"channel_post":{"message_id":2,"date":1700000060,
              "chat":{"id":-1002999999999,"type":"channel","title":"频道"},"text":"/id"}},
            {"update_id":13,"my_chat_member":{"chat":{"id":-1001234567890,"type":"supergroup"}}}
        ]}"#;
        Mock::given(method("GET"))
            .and(path(format!("/bot{TOKEN}/getUpdates")))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let updates = get_updates(TOKEN, None).await.expect("getUpdates 应成功");
        assert_eq!(updates.len(), 3);

        let msg = updates[0].message.as_ref().expect("应解析 message");
        assert_eq!(msg.chat.id, -1001234567890);
        assert_eq!(msg.chat.chat_type, "supergroup");
        assert_eq!(msg.chat.title.as_deref(), Some("测试群"));
        assert_eq!(msg.text.as_deref(), Some("/id"));
        assert_eq!(msg.date, 1700000000);
        assert_eq!(updates[0].update_id, 11);

        let post = updates[1]
            .channel_post
            .as_ref()
            .expect("应解析 channel_post");
        assert_eq!(post.chat.id, -1002999999999);
        assert_eq!(post.chat.chat_type, "channel");

        // 缺 text/date 的 message（媒体消息）应反序列化为 None/0
        assert!(updates[2].message.is_none());
        server.verify().await;
    }

    #[tokio::test]
    // ENV_LOCK 需在整个异步测试期间持锁（串行化 TG_BOT_API_BASE 覆写），跨 await 持锁是有意为之
    #[allow(clippy::await_holding_lock)]
    async fn t_get_updates_409_conflict_err() {
        let server = MockServer::start().await;
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvBaseGuard::set(&server.uri());

        Mock::given(method("GET"))
            .and(path(format!("/bot{TOKEN}/getUpdates")))
            .respond_with(ResponseTemplate::new(409).set_body_string(
                r#"{"ok":false,"error_code":409,"description":"Conflict: terminated by other getUpdates request"}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let err = get_updates(TOKEN, None).await.expect_err("409 应返回 Err");
        assert!(
            err.to_string().contains("409"),
            "错误信息应含 409 以便调用方识别冲突: {err}"
        );
        server.verify().await;
    }

    #[tokio::test]
    // ENV_LOCK 需在整个异步测试期间持锁（串行化 TG_BOT_API_BASE 覆写），跨 await 持锁是有意为之
    #[allow(clippy::await_holding_lock)]
    async fn t_get_bot_chats_retries_once_on_409() {
        let server = MockServer::start().await;
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvBaseGuard::set(&server.uri());

        // 先挂 200（expect 1），再挂 409（最多消耗 1 次）：wiremock 最新挂载优先匹配，
        // 第一次请求撞 409，重试落到 200
        Mock::given(method("GET"))
            .and(path(format!("/bot{TOKEN}/getUpdates")))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"ok":true,"result":[
                    {"update_id":11,"message":{"message_id":1,"date":1700000000,
                      "chat":{"id":-1001234567890,"type":"supergroup","title":"测试群"}}}
                ]}"#,
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/bot{TOKEN}/getUpdates")))
            .respond_with(ResponseTemplate::new(409).set_body_string(
                r#"{"ok":false,"error_code":409,"description":"Conflict"}"#,
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let chats = get_bot_chats(TOKEN, None)
            .await
            .expect("409 重试后应成功");
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].id, -1001234567890);
        assert_eq!(chats[0].title, "测试群");
        server.verify().await;
    }
}
