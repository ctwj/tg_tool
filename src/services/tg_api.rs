// Telegram API wrapper using grammers-client 0.7
// Provides high-level operations for interacting with Telegram

use crate::errors::AppError;
use crate::state::{PeerCache, TgClientMap};

/// Cache TTL for peer resolution (5 minutes)
const PEER_CACHE_TTL_SECS: u64 = 300;

/// 将 chat_id 归一化为两种可能格式的候选数组。
///
/// Telegram 群组/频道 ID 有两种表示：
/// - **Bot API / Web 格式**：超级群和频道带 `-100` 前缀（如 `-1002859432332`）
/// - **MTProto 原始格式**：`grammers-client` 的 `Chat::id()` 返回不带前缀的正数（如 `2859432332`）
///
/// 用户在 UI 配置的 `ImageGroupChatId` 通常是 Bot API 格式，
/// 而 `iter_dialogs()` 遍历出的 `Chat::id()` 是 MTProto 原始格式。
/// 不做归一化会导致永远匹配不到。
fn id_candidates(chat_id: i64) -> [i64; 2] {
    const MARKER: i64 = 1_000_000_000_000; // -100 前缀的阈值
    if chat_id <= -MARKER {
        // -100xxx 格式 → 同时接受原始正数
        [chat_id, -chat_id - MARKER]
    } else if chat_id > 0 && chat_id < MARKER {
        // 原始正数（可能是 channel_id）→ 同时接受 -100xxx 格式
        [chat_id, -(chat_id + MARKER)]
    } else {
        [chat_id, chat_id]
    }
}

#[cfg(test)]
mod id_candidates_tests {
    use super::id_candidates;

    #[test]
    fn test_bot_api_format() {
        // -1002859432332 → 同时匹配 2859432332
        assert_eq!(id_candidates(-1002859432332), [-1002859432332, 2859432332]);
    }

    #[test]
    fn test_mtproto_raw_format() {
        // 2859432332 → 同时匹配 -1002859432332
        assert_eq!(id_candidates(2859432332), [2859432332, -1002859432332]);
    }

    #[test]
    fn test_user_id_unchanged() {
        // 普通用户 ID（正数小值）→ 不会错误加 -100 前缀
        // 注意：用户 ID < MARKER 会被当作可能 channel_id，但比较时仍包含原值
        assert_eq!(id_candidates(12345678), [12345678, -1000012345678]);
    }

    #[test]
    fn test_negative_chat_id() {
        // 普通群（Chat）的负 ID（如 -12345678）→ 保持不变
        // 这个范围 > -MARKER，不会触发 -100 解析
        assert_eq!(id_candidates(-12345678), [-12345678, -12345678]);
    }
}

/// Chat information
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatInfo {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub chat_type: String,
}

/// Get chat list for a client
pub async fn get_chat_list(
    client_id: &str,
    tg_clients: &TgClientMap,
) -> Result<Vec<ChatInfo>, AppError> {
    let clients = tg_clients.read().await;
    let client = clients
        .get(client_id)
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::NotFound("客户端未连接".into()))?;
    drop(clients);

    let mut dialogs = client.iter_dialogs();
    let mut chats = Vec::new();

    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("获取聊天列表失败: {e}")))?
    {
        let chat = dialog.chat();
        let id = chat.id();
        let name = chat.name().to_string();
        let chat_type = match chat {
            grammers_client::types::Chat::User(_) => "private".to_string(),
            grammers_client::types::Chat::Group(_) => "group".to_string(),
            grammers_client::types::Chat::Channel(_) => "channel".to_string(),
        };

        chats.push(ChatInfo {
            id,
            name,
            chat_type,
        });
    }

    Ok(chats)
}

/// Send a message to a chat using any available active client
pub async fn send_message(
    chat_id: i64,
    text: &str,
    tg_clients: &TgClientMap,
) -> Result<(), AppError> {
    let clients = tg_clients.read().await;
    let client = clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::NotFound("没有可用的在线客户端".into()))?;
    drop(clients);

    // Resolve peer by searching dialogs
    let mut dialogs = client.iter_dialogs();
    let mut target_packed = None;

    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("搜索目标聊天失败: {e}")))?
    {
        if dialog.chat().id() == chat_id {
            target_packed = Some(dialog.chat().pack());
            break;
        }
    }

    let packed =
        target_packed.ok_or_else(|| AppError::NotFound(format!("未找到目标聊天: {chat_id}")))?;

    client
        .send_message(packed, text)
        .await
        .map_err(|e| AppError::Internal(format!("发送消息失败: {e}")))?;

    Ok(())
}

/// Get current user info — returns cached UserInfo if available
pub async fn get_me(
    client_id: &str,
    tg_clients: &TgClientMap,
) -> Result<serde_json::Value, AppError> {
    let clients = tg_clients.read().await;
    let entry = clients
        .get(client_id)
        .ok_or_else(|| AppError::NotFound("客户端不存在".into()))?;

    let connected = entry.client.is_some();
    let user_info = entry.user_info.as_ref().map(|info| {
        serde_json::json!({
            "user_id": info.user_id,
            "username": info.username,
            "first_name": info.first_name,
            "last_name": info.last_name,
            "is_bot": info.is_bot,
        })
    });

    drop(clients);

    Ok(serde_json::json!({
        "connected": connected,
        "user_info": user_info,
    }))
}

/// Resolve a chat_id to a PackedChat, using peer cache with TTL
pub async fn resolve_peer(
    chat_id: i64,
    tg_clients: &TgClientMap,
    peer_cache: &PeerCache,
) -> Result<grammers_client::types::PackedChat, AppError> {
    // Check cache first
    {
        let cache = peer_cache.read().await;
        if let Some((packed, cached_at)) = cache.get(&chat_id)
            && cached_at.elapsed().as_secs() < PEER_CACHE_TTL_SECS
        {
            return Ok(*packed);
        }
    }

    // Cache miss or expired — resolve via dialogs
    let clients = tg_clients.read().await;
    let client = clients
        .values()
        .find(|e| e.status == "active" && e.client.is_some())
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::NotFound("没有可用的在线客户端".into()))?;
    drop(clients);

    // ID 归一化：同时匹配 Bot API 格式（-100xxx）和 MTProto 原始格式（正数）
    let candidates = id_candidates(chat_id);

    let mut dialogs = client.iter_dialogs();
    let mut found = None;
    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|e| AppError::Internal(format!("搜索目标聊天失败: {e}")))?
    {
        let id = dialog.chat().id();
        if candidates.contains(&id) {
            found = Some(dialog.chat().pack());
            break;
        }
    }

    let packed = found.ok_or_else(|| AppError::NotFound(format!("未找到目标聊天: {chat_id}")))?;

    // Update cache
    {
        let mut cache = peer_cache.write().await;
        cache.insert(chat_id, (packed, std::time::Instant::now()));
    }

    Ok(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_info_serialization() {
        let info = ChatInfo {
            id: -1001234567890,
            name: "Test Channel".to_string(),
            chat_type: "channel".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("Test Channel"));
        assert!(json.contains("channel"));
    }

    #[test]
    fn test_chat_info_types() {
        let types = vec!["private", "group", "supergroup", "channel"];
        for t in types {
            let info = ChatInfo {
                id: 123,
                name: "Test".to_string(),
                chat_type: t.to_string(),
            };
            assert_eq!(info.chat_type, t);
        }
    }
}
