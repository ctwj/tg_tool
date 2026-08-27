//! Bot /id 命令监听器 — 群组内发送 `/id`，Bot 直接回复当前会话 Chat ID
//!
//! 背景：图床配置需要"图床群组 chat id"，但 Bot 拉进群组后用户无从获知
//! （`bot-chats` 下拉只显示最近活跃群，静默群不出现）。
//!
//! 实现：后台任务每 POLL_INTERVAL_SECS 对所有 Bot 类型客户端调 getUpdates
//! （不带 offset、不确认消费，与 `get_bot_chats` 共用同一 pending 池、互不吞数据），
//! 检测 `/id` 命令后用 Bot API sendMessage 在原会话回复 chat id。
//! getUpdates 返回的 chat.id 即 Bot API 格式（-100 前缀），与图床配置消费格式一致，无需转换。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::services::bot_api::{self, BotUpdate, UpdateChat};
use crate::state::{AppState, DbPool};

/// 轮询间隔（秒）：单 bot 每 10s 一次 getUpdates，远低于 Bot API 频率限制
const POLL_INTERVAL_SECS: u64 = 10;
/// 只回复最近 N 秒内的 /id 消息（防止服务重启后对 pending 旧消息重复刷屏）
const FRESH_WINDOW_SECS: i64 = 600;
/// 单 bot 连续失败后的退避上限（秒）
const MAX_BACKOFF_SECS: u64 = 60;

/// 监听器运行时状态（仿 `crawler/scheduler.rs` 的 State + CancellationToken 模式）
#[derive(Debug)]
pub struct BotCommandListenerState {
    pub running: bool,
    pub handle: Option<tokio::task::JoinHandle<()>>,
    pub cancel: Option<CancellationToken>,
}

/// 全局共享句柄（参考现有 `SchedulerHandle` 模式）
pub type BotCommandListenerHandle = Arc<RwLock<BotCommandListenerState>>;

/// 创建监听器句柄（未启动状态）
pub fn create_bot_command_listener() -> BotCommandListenerHandle {
    Arc::new(RwLock::new(BotCommandListenerState {
        running: false,
        handle: None,
        cancel: None,
    }))
}

/// 启动监听器（若已在运行直接返回）
pub async fn start_bot_command_listener(l: BotCommandListenerHandle, state: AppState) {
    let mut s = l.write().await;
    if s.running {
        return;
    }
    let cancel = CancellationToken::new();
    s.cancel = Some(cancel.clone());
    s.running = true;
    let handle = tokio::spawn(run_loop(state, cancel));
    s.handle = Some(handle);
    tracing::info!("Bot /id 命令监听器启动 ({}s tick)", POLL_INTERVAL_SECS);
}

/// 停止监听器（优雅关闭序列调用）
pub async fn stop_bot_command_listener(l: BotCommandListenerHandle) {
    let mut state = l.write().await;
    if let Some(cancel) = state.cancel.take() {
        cancel.cancel();
    }
    if let Some(handle) = state.handle.take() {
        handle.abort();
    }
    state.running = false;
}

async fn run_loop(state: AppState, cancel: CancellationToken) {
    let duration = Duration::from_secs(POLL_INTERVAL_SECS);
    // 每 bot 游标仅存于本 loop（无需持久化：重启后由 FRESH_WINDOW_SECS 时间窗兜底）
    let mut cursors: HashMap<String, BotCursor> = HashMap::new();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(duration) => {
                if let Err(e) = tick(&state, &mut cursors).await {
                    tracing::warn!("Bot /id 监听 tick 失败: {e}");
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!("Bot /id 命令监听器已停止");
                break;
            }
        }
    }
}

/// 每 bot 的轮询游标与退避状态（pub 供集成测试直接调 `tick` 构造与断言）
#[derive(Debug, Default, Clone)]
pub struct BotCursor {
    /// 已见最大 update_id（getUpdates 按 update_id 升序追加，单调去重 O(1)、无需清理）
    pub max_seen_update_id: i64,
    /// 连续失败次数（成功清零）
    pub consec_failures: u32,
    /// 退避截止时间（None = 不退避）
    pub skip_until: Option<Instant>,
}

/// DB 里的 Bot 客户端行
struct BotRow {
    id: String,
    token: String,
    username: Option<String>,
}

/// 单次 tick：扫描所有 Bot 客户端的 updates，对新的 /id 命令回复 chat id
/// （pub 供集成测试直接调用，不必等轮询周期）
pub async fn tick(
    state: &AppState,
    cursors: &mut HashMap<String, BotCursor>,
) -> Result<(), String> {
    let bots = fetch_bots(&state.db).await?;
    if bots.is_empty() {
        return Ok(());
    }

    // Bot API 走 HTTP 代理（http_proxy_url 优先，回退 proxy_url，同 add_client 惯例）
    let proxy_url = state.http_proxy_url().await;
    let proxy_url = if proxy_url.is_none() {
        state.proxy_url().await
    } else {
        proxy_url
    };
    let now = chrono::Utc::now().timestamp();

    for bot in bots {
        let cur = cursors.entry(bot.id.clone()).or_default();
        if let Some(t) = cur.skip_until
            && Instant::now() < t
        {
            continue; // 退避中
        }

        let updates = match bot_api::get_updates(&bot.token, proxy_url.as_deref()).await {
            Ok(v) => v,
            Err(e) => {
                // 409（与 get_bot_chats 并发）、webhook 冲突与瞬时网络错误都按本 tick 跳过；
                // 连续失败指数退避（2→4→…→60s 封顶）防刷日志
                cur.consec_failures += 1;
                let backoff = MAX_BACKOFF_SECS.min(1u64 << cur.consec_failures.min(6));
                cur.skip_until = Some(Instant::now() + Duration::from_secs(backoff));
                // 首次失败 warn（生产 RUST_LOG=info 可见——"群里发 /id 没反应"先看这里）；
                // 持续失败每 30 次（约 30 分钟）再提醒一次，其余 debug 防刷屏
                if cur.consec_failures == 1 || cur.consec_failures.is_multiple_of(30) {
                    tracing::warn!(
                        "bot {} getUpdates 失败(连续 {} 次，退避 {}s): {e}",
                        bot.id,
                        cur.consec_failures,
                        backoff
                    );
                } else {
                    tracing::debug!(
                        "bot {} getUpdates 失败(连续 {} 次): {e}",
                        bot.id,
                        cur.consec_failures
                    );
                }
                continue;
            }
        };
        cur.consec_failures = 0;
        cur.skip_until = None;

        // username 缺失（migration 017 之前的旧行）→ getMe 惰性补一次，仅内存使用不写库
        let username = match bot.username.as_deref() {
            Some(u) if !u.is_empty() => Some(u.to_string()),
            _ => bot_api::validate_token(&bot.token, proxy_url.as_deref())
                .await
                .ok()
                .and_then(|i| i.username),
        };

        // 先推进游标再发送：失败不重试（用户重发 /id 触发新 update），避免失败刷屏
        let replies = extract_id_replies(updates, cur, username.as_deref(), now);
        for chat in replies {
            let text = build_reply_text(&chat);
            match bot_api::send_message(
                &bot.token,
                &chat.id.to_string(),
                &text,
                proxy_url.as_deref(),
            )
            .await
            {
                Ok(_) => {
                    tracing::info!("bot {} 在 chat {} 回复了 /id 查询", bot.id, chat.id)
                }
                Err(e) => tracing::warn!("bot {} 回复 /id 失败(chat {}): {e}", bot.id, chat.id),
            }
        }
    }
    Ok(())
}

/// 从一批 updates 中筛出需要回复的会话（去重 + 命令匹配 + 时间窗），同时推进游标
fn extract_id_replies(
    updates: Vec<BotUpdate>,
    cur: &mut BotCursor,
    username: Option<&str>,
    now: i64,
) -> Vec<UpdateChat> {
    let mut out = Vec::new();
    for u in updates {
        if u.update_id <= cur.max_seen_update_id {
            continue;
        }
        cur.max_seen_update_id = cur.max_seen_update_id.max(u.update_id);

        // 群组消息走 message，频道消息走 channel_post
        let Some(msg) = u.message.or(u.channel_post) else {
            continue;
        };
        let is_cmd = msg
            .text
            .as_deref()
            .is_some_and(|t| is_id_command(t, username));
        if !is_cmd {
            continue;
        }
        // 时间窗：超窗旧消息（如重启后 pending 残留）不回复
        if msg.date <= 0 || now - msg.date > FRESH_WINDOW_SECS {
            continue;
        }
        out.push(msg.chat);
    }
    out
}

/// 判断消息首 token 是否为指向本 bot 的 /id 命令
/// - `/id`、`/id@本bot`（大小写不敏感）→ true
/// - `/id@其他bot`、`/idx`、正文中间出现 /id → false
/// - username 缺失时只认裸 `/id`，绝不响应 `/id@别人`
fn is_id_command(text: &str, bot_username: Option<&str>) -> bool {
    // split_whitespace 自身忽略首尾空白，无需 trim
    let Some(first) = text.split_whitespace().next() else {
        return false;
    };
    let first = first.to_lowercase();
    if first == "/id" {
        return true;
    }
    match bot_username.filter(|u| !u.is_empty()) {
        Some(u) => first == format!("/id@{}", u.to_lowercase()),
        None => false,
    }
}

/// 构造 /id 回复文本（纯文本，无 parse_mode）
fn build_reply_text(chat: &UpdateChat) -> String {
    let name = chat
        .title
        .clone()
        .or_else(|| chat.username.clone())
        .or_else(|| chat.first_name.clone())
        .unwrap_or_else(|| format!("Chat {}", chat.id));
    format!(
        "本会话 Chat ID: {}\n名称: {}\n类型: {}\n\n此 ID 为 Bot API 格式，可直接填入图床配置的群组输入框",
        chat.id, name, chat.chat_type
    )
}

async fn fetch_bots(db: &DbPool) -> Result<Vec<BotRow>, String> {
    use sqlx::Row;
    let sql = "SELECT id, token, username FROM clients \
               WHERE client_type = 'Bot' AND token IS NOT NULL AND token != ''";
    let map_row = |r: sqlx::sqlite::SqliteRow| BotRow {
        id: r.get("id"),
        token: r.get("token"),
        username: r.get("username"),
    };
    let map_row_pg = |r: sqlx::postgres::PgRow| BotRow {
        id: r.get("id"),
        token: r.get("token"),
        username: r.get("username"),
    };
    let rows = match db {
        DbPool::Sqlite(pool) => sqlx::query(sql)
            .map(map_row)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?,
        DbPool::Postgres(pool) => sqlx::query(sql)
            .map(map_row_pg)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?,
    };
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(id: i64, chat_type: &str, title: Option<&str>) -> UpdateChat {
        UpdateChat {
            id,
            chat_type: chat_type.to_string(),
            title: title.map(|s| s.to_string()),
            username: None,
            first_name: None,
        }
    }

    fn msg_update(update_id: i64, text: &str, date: i64, chat: UpdateChat) -> BotUpdate {
        BotUpdate {
            update_id,
            message: Some(crate::services::bot_api::UpdateMessage {
                chat,
                text: Some(text.to_string()),
                date,
            }),
            channel_post: None,
            my_chat_member: None,
        }
    }

    #[test]
    fn t_is_id_command_matrix() {
        // 裸命令与指向本 bot 的命令
        assert!(is_id_command("/id", Some("MyBot")));
        assert!(is_id_command("/id@MyBot", Some("MyBot")));
        assert!(is_id_command("/ID@mybot", Some("MyBot"))); // 大小写不敏感
        assert!(is_id_command("  /id  ", None)); // 首尾空白容忍
        assert!(is_id_command("/id\n后续内容", None)); // 命令后跟正文

        // 不应响应
        assert!(!is_id_command("/id@OtherBot", Some("MyBot"))); // 指向别的 bot
        assert!(!is_id_command("/idx", None)); // 前缀相同但命令不同
        assert!(!is_id_command("/identify", Some("MyBot")));
        assert!(!is_id_command("hello /id", None)); // 非首 token
        assert!(!is_id_command("", None));
        assert!(!is_id_command("/start", None));

        // username 缺失时只认裸 /id，绝不响应带 @ 的
        assert!(is_id_command("/id", None));
        assert!(!is_id_command("/id@anyone", None));
    }

    #[test]
    fn t_build_reply_text_contains_id_and_title() {
        let text = build_reply_text(&chat(-1001234567890, "supergroup", Some("我的图床群")));
        assert!(text.contains("-1001234567890"));
        assert!(text.contains("我的图床群"));
        assert!(text.contains("supergroup"));

        // title 缺失时回退到 Chat {id}
        let text = build_reply_text(&chat(12345, "private", None));
        assert!(text.contains("12345"));
        assert!(text.contains("Chat 12345"));
    }

    #[test]
    fn t_extract_id_replies_dedup_and_filters() {
        let now = 1_700_000_600i64;
        let target = chat(-1001234567890, "supergroup", Some("测试群"));
        let updates = vec![
            msg_update(10, "/id", now - 5, chat(-100999, "group", Some("别的群"))),
            msg_update(11, "/hello", now - 5, chat(-100888, "group", None)),
            msg_update(12, "/id@MyBot", now - 10, target.clone()),
            msg_update(13, "/id", now - 700, chat(-100777, "group", None)), // 超时间窗
        ];

        let mut cur = BotCursor::default();
        let replies = extract_id_replies(updates.clone(), &mut cur, Some("MyBot"), now);
        // update 10（别的群的裸 /id）与 12（指向本 bot）命中；11 非命令、13 超窗
        assert_eq!(replies.len(), 2, "应命中 update 10 与 12");
        assert_eq!(replies[0].id, -100999);
        assert_eq!(replies[1].id, -1001234567890);
        assert_eq!(cur.max_seen_update_id, 13);

        // 同一批再跑一遍：游标已推进，无重复回复
        let again = extract_id_replies(updates, &mut cur, Some("MyBot"), now);
        assert!(again.is_empty(), "重复批次不应再产生回复");

        // 游标单调：只处理 update_id 更大的新消息
        let newer = extract_id_replies(
            vec![msg_update(14, "/id", now - 1, chat(-100666, "group", None))],
            &mut cur,
            None,
            now,
        );
        assert_eq!(newer.len(), 1);
    }

    #[test]
    fn t_extract_id_replies_channel_post() {
        let now = 1_700_000_600i64;
        let u = BotUpdate {
            update_id: 20,
            message: None,
            channel_post: Some(crate::services::bot_api::UpdateMessage {
                chat: chat(-1002999999999, "channel", Some("频道")),
                text: Some("/id".to_string()),
                date: now - 3,
            }),
            my_chat_member: None,
        };
        let mut cur = BotCursor::default();
        let replies = extract_id_replies(vec![u], &mut cur, None, now);
        assert_eq!(replies.len(), 1, "channel_post 的 /id 也应回复");
        assert_eq!(replies[0].id, -1002999999999);
    }
}
