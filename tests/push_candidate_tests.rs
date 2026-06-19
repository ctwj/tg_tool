//! Feature 041 集成测试 — 推送候选集漏数据修复
//!
//! 验证 4 个核心修复点（TDD 红测试，先全部失败，再随实现逐个变绿）：
//! - T004: 多配置场景资源能被独立推送（候选 SQL 废弃 is_pushed 全局过滤）
//! - T005: failed → pushed 状态转换（ON CONFLICT DO UPDATE）
//! - T006: 候选 SQL ORDER BY + remaining_count 字段
//! - T007: 死信清理后 img 置空的资源仍可被推送

use axum::body::Body;
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;
use tgTool::config::Config;
use tgTool::services::crypto;
use tgTool::state::{AppState, DbPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

// ─── 测试基础设施（与 api_integration.rs 同范式） ───────────────────────────────

/// 创建测试 SQLite 内存数据库（含全部 migrations + root 用户）
async fn setup_test_db() -> DbPool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test SQLite pool");

    let migration_sql = include_str!("../migrations/001_init_sqlite.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("Failed to run test migrations");

    for n in &[
        "002_collector_client_id_sqlite.sql",
        "003_extracted_resources_sqlite.sql",
        "004_collector_histories_is_extracted_sqlite.sql",
        "005_add_share_ids_sqlite.sql",
        "009_extract_histories_sqlite.sql",
        "008_image_tables_sqlite.sql",
        "010_rule_filter_sqlite.sql",
        "011_rule_source_client_sqlite.sql",
        "012_push_configs_sqlite.sql",
        "013_resource_link_check_sqlite.sql",
        "014_push_config_link_check_sqlite.sql",
        "015_forward_task_message_id_sqlite.sql",
        "016_users_must_change_password_sqlite.sql",
        "017_client_name_username_sqlite.sql",
    ] {
        let path = format!("migrations/{n}");
        let sql = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
        let _ = sqlx::raw_sql(&sql).execute(&pool).await;
    }

    let hash = crypto::hash_password("123456").expect("Failed to hash root password");
    sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', ?, 100, 1)")
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("Failed to insert root user");

    DbPool::Sqlite(pool)
}

fn make_test_state(db: DbPool) -> (AppState, tgTool::state::TgClientMap) {
    let config = Config {
        port: 3000,
        log_dir: None,
        rust_log: "warn".to_string(),
        tg_store: PathBuf::from("./test_tg_store"),
        tg_app_id: 12345,
        tg_app_hash: "testhash".to_string(),
        sql_dsn: String::new(),
        redis_conn_string: String::new(),
        session_secret: "test-secret-for-integration".to_string(),
        rate_limit_max: 10000,
        rate_limit_window_secs: 60,
    };
    let tg_clients =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let option_cache =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let peer_cache =
        std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let tg_manager = std::sync::Arc::new(tgTool::services::tg_manager::TgManager::new(
        config.clone(),
        db.clone(),
        tg_clients.clone(),
        option_cache,
        peer_cache,
    ));
    let state = AppState::new(
        db,
        config,
        tg_manager,
        std::path::PathBuf::from("image_cache"),
    );
    (state, tg_clients)
}

fn build_test_app(state: AppState) -> axum::Router {
    tgTool::routes::build_router(state).layer(tgTool::middleware::cors::cors_layer())
}

async fn parse_body(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn build_request(method: &str, uri: &str, body: Option<String>) -> axum::http::Request<Body> {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, Body::from))
        .unwrap()
}

fn build_auth_request(
    method: &str,
    uri: &str,
    token: &str,
    body: Option<String>,
) -> axum::http::Request<Body> {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, Body::from))
        .unwrap()
}

async fn get_root_token(app: &mut axum::Router) -> String {
    let req = build_request(
        "POST",
        "/api/auth/login",
        Some(r#"{"username":"root","password":"123456"}"#.to_string()),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["success"], true, "login failed: {body}");
    body["data"]["token"].as_str().unwrap().to_string()
}

/// 启动一个本地 HTTP mock 服务器，对所有 POST 请求返回 200 OK + `{"code":0}`
/// 用于接收 push_for_config 的推送请求。返回 mock 服务的 URL 和 JoinHandle（测试结束后 drop 即可）。
async fn start_mock_push_server_ok() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/push");
    let handle = tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 8192];
                    // 读一次请求（最多等到 1s 超时，避免卡死）
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        socket.read(&mut buf),
                    )
                    .await;
                    let body = r#"{"code":0}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        }
    });
    (url, handle)
}

/// 插入一条 extracted_resource，返回其自增 id。
/// - url=None 时写入 NULL（用于触发 EmptyResource 分类）
/// - img=None 时写入 NULL
async fn insert_resource(
    pool: &sqlx::SqlitePool,
    title: &str,
    url: Option<&str>,
    img: Option<&str>,
    created_at: Option<&str>,
) -> i64 {
    let created_v = created_at.unwrap_or("2026-06-10 12:00:00");
    sqlx::query(
        "INSERT INTO extracted_resources (collector_history_id, title, url, img, source, extract_mode, is_pushed, created_at, updated_at) \
         VALUES (1, ?, ?, ?, 'tg', 'rule', 0, ?, ?)",
    )
    .bind(title)
    .bind(url)
    .bind(img)
    .bind(created_v)
    .bind(created_v)
    .execute(pool)
    .await
    .unwrap();
    let id: i64 =
        sqlx::query_scalar("SELECT last_insert_rowid()")
            .fetch_one(pool)
            .await
            .unwrap();
    id
}

/// 准备 clients/collectors/collector_histories 的最小依赖行，确保 FK 通过。
async fn setup_foreign_key_parents(pool: &sqlx::SqlitePool) {
    sqlx::query("INSERT INTO clients (id, user_id, client_type, status) VALUES ('test-client', 1, 'Client', 'active')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 100, '频道A', 'origin', 1)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collector_histories (collector_id, channel_id, message_id, is_auto_push) VALUES (1, 100, 1, 0)")
        .execute(pool)
        .await
        .unwrap();
}

/// 创建推送配置，返回 id。link_check_before_push=0（关闭外链检测，使用 classify_without_link_check）。
async fn create_push_config(
    pool: &sqlx::SqlitePool,
    name: &str,
    api_url: &str,
    target: &str,
    data_source_type: &str,
) -> i64 {
    sqlx::query(
        "INSERT INTO push_configs (name, api_url, target, data_source_type, link_check_before_push, is_active, auto_push) \
         VALUES (?, ?, ?, ?, 0, 1, 0)",
    )
    .bind(name)
    .bind(api_url)
    .bind(target)
    .bind(data_source_type)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 通过 HTTP API 触发按配置推送，返回响应 JSON（包在 data 字段里）。
async fn trigger_push(
    app: &mut axum::Router,
    token: &str,
    config_id: i64,
    batch_size: Option<i64>,
) -> serde_json::Value {
    let body = match batch_size {
        Some(bs) => format!(r#"{{"batch_size":{bs}}}"#),
        None => "{}".to_string(),
    };
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/push/configs/{config_id}/trigger"),
            token,
            Some(body),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    body
}

// ─── T004: 多配置场景资源能被独立推送 ─────────────────────────────────────────

#[tokio::test]
async fn test_push_for_config_multi_config_independent() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    setup_foreign_key_parents(&pool).await;

    // 准备 mock 推送目标
    let (mock_url, _mock_handle) = start_mock_push_server_ok().await;

    // 插入资源 R1（is_pushed=FALSE，未推送过）
    let r1 = insert_resource(&pool, "资源 R1", Some("https://example.com/r1"), None, None).await;

    // 创建配置 A、配置 B（都 data_source_type='all'，指向同一 mock URL）
    let config_a = create_push_config(&pool, "配置A", &mock_url, "api_a", "all").await;
    let config_b = create_push_config(&pool, "配置B", &mock_url, "api_b", "all").await;

    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 配置 A 推送 R1 成功（模拟 A 已推送 → is_pushed=TRUE + push_status(R1,A,pushed)）
    let resp_a = trigger_push(&mut app, &token, config_a, None).await;
    assert_eq!(
        resp_a["success"], true,
        "config A push failed: {resp_a}"
    );
    assert_eq!(resp_a["data"]["status"], "success");
    assert_eq!(resp_a["data"]["processed_count"], 1);

    // 验证 A 推送后 is_pushed=TRUE（全局字段被置位）+ push_status(R1, A, pushed)
    let is_pushed: i64 =
        sqlx::query_scalar("SELECT is_pushed FROM extracted_resources WHERE id = ?")
            .bind(r1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(is_pushed, 1, "配置 A 推送后 is_pushed 应为 TRUE");

    let status_a: String =
        sqlx::query_scalar("SELECT status FROM resource_push_status WHERE resource_id = ? AND push_config_id = ?")
            .bind(r1)
            .bind(config_a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_a, "pushed");

    // 关键断言：配置 B 仍能推送 R1（修复前 B 的候选 SQL 因 is_pushed=TRUE 被排除）
    let resp_b = trigger_push(&mut app, &token, config_b, None).await;
    assert_eq!(
        resp_b["success"], true,
        "config B push should succeed, got: {resp_b}"
    );
    assert_eq!(
        resp_b["data"]["status"], "success",
        "config B should have valid resources to push, got: {resp_b}"
    );
    assert_eq!(
        resp_b["data"]["processed_count"], 1,
        "config B should push R1 (multi-config independence)"
    );

    // 验证 push_status(R1, B, pushed) 已写入
    let status_b: String =
        sqlx::query_scalar("SELECT status FROM resource_push_status WHERE resource_id = ? AND push_config_id = ?")
            .bind(r1)
            .bind(config_b)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_b, "pushed", "配置 B 应能独立推送 R1");
}

// ─── T005: failed → pushed 状态转换（ON CONFLICT DO UPDATE） ──────────────────

#[tokio::test]
async fn test_push_status_failed_to_pushed_transition() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    setup_foreign_key_parents(&pool).await;

    let (mock_url, _mock_handle) = start_mock_push_server_ok().await;

    // 插入资源 R2（is_pushed=FALSE）
    let r2 = insert_resource(&pool, "资源 R2", Some("https://example.com/r2"), None, None).await;

    // 创建配置 C
    let config_c = create_push_config(&pool, "配置C", &mock_url, "api_c", "all").await;

    // 手动写入 failed 状态（模拟上一次推送失败）
    sqlx::query("INSERT INTO resource_push_status (resource_id, push_config_id, status) VALUES (?, ?, 'failed')")
        .bind(r2)
        .bind(config_c)
        .execute(&pool)
        .await
        .unwrap();

    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 触发配置 C 推送 → 应成功
    let resp = trigger_push(&mut app, &token, config_c, None).await;
    assert_eq!(resp["success"], true, "push should succeed: {resp}");
    assert_eq!(resp["data"]["processed_count"], 1);

    // 关键断言：(R2, C) 行的 status 已从 failed 升级为 pushed（修复前因 ON CONFLICT DO NOTHING 卡在 failed）
    let status: String =
        sqlx::query_scalar("SELECT status FROM resource_push_status WHERE resource_id = ? AND push_config_id = ?")
            .bind(r2)
            .bind(config_c)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        status, "pushed",
        "failed → pushed 状态转换必须生效（ON CONFLICT DO UPDATE）"
    );
}

// ─── T006: 候选 SQL ORDER BY created_at + remaining_count 字段 ─────────────────

#[tokio::test]
async fn test_candidate_sql_ordered_by_created_at() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    setup_foreign_key_parents(&pool).await;

    let (mock_url, _mock_handle) = start_mock_push_server_ok().await;

    // 插入 5 条资源，created_at 故意逆序（id 升序，但 created_at 降序）
    // 修复前候选 SQL 无 ORDER BY，默认按 rowid（=id）ASC，会返回最早插入的 3 条（R1/R2/R3 = 最新 created_at 的 3 条）
    // 修复后 ORDER BY created_at ASC, id ASC，应返回最早 created_at 的 3 条（R5/R4/R3）
    let _r1 = insert_resource(&pool, "R1", Some("https://example.com/r1"), None, Some("2026-06-10 12:00:00")).await;
    let _r2 = insert_resource(&pool, "R2", Some("https://example.com/r2"), None, Some("2026-06-09 12:00:00")).await;
    let _r3 = insert_resource(&pool, "R3", Some("https://example.com/r3"), None, Some("2026-06-08 12:00:00")).await;
    let _r4 = insert_resource(&pool, "R4", Some("https://example.com/r4"), None, Some("2026-06-07 12:00:00")).await;
    let _r5 = insert_resource(&pool, "R5", Some("https://example.com/r5"), None, Some("2026-06-06 12:00:00")).await;

    let config = create_push_config(&pool, "配置Order", &mock_url, "api_order", "all").await;

    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // batch_size=3 触发推送
    let resp = trigger_push(&mut app, &token, config, Some(3)).await;
    assert_eq!(resp["success"], true, "push failed: {resp}");
    assert_eq!(resp["data"]["processed_count"], 3);

    // 关键断言 1：推送的 3 条应是最早 created_at 的（R5, R4, R3），即 push_status 写入 (R3/R4/R5, config, pushed)
    let pushed_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT resource_id FROM resource_push_status WHERE push_config_id = ? AND status = 'pushed' ORDER BY resource_id ASC",
    )
    .bind(config)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        pushed_ids,
        vec![3, 4, 5],
        "候选 SQL 应 ORDER BY created_at ASC，推送最早 3 条 R3/R4/R5（修复前会推 R1/R2/R3）"
    );

    // 关键断言 2：返回 JSON 含 remaining_count = 2（5 - 3 = 2）
    assert_eq!(
        resp["data"]["remaining_count"], 2,
        "remaining_count 字段应存在且等于 total - processed = 2"
    );
}

// ─── T007: 死信清理后 img 置空的资源仍可被推送 ────────────────────────────────

#[tokio::test]
async fn test_dead_letter_cleared_img_pushable() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    setup_foreign_key_parents(&pool).await;

    let (mock_url, _mock_handle) = start_mock_push_server_ok().await;

    // 插入资源 R7：模拟 feature 040 死信清理结果（img='' 空串、url 有效、is_pushed=FALSE）
    let r7 = insert_resource(
        &pool,
        "死信清理后的资源",
        Some("https://example.com/dl-after-clear"),
        Some(""), // img 显式空串
        None,
    )
    .await;

    // 创建配置 D
    let config_d = create_push_config(&pool, "配置D", &mock_url, "api_d", "all").await;

    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 触发推送
    let resp = trigger_push(&mut app, &token, config_d, None).await;
    assert_eq!(resp["success"], true, "push failed: {resp}");

    // 关键断言：R7 被推送到 mock API（processed_count=1，不被空 img 误判为空资源）
    assert_eq!(
        resp["data"]["processed_count"], 1,
        "死信清理后 img 空但 url 有效的资源应被推送（修复前可能被空资源规则吞掉）"
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM resource_push_status WHERE resource_id = ? AND push_config_id = ?")
            .bind(r7)
            .bind(config_d)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pushed");
}

// ─── T014: 跳过明细 5 类（FR-003） — message 文本 + push_skip_records 明细 ───

#[tokio::test]
async fn test_skip_details_5_categories_in_message() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    setup_foreign_key_parents(&pool).await;

    let (mock_url, _mock_handle) = start_mock_push_server_ok().await;

    // 准备混合资源：
    // - 2 条图片未转存（img 非空 + img_forward_status != forwarded）
    // - 1 条链接失效（pre-populate link_check_results with 'invalid'）
    // - 1 条空资源（img + url 都空）
    // - 1 条有效资源（img 空 + url 缓存 valid）
    let url_invalid = "https://pan.quark.cn/s/expired";
    let url_valid = "https://pan.quark.cn/s/ok";

    // 通过公共函数计算 url_hash，预填缓存
    let norm_invalid = tgTool::services::link_check::normalize_url(url_invalid);
    let hash_invalid = tgTool::services::link_check::url_hash(&norm_invalid);
    let norm_valid = tgTool::services::link_check::normalize_url(url_valid);
    let hash_valid = tgTool::services::link_check::url_hash(&norm_valid);

    let future_ts = "2999-12-31 23:59:59"; // 永不过期
    sqlx::query("INSERT INTO link_check_results (url_hash, normalized_url, status, expires_at) VALUES (?, ?, 'invalid', ?)")
        .bind(&hash_invalid)
        .bind(&norm_invalid)
        .bind(future_ts)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO link_check_results (url_hash, normalized_url, status, expires_at) VALUES (?, ?, 'valid', ?)")
        .bind(&hash_valid)
        .bind(&norm_valid)
        .bind(future_ts)
        .execute(&pool)
        .await
        .unwrap();

    // 插入 5 条资源
    // 注意：make_resource 默认有 created_at，但这里直接 SQL 插入避免依赖辅助函数
    let _r_img1 = insert_resource(&pool, "img-not-fwd-1", Some(url_valid), Some("img_a"), None).await;
    // 给 r_img1 添加 img_forward_status（默认 insert_resource 不带 forward_tasks 关联，status 为 None → ImageNotForwarded）
    let _r_img2 = insert_resource(&pool, "img-not-fwd-2", Some(url_valid), Some("img_b"), None).await;
    let _r_link = insert_resource(&pool, "link-invalid", Some(url_invalid), None, None).await;
    let _r_empty = insert_resource(&pool, "empty", None, Some(""), None).await;
    let _r_valid = insert_resource(&pool, "valid", Some(url_valid), None, None).await;

    // 创建配置：开启 link_check_before_push（用 cache 中的 invalid 状态触发 LinkInvalid）
    sqlx::query(
        "INSERT INTO push_configs (name, api_url, target, data_source_type, link_check_before_push, is_active, auto_push) \
         VALUES ('配置Skip', ?, 'api_skip', 'all', 1, 1, 0)",
    )
    .bind(&mock_url)
    .execute(&pool)
    .await
    .unwrap();
    let config_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = trigger_push(&mut app, &token, config_id, None).await;
    assert_eq!(resp["success"], true, "push failed: {resp}");
    // 只推送 1 条（valid），其余 4 条跳过
    assert_eq!(resp["data"]["processed_count"], 1);
    // 返回 JSON 的 skipped 对象各键计数正确
    assert_eq!(resp["data"]["skipped"]["image_not_forwarded"], 2);
    assert_eq!(resp["data"]["skipped"]["link_invalid"], 1);
    assert_eq!(resp["data"]["skipped"]["empty_resource"], 1);
    assert_eq!(resp["data"]["skipped"]["other"], 0);
    assert_eq!(resp["data"]["skipped"]["total"], 4);

    // 验证 push_histories.message 含分类汇总（5 类格式）
    let message: String =
        sqlx::query_scalar("SELECT message FROM push_histories WHERE push_config_id = ? ORDER BY id DESC LIMIT 1")
            .bind(config_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        message.contains("图片未转存") && message.contains('2'),
        "message 应含 '图片未转存 2'，实际：{message}"
    );
    assert!(
        message.contains("链接失效") && message.contains('1'),
        "message 应含 '链接失效 1'，实际：{message}"
    );
    assert!(
        message.contains("空资源") && message.contains('1'),
        "message 应含 '空资源 1'，实际：{message}"
    );

    // 验证 push_skip_records 表写入 4 条明细
    let skip_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM push_skip_records psr \
            JOIN push_histories ph ON psr.push_history_id = ph.id \
            WHERE ph.push_config_id = ?")
            .bind(config_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(skip_count, 4, "应有 4 条跳过明细");

    // 验证 skip_reason 值匹配（按 reason 分组计数）
    let reasons: Vec<(String, i64)> = sqlx::query_as(
        "SELECT psr.skip_reason, COUNT(*) FROM push_skip_records psr \
         JOIN push_histories ph ON psr.push_history_id = ph.id \
         WHERE ph.push_config_id = ? GROUP BY psr.skip_reason",
    )
    .bind(config_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut reason_map: std::collections::HashMap<String, i64> = reasons.into_iter().collect();
    assert_eq!(reason_map.remove("image_not_forwarded").unwrap_or(0), 2);
    assert_eq!(reason_map.remove("link_invalid").unwrap_or(0), 1);
    assert_eq!(reason_map.remove("empty_resource").unwrap_or(0), 1);
}

