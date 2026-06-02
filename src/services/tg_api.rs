// Telegram API wrapper using grammers-client 0.7
// Provides high-level operations for interacting with Telegram

use crate::errors::AppError;
use crate::state::TgClientMap;

/// Chat information
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatInfo {
    pub id: i64,
    pub name: String,
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

    let packed = target_packed
        .ok_or_else(|| AppError::NotFound(format!("未找到目标聊天: {chat_id}")))?;

    client
        .send_message(packed, text)
        .await
        .map_err(|e| AppError::Internal(format!("发送消息失败: {e}")))?;

    Ok(())
}

/// Get current user info
pub async fn get_me(
    client_id: &str,
    tg_clients: &TgClientMap,
) -> Result<serde_json::Value, AppError> {
    let clients = tg_clients.read().await;
    let client = clients
        .get(client_id)
        .and_then(|e| e.client.clone())
        .ok_or_else(|| AppError::NotFound("客户端未连接".into()))?;
    drop(clients);

    // Verify connection works by trying to get dialogs
    let mut dialogs = client.iter_dialogs();
    if let Some(_dialog) = dialogs.next().await.map_err(|e| AppError::Internal(format!("获取信息失败: {e}")))? {
        Ok(serde_json::json!({ "connected": true }))
    } else {
        Ok(serde_json::json!({ "connected": true, "dialogs": 0 }))
    }
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
