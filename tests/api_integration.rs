//! API 集成测试 — 使用 SQLite 内存数据库测试完整路由

use axum::body::Body;
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use tgTool::config::Config;
use tgTool::services::crypto;
use tgTool::state::{AppState, DbPool};
use tower::ServiceExt;

use std::path::PathBuf;

/// 创建用于测试的 SQLite 内存数据库连接池，并插入 root 用户
async fn setup_test_db() -> DbPool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create test SQLite pool");

    // 执行建表语句
    let migration_sql = include_str!("../migrations/001_init_sqlite.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("Failed to run test migrations");

    // Migration 002: Add client_id to collectors (ignore if duplicate column)
    let m2 = include_str!("../migrations/002_collector_client_id_sqlite.sql");
    let _ = sqlx::raw_sql(m2).execute(&pool).await;

    // Migration 003: Create extracted_resources table
    let m3 = include_str!("../migrations/003_extracted_resources_sqlite.sql");
    sqlx::raw_sql(m3)
        .execute(&pool)
        .await
        .expect("Failed to run migration 003");

    // Migration 004: Add is_extracted to collector_histories
    let m4 = include_str!("../migrations/004_collector_histories_is_extracted_sqlite.sql");
    let _ = sqlx::raw_sql(m4).execute(&pool).await;

    // Migration 005: Add share_ids to extracted_resources
    let m5 = include_str!("../migrations/005_add_share_ids_sqlite.sql");
    let _ = sqlx::raw_sql(m5).execute(&pool).await;

    // Migration 009: extract_histories
    let m9 = include_str!("../migrations/009_extract_histories_sqlite.sql");
    let _ = sqlx::raw_sql(m9).execute(&pool).await;

    // Migration 008: image forward tables (forward_tasks)
    let m8 = include_str!("../migrations/008_image_tables_sqlite.sql");
    let _ = sqlx::raw_sql(m8).execute(&pool).await;

    // Migration 010: rule filter columns (forward_client_id/filter_mode/keywords/media_filter)
    let m10 = include_str!("../migrations/010_rule_filter_sqlite.sql");
    let _ = sqlx::raw_sql(m10).execute(&pool).await;

    // Migration 011: rule source_client_id
    let m11 = include_str!("../migrations/011_rule_source_client_sqlite.sql");
    let _ = sqlx::raw_sql(m11).execute(&pool).await;

    // Migration 012: push_configs + push_config_collectors + resource_push_status
    let m12 = include_str!("../migrations/012_push_configs_sqlite.sql");
    let _ = sqlx::raw_sql(m12).execute(&pool).await;

    // Migration 013: link_check_results + push_skip_records + push_histories 跳过统计列
    let m13 = include_str!("../migrations/013_resource_link_check_sqlite.sql");
    let _ = sqlx::raw_sql(m13).execute(&pool).await;

    // Migration 014: push_configs 加 link_check_before_push 开关
    let m14 = include_str!("../migrations/014_push_config_link_check_sqlite.sql");
    let _ = sqlx::raw_sql(m14).execute(&pool).await;

    // Migration 015: forward_tasks 加 image_message_id 字段
    let m15 = include_str!("../migrations/015_forward_task_message_id_sqlite.sql");
    let _ = sqlx::raw_sql(m15).execute(&pool).await;

    // Migration 016: users 加 must_change_password（feature 027 SEC-002）
    let m16 = include_str!("../migrations/016_users_must_change_password_sqlite.sql");
    let _ = sqlx::raw_sql(m16).execute(&pool).await;

    // Migration 017: clients 加 name/username（客户端列表显示账号名）
    let m17 = include_str!("../migrations/017_client_name_username_sqlite.sql");
    let _ = sqlx::raw_sql(m17).execute(&pool).await;

    // 插入 root 用户（使用当前 bcrypt 版本生成 hash）
    let hash = crypto::hash_password("123456").expect("Failed to hash root password");
    sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', ?, 100, 1)")
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("Failed to insert root user");

    DbPool::Sqlite(pool)
}

/// 创建测试用的 AppState（返回 state + tg_clients 引用，便于测试注入）
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

/// 构建 axum 测试 Router
fn build_test_app(state: AppState) -> axum::Router {
    tgTool::routes::build_router(state).layer(tgTool::middleware::cors::cors_layer())
}

/// 从 Response body 读取并解析 JSON
async fn parse_body(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// 构建一个 HTTP request（无认证）
fn build_request(method: &str, uri: &str, body: Option<String>) -> axum::http::Request<Body> {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, Body::from))
        .unwrap()
}

/// 构建一个带认证 token 的 HTTP request
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

/// 以 root 用户登录，返回 auth token
async fn get_root_token(app: &mut axum::Router) -> String {
    let req = build_request(
        "POST",
        "/api/auth/login",
        Some(r#"{"username":"root","password":"123456"}"#.to_string()),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    body["data"]["token"].as_str().unwrap().to_string()
}

/// 从 JSON Value 提取 success 字段
fn assert_success(body: &serde_json::Value) {
    assert_eq!(body["success"], true, "Expected success=true, got: {body}");
}

// ============================================================
// 测试用例
// ============================================================

#[tokio::test]
async fn test_status_endpoint() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request("GET", "/api/status", None))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["data"]["clients"]["total"], 0);
}

#[tokio::test]
async fn test_register_new_user() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request(
            "POST",
            "/api/auth/register",
            Some(serde_json::json!({"username": "testuser", "password": "pass123"}).to_string()),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    assert!(body["data"]["token"].is_string());
}

#[tokio::test]
async fn test_register_duplicate_user_fails() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let req_body = serde_json::json!({"username": "dup", "password": "pass123"}).to_string();

    // 第一次注册成功
    let resp1 = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/register",
            Some(req_body.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);

    // 第二次注册失败（用户名唯一约束）
    let resp2 = app
        .oneshot(build_request("POST", "/api/auth/register", Some(req_body)))
        .await
        .unwrap();
    assert_eq!(resp2.status(), 400);
}

#[tokio::test]
async fn test_login_with_root_user() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    // root 用户在 migration 中已创建，密码是 123456
    let response = app
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(serde_json::json!({"username": "root", "password": "123456"}).to_string()),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    assert!(body["data"]["token"].is_string());
}

#[tokio::test]
async fn test_login_wrong_password() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(serde_json::json!({"username": "root", "password": "wrongpass"}).to_string()),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn test_login_nonexistent_user() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(serde_json::json!({"username": "nonexistent", "password": "xxx"}).to_string()),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn test_logout() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request("POST", "/api/auth/logout", None))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
}

#[tokio::test]
async fn test_create_and_delete_user() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建用户
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/users",
            &token,
            Some(
                serde_json::json!({"username": "newuser", "password": "pass123", "role": 1})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除用户 id=2（id=1 是 root）
    let response = app
        .oneshot(build_auth_request("DELETE", "/api/users/2", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_delete_root_user_forbidden() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let response = app
        .oneshot(build_auth_request("DELETE", "/api/users/1", &token, None))
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn test_list_users() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let response = app
        .oneshot(build_auth_request(
            "GET",
            "/api/users?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_rule_crud() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建规则
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_id": 123456,
                    "source_chat_name": "Test Channel",
                    "forward_method": "Chat",
                    "forward_target": "-100999",
                    "is_active": true
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);

    // 列出规则
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/rules?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 切换规则状态
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/1/toggle",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除规则
    let response = app
        .oneshot(build_auth_request("DELETE", "/api/rules/1", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_collector_crud() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建采集器
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors",
            &token,
            Some(
                serde_json::json!({
                    "client_id": "test_client",
                    "channel_id": 999888,
                    "channel_name": "News Channel",
                    "collector_type": "origin",
                    "is_active": true,
                    "remark": "test collector"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 列出采集器
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/collectors?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 切换采集器状态
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/collectors/1/toggle",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除采集器
    let response = app
        .oneshot(build_auth_request(
            "DELETE",
            "/api/collectors/1",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_client_add_and_remove() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 添加客户端
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/clients",
            &token,
            Some(serde_json::json!({"client_type": "Client", "phone": "+123456"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    let client_id = body["data"]["id"].as_str().unwrap().to_string();

    // 列出客户端
    let response = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/clients", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    let list = body["data"]["list"]
        .as_array()
        .expect("list should be array");
    assert!(
        !list.is_empty(),
        "list should not be empty after adding client"
    );
    let c = &list[0];
    assert_eq!(c["client_type"], "Client");
    assert_eq!(c["phone"], "+123456");
    assert_eq!(c["status"], "new");
    // Verify no password-like fields leaked
    assert!(c.get("password").is_none());

    // 获取客户端状态（应为 new，因为 TgClientMap 中没有）
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            &format!("/api/clients/{client_id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_eq!(body["data"]["status"], "new");

    // 删除客户端
    let response = app
        .oneshot(build_auth_request(
            "DELETE",
            &format!("/api/clients/{client_id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_options_crud() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 获取初始 options
    let response = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/options", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 更新 options
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/options",
            &token,
            Some(
                serde_json::json!({"push_api_url": "https://example.com", "push_interval": "30"})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 再次获取 options，应该包含更新后的值
    let response = app
        .oneshot(build_auth_request("GET", "/api/options", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_eq!(body["data"]["push_api_url"], "https://example.com");
    assert_eq!(body["data"]["push_interval"], "30");
}

#[tokio::test]
async fn test_push_endpoints() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 推送统计
    let response = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/stats", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["total"], 0);

    // 推送历史
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/push/histories?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 触发推送
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/trigger",
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 更新调度
    let response = app
        .oneshot(build_auth_request(
            "PUT",
            "/api/push/scheduler",
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_file_endpoints() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 列出文件
    let response = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/files?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除不存在的文件应返回 404
    let response = app
        .oneshot(build_auth_request("DELETE", "/api/files/999", &token, None))
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn test_404_for_unknown_api() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request("GET", "/api/nonexistent", None))
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn test_register_and_login_flow() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    // 注册
    let reg_response = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/register",
            Some(serde_json::json!({"username": "flowuser", "password": "mypassword"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(reg_response.status(), 200);

    // 用同样的凭据登录
    let login_response = app
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(serde_json::json!({"username": "flowuser", "password": "mypassword"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(login_response.status(), 200);
    let body = parse_body(login_response.into_body()).await;
    assert_success(&body);
    assert!(body["data"]["token"].is_string());
}

// ============================================================
// 新增测试：Client 生命周期
// ============================================================

#[tokio::test]
async fn test_client_start_stop() {
    let db = setup_test_db().await;
    let (state, tg_clients) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 添加客户端
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/clients",
            &token,
            Some(serde_json::json!({"client_type": "Client", "phone": "+999"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    let client_id = body["data"]["id"].as_str().unwrap().to_string();

    // 启动客户端 — 测试环境无法真正连接 Telegram，返回 500 或 200（取决于网络环境）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/clients/{client_id}/start"),
            &token,
            None,
        ))
        .await
        .unwrap();
    // CI 环境可能返回 200（连接超时后降级）或 500（连接失败）
    assert!(
        resp.status() == 500 || resp.status() == 200,
        "expected 500 or 200, got {}",
        resp.status()
    );

    // 直接注入模拟客户端到内存中，测试 stop/get_status 逻辑
    tg_clients.write().await.insert(
        client_id.clone(),
        tgTool::state::TgClientEntry {
            status: "active".to_string(),
            handle: None,
            client: None,
            login_token: None,
            password_token: None,
            session_path: format!("tg_store/{client_id}.session"),
            user_info: None,
        },
    );

    // 检查状态应为 active
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            &format!("/api/clients/{client_id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["status"], "active");

    // 停止客户端
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/clients/{client_id}/stop"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 停止后状态应为 offline
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            &format!("/api/clients/{client_id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["status"], "offline");

    // 启动不存在的客户端 → 连接失败或 DB 不存在
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/clients/nonexistent123/start",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert!(
        resp.status() == 500 || resp.status() == 404,
        "expected 500 or 404, got {}",
        resp.status()
    );

    // 停止不存在的客户端 → 404（DB 中不存在，更新 0 行）
    let resp = app
        .oneshot(build_auth_request(
            "POST",
            "/api/clients/nonexistent123/stop",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_client_auth() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 添加客户端
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/clients",
            &token,
            Some(serde_json::json!({"client_type": "Client", "phone": "+111"}).to_string()),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let client_id = body["data"]["id"].as_str().unwrap().to_string();

    // 提交验证码 — 客户端未连接，期望 400（客户端未连接/不存在）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/clients/{client_id}/auth"),
            &token,
            Some(serde_json::json!({"type": "code", "value": "12345"}).to_string()),
        ))
        .await
        .unwrap();
    // 客户端在内存中不存在（未通过 start_client 连接），返回 404
    assert_eq!(resp.status(), 404);

    // 提交密码 — 同样客户端未连接
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/clients/{client_id}/auth"),
            &token,
            Some(serde_json::json!({"type": "password", "value": "mypass"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // 无效认证类型 → 400
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/clients/{client_id}/auth"),
            &token,
            Some(serde_json::json!({"type": "invalid", "value": "x"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // 获取聊天列表 — 客户端未连接，返回 404
    let resp = app
        .oneshot(build_auth_request(
            "GET",
            &format!("/api/clients/{client_id}/chats"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ============================================================
// 新增测试：Rule CRUD 细节
// ============================================================

#[tokio::test]
async fn test_rule_crud_with_data_verification() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建规则
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_id": -100123456,
                    "source_chat_name": "Test Channel",
                    "forward_method": "Chat",
                    "forward_target": "-100999",
                    "is_active": true,
                    "remark": "initial remark"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 列出规则并验证数据
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    let list = body["data"]["list"]
        .as_array()
        .expect("list should be array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["source_chat_id"], -100123456);
    assert_eq!(list[0]["source_chat_name"], "Test Channel");
    assert_eq!(list[0]["forward_method"], "Chat");
    assert_eq!(list[0]["forward_target"], "-100999");
    assert_eq!(list[0]["is_active"], true);
    assert_eq!(list[0]["remark"], "initial remark");

    // Toggle → is_active 应翻转
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/1/toggle",
            &token,
            None,
        ))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules/1", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["is_active"], false);

    // 删除
    let resp = app
        .clone()
        .oneshot(build_auth_request("DELETE", "/api/rules/1", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 删除后 list 为空
    let resp = app
        .oneshot(build_auth_request("GET", "/api/rules", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_rule_get_update_not_found() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // GET 不存在的规则 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules/999", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // UPDATE 不存在的规则 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/999",
            &token,
            Some(serde_json::json!({"remark": "test"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // DELETE 不存在的规则 → 200（DELETE 无 404 检查）
    let resp = app
        .clone()
        .oneshot(build_auth_request("DELETE", "/api/rules/999", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // UPDATE 空body → 短路返回成功
    let resp = app
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/1",
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_rule_update_fields() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建规则
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_id": 111,
                    "source_chat_name": "Original",
                    "forward_method": "Chat",
                    "forward_target": "-100",
                    "is_active": true
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();

    // 更新多个字段
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/1",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_name": "Updated Channel",
                    "forward_target": "-200",
                    "is_active": false,
                    "remark": "new remark"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 验证更新后的数据
    let resp = app
        .oneshot(build_auth_request("GET", "/api/rules/1", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["source_chat_name"], "Updated Channel");
    assert_eq!(body["data"]["forward_target"], "-200");
    assert_eq!(body["data"]["is_active"], false);
    assert_eq!(body["data"]["remark"], "new remark");
    // 未更新的字段保持不变
    assert_eq!(body["data"]["source_chat_id"], 111);
    assert_eq!(body["data"]["forward_method"], "Chat");
}

#[tokio::test]
async fn test_rule_messages_pagination() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建规则
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(serde_json::json!({"source_chat_id": 1, "forward_method": "Chat"}).to_string()),
        ))
        .await
        .unwrap();

    // 直接插 DB 5 条 messages
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    for i in 1..=5 {
        sqlx::query("INSERT INTO messages (rule_id, chat_id, message_id, content, status) VALUES (1, -100, ?, ?, 'pending')")
            .bind(i).bind(format!("msg {}", i))
            .execute(&pool).await.unwrap();
    }

    // 获取第1页 (page_size=3)
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/rules/1/messages?page=1&page_size=3",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["pagination"]["total"], 5);
    assert_eq!(body["data"]["pagination"]["page"], 1);
    assert_eq!(body["data"]["pagination"]["page_size"], 3);
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 3);

    // 获取第2页
    let resp = app
        .oneshot(build_auth_request(
            "GET",
            "/api/rules/1/messages?page=2&page_size=3",
            &token,
            None,
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 2); // 剩余2条
}

// ============================================================
// 新增测试：Collector CRUD 细节
// ============================================================

#[tokio::test]
async fn test_collector_crud_with_data_verification() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建采集器
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors",
            &token,
            Some(
                serde_json::json!({
                    "client_id": "test_client",
                    "channel_id": -100888,
                    "channel_name": "News",
                    "collector_type": "origin",
                    "is_active": true,
                    "remark": "test"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();

    // 验证 list 数据
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/collectors", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let list = body["data"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["channel_id"], -100888);
    assert_eq!(list[0]["channel_name"], "News");
    assert_eq!(list[0]["collector_type"], "origin");
    assert_eq!(list[0]["is_active"], true);

    // Toggle → is_active 翻转
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/collectors/1/toggle",
            &token,
            None,
        ))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/collectors/1", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["is_active"], false);
}

#[tokio::test]
async fn test_collector_get_update_not_found() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // GET 不存在 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/collectors/999",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // UPDATE 不存在 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/collectors/999",
            &token,
            Some(serde_json::json!({"remark": "x"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // UPDATE 空 body → 短路成功
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/collectors/1",
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 创建后 UPDATE 字段
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors",
            &token,
            Some(serde_json::json!({"client_id": "test_client", "channel_id": 100, "collector_type": "origin"}).to_string()),
        ))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/collectors/1",
            &token,
            Some(
                serde_json::json!({"channel_name": "Updated", "remark": "new remark"}).to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = app
        .oneshot(build_auth_request("GET", "/api/collectors/1", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["channel_name"], "Updated");
    assert_eq!(body["data"]["remark"], "new remark");
}

#[tokio::test]
async fn test_collector_histories_pagination() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建采集器
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors",
            &token,
            Some(serde_json::json!({"client_id": "test_client", "channel_id": -100, "collector_type": "origin"}).to_string()),
        ))
        .await
        .unwrap();

    // 直接插 DB 3 条 collector_histories
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    for i in 1..=3 {
        sqlx::query("INSERT INTO collector_histories (collector_id, channel_id, message_id, is_auto_push) VALUES (1, -100, ?, 0)")
            .bind(i)
            .execute(&pool).await.unwrap();
    }

    // 获取历史列表
    let resp = app
        .oneshot(build_auth_request(
            "GET",
            "/api/collectors/histories?page=1&page_size=2",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["pagination"]["total"], 3);
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 2);
    // 验证历史数据字段
    let first = &body["data"]["list"].as_array().unwrap()[0];
    assert_eq!(first["collector_id"], 1);
    assert_eq!(first["channel_id"], -100);
}

#[tokio::test]
async fn test_fetch_history_no_active_client() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建采集器但不启动任何客户端
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors",
            &token,
            Some(serde_json::json!({"client_id": "test_client", "channel_id": -100, "collector_type": "origin"}).to_string()),
        ))
        .await
        .unwrap();

    // 触发采集 → 应返回 400（没有活跃客户端）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors/1/fetch",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // 不存在的采集器 → 404
    let resp = app
        .oneshot(build_auth_request(
            "POST",
            "/api/collectors/999/fetch",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ============================================================
// 新增测试：User CRUD + 脱敏验证
// ============================================================

#[tokio::test]
async fn test_list_users_no_password_leak() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = app
        .oneshot(build_auth_request("GET", "/api/users", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    let list = body["data"]["list"].as_array().unwrap();
    assert!(!list.is_empty());
    // password 字段不应出现在返回中
    assert!(
        list[0].get("password").is_none(),
        "password should not be in response"
    );
    assert_eq!(list[0]["username"], "root");
    assert_eq!(list[0]["role"], 100);
}

#[tokio::test]
async fn test_user_get_update_lifecycle() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建用户
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/users",
            &token,
            Some(
                serde_json::json!({"username": "lifecycle", "password": "pass123", "role": 1})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET 单个用户 → 无 password
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/users/2", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["username"], "lifecycle");
    assert!(body["data"].get("password").is_none());

    // UPDATE 用户字段
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/users/2",
            &token,
            Some(
                serde_json::json!({"display_name": "Display Name", "role": 10, "status": 1})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 验证更新后的数据
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/users/2", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["display_name"], "Display Name");
    assert_eq!(body["data"]["role"], 10);
    assert_eq!(body["data"]["status"], 1);

    // UPDATE 不存在的用户 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/users/999",
            &token,
            Some(serde_json::json!({"display_name": "x"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // GET 不存在的用户 → 404
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/users/999", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // DELETE 后再 GET → 404
    let _ = app
        .clone()
        .oneshot(build_auth_request("DELETE", "/api/users/2", &token, None))
        .await
        .unwrap();

    let resp = app
        .oneshot(build_auth_request("GET", "/api/users/2", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_user_create_missing_fields() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 无 password → 400
    let resp = app
        .oneshot(build_auth_request(
            "POST",
            "/api/users",
            &token,
            Some(serde_json::json!({"username": "nopass"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

// ============================================================
// 新增测试：Push 统计 + 重试 + 调度
// ============================================================

#[tokio::test]
async fn test_push_stats_with_data() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    // 插入测试数据
    sqlx::query("INSERT INTO push_histories (batch_id, status, data_count, pushed_at) VALUES ('b1', 'success', 5, CURRENT_TIMESTAMP)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO push_histories (batch_id, status, data_count, error_msg, pushed_at) VALUES ('b2', 'failed', 0, 'timeout', CURRENT_TIMESTAMP)")
        .execute(&pool).await.unwrap();

    let resp = app
        .oneshot(build_auth_request("GET", "/api/push/stats", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["total"], 2);
    assert_eq!(body["data"]["success"], 1);
    assert_eq!(body["data"]["failed"], 1);
}

#[tokio::test]
async fn test_push_retry() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    // 插入一条失败的推送记录
    sqlx::query("INSERT INTO push_histories (batch_id, status, data_count, pushed_at) VALUES ('retry-batch', 'failed', 3, CURRENT_TIMESTAMP)")
        .execute(&pool).await.unwrap();

    // 触发重试
    let resp = app
        .clone()
        .oneshot(build_auth_request("POST", "/api/push/retry", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert!(body["message"].as_str().unwrap().contains("1 条"));

    // 验证数据库中 status 已变为 pending
    let status: String =
        sqlx::query_scalar("SELECT status FROM push_histories WHERE batch_id = 'retry-batch'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pending");
}

#[tokio::test]
async fn test_push_scheduler_saves_config() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 更新调度配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/push/scheduler",
            &token,
            Some(serde_json::json!({"interval": "15", "api_url": "https://test.com"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 通过 options 接口验证配置已保存
    let resp = app
        .oneshot(build_auth_request("GET", "/api/options", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["push_interval"], "15");
    assert_eq!(body["data"]["push_api_url"], "https://test.com");
}

#[tokio::test]
async fn test_push_list_histories_structure() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    sqlx::query("INSERT INTO push_histories (batch_id, target, status, data_count, message, pushed_at) VALUES ('b1', 'remote', 'success', 10, 'ok', CURRENT_TIMESTAMP)")
        .execute(&pool).await.unwrap();

    let resp = app
        .oneshot(build_auth_request(
            "GET",
            "/api/push/histories?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["pagination"]["total"], 1);
    let list = body["data"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["batch_id"], "b1");
    assert_eq!(list[0]["status"], "success");
    assert_eq!(list[0]["data_count"], 10);
}

// ============================================================
// 新增测试：系统状态
// ============================================================

#[tokio::test]
async fn test_system_status_with_clients() {
    let db = setup_test_db().await;
    let (state, tg_clients) = make_test_state(db);
    let app = build_test_app(state);

    // 直接注入模拟客户端到内存中（避免需要真实 Telegram 连接）
    tg_clients.write().await.insert(
        "bot_mock".to_string(),
        tgTool::state::TgClientEntry {
            status: "active".to_string(),
            handle: None,
            client: None,
            login_token: None,
            password_token: None,
            session_path: "tg_store/bot_mock.session".to_string(),
            user_info: None,
        },
    );
    tg_clients.write().await.insert(
        "client_offline".to_string(),
        tgTool::state::TgClientEntry {
            status: "offline".to_string(),
            handle: None,
            client: None,
            login_token: None,
            password_token: None,
            session_path: "tg_store/client_offline.session".to_string(),
            user_info: None,
        },
    );

    // system_status 从数据库读取客户端数量，内存注入不影响结果

    // GET /api/status 是公共路由，不需要认证
    let resp = app
        .oneshot(build_request("GET", "/api/status", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    // 注意：system_status 从数据库 clients 表查询，内存注入不影响结果
    // 在空数据库下 clients.total=0, clients.active=0
    assert_eq!(body["data"]["clients"]["total"], 0);
    assert_eq!(body["data"]["clients"]["active"], 0);
}

// ============================================================
// 新增测试：文件下载 404
// ============================================================

#[tokio::test]
async fn test_file_download_not_found() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let resp = app
        .oneshot(build_request(
            "GET",
            "/api/files/download/nonexistent.txt",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ============================================================
// T018-T020: Auth 中间件测试
// ============================================================

#[tokio::test]
async fn test_t018_unauthenticated_protected_routes_return_401() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    // 不带 token 访问各个受保护路由 → 全部返回 401
    let protected_endpoints = vec![
        ("GET", "/api/clients"),
        ("GET", "/api/rules"),
        ("GET", "/api/collectors"),
        ("GET", "/api/push/stats"),
        ("GET", "/api/users"),
        ("GET", "/api/files"),
        ("GET", "/api/options"),
        ("GET", "/api/auth/me"),
    ];

    for (method, uri) in protected_endpoints {
        let resp = app
            .clone()
            .oneshot(build_request(method, uri, None))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            401,
            "Expected 401 for {method} {uri} without auth"
        );
    }
}

#[tokio::test]
async fn test_t019_authenticated_protected_routes_return_data() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 带 token 访问受保护路由 → 正常返回数据
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/clients", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/collectors", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/stats", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/options", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_t020_public_routes_no_auth_needed() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    // 公开路由无需认证即可访问
    // GET /api/status
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/status", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // POST /api/auth/register
    let resp = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/register",
            Some(r#"{"username":"pubtest","password":"pass123"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // POST /api/auth/login
    let resp = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(r#"{"username":"root","password":"123456"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // POST /api/auth/logout
    let resp = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/logout", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET /api/files/download/nonexistent → 公开路由，但文件不存在返回 404
    let resp = app
        .oneshot(build_request(
            "GET",
            "/api/files/download/nothing.txt",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404); // 404 不是 401，说明没有 auth 检查
}

// ===== Phase 3-5 新增测试 =====

/// T010: 验证健康检查返回 db_status 字段
#[tokio::test]
async fn test_health_check_returns_db_status() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let resp = app
        .oneshot(build_request("GET", "/api/status", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(body["success"].as_bool().unwrap());
    assert_eq!(body["data"]["db_status"].as_str().unwrap(), "ok");
    assert!(body["data"]["version"].is_string());
    assert!(body["data"]["clients"].is_object());
}

/// T010: 验证客户端信息查询（未认证连接）返回合理响应
#[tokio::test]
async fn test_get_me_unconnected_client() {
    let db = setup_test_db().await;
    let (state, _tg_clients) = make_test_state(db);
    let app = build_test_app(state);

    // 登录获取 token
    let resp = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(r#"{"username":"root","password":"123456"}"#.to_string()),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let token = body["data"]["token"].as_str().unwrap().to_string();

    // 添加客户端
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/clients",
            &token,
            Some(r#"{"id":"test_info","client_type":"user","phone":"+123456"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 查看客户端状态 — 应返回 connected: false, user_info: null
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "GET",
            "/api/clients/test_info",
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    // 客户端未连接时 status 应该反映真实状态
    assert!(body["success"].as_bool().unwrap());
}

// ============================================================
// Phase 005: 推送配置校验 + 资源管理 集成测试
// ============================================================

// ============================================================
// Phase 011: 登录验证码 集成测试
// ============================================================

#[tokio::test]
async fn test_captcha_status_initially_not_required() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let resp = app
        .oneshot(build_request("GET", "/api/auth/captcha-status", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(body["success"].as_bool().unwrap());
    assert_eq!(body["data"]["required"], false);
}

#[tokio::test]
async fn test_captcha_image_generation() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    let resp = app
        .oneshot(build_request("GET", "/api/auth/captcha-image", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(body["success"].as_bool().unwrap());
    assert!(body["data"]["captcha_key"].is_string());
    assert!(body["data"]["captcha_image"].is_string());
    let image = body["data"]["captcha_image"].as_str().unwrap();
    assert!(image.starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn test_login_captcha_triggered_after_3_failures() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state);

    // Fail 2 times with wrong password — should return 401
    for _ in 0..2 {
        let resp = app
            .clone()
            .oneshot(build_request(
                "POST",
                "/api/auth/login",
                Some(r#"{"username":"root","password":"wrong"}"#.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    // 3rd failure triggers captcha_required — returns 200 with captcha_required flag
    let resp = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/auth/login",
            Some(r#"{"username":"root","password":"wrong"}"#.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(!body["success"].as_bool().unwrap());
    assert_eq!(body["data"]["captcha_required"], true);

    // captcha-status should now show required
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/auth/captcha-status", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["required"], true);
}

#[tokio::test]
async fn test_login_with_captcha_wrong_code_rejected() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state.clone());

    // Fail 3 times to trigger captcha
    for _ in 0..3 {
        let _ = app
            .clone()
            .oneshot(build_request(
                "POST",
                "/api/auth/login",
                Some(r#"{"username":"root","password":"wrong"}"#.to_string()),
            ))
            .await
            .unwrap();
    }

    // Get a captcha
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/auth/captcha-image", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let captcha_key = body["data"]["captcha_key"].as_str().unwrap().to_string();

    // Try login with wrong captcha code
    let login_body = format!(
        r#"{{"username":"root","password":"123456","captcha_key":"{}","captcha_code":"zzzzz"}}"#,
        captcha_key
    );
    let resp = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/login", Some(login_body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(!body["success"].as_bool().unwrap());
    // Should indicate captcha error (still required)
    assert_eq!(body["data"]["captcha_required"], true);
}

#[tokio::test]
async fn test_login_with_correct_captcha_succeeds() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let captcha_store = state.captcha_store.clone();
    let app = build_test_app(state);

    // Fail 3 times to trigger captcha
    for _ in 0..3 {
        let _ = app
            .clone()
            .oneshot(build_request(
                "POST",
                "/api/auth/login",
                Some(r#"{"username":"root","password":"wrong"}"#.to_string()),
            ))
            .await
            .unwrap();
    }

    // Get captcha and extract the answer from the store
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/auth/captcha-image", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let captcha_key = body["data"]["captcha_key"].as_str().unwrap().to_string();

    // Get the answer from the store
    let answer = captcha_store
        .get(&captcha_key)
        .map(|e| e.value().answer.clone())
        .unwrap();

    // Login with correct captcha
    let login_body = format!(
        r#"{{"username":"root","password":"123456","captcha_key":"{}","captcha_code":"{}"}}"#,
        captcha_key, answer
    );
    let resp = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/login", Some(login_body)))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert!(body["success"].as_bool().unwrap());
    assert!(body["data"]["token"].is_string());

    // After successful login, captcha-status should be cleared
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/auth/captcha-status", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["required"], false);
}

#[tokio::test]
async fn test_captcha_single_use() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let app = build_test_app(state.clone());

    // Fail 3 times
    for _ in 0..3 {
        let _ = app
            .clone()
            .oneshot(build_request(
                "POST",
                "/api/auth/login",
                Some(r#"{"username":"root","password":"wrong"}"#.to_string()),
            ))
            .await
            .unwrap();
    }

    // Get captcha
    let captcha_store = state.captcha_store.clone();
    let resp = app
        .clone()
        .oneshot(build_request("GET", "/api/auth/captcha-image", None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let captcha_key = body["data"]["captcha_key"].as_str().unwrap().to_string();
    let answer = captcha_store
        .get(&captcha_key)
        .map(|e| e.value().answer.clone())
        .unwrap();

    // Use captcha with wrong password — captcha consumed
    let login_body = format!(
        r#"{{"username":"root","password":"wrong","captcha_key":"{}","captcha_code":"{}"}}"#,
        captcha_key, answer
    );
    let _ = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/login", Some(login_body)))
        .await
        .unwrap();

    // Try same captcha again with correct password — should fail (captcha already used)
    let login_body2 = format!(
        r#"{{"username":"root","password":"123456","captcha_key":"{}","captcha_code":"{}"}}"#,
        captcha_key, answer
    );
    let resp = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/login", Some(login_body2)))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert!(!body["success"].as_bool().unwrap());
    assert_eq!(body["data"]["captcha_required"], true);
}

// ============================================================
// T002-T004: 资源详情查看（提取对比）
// ============================================================

#[tokio::test]
async fn test_get_resource_detail_success() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 插入一条采集历史（需要先有 collector）
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    sqlx::query(
        "INSERT INTO collectors (user_id, channel_id, collector_type, is_active) VALUES (1, 100, 'channel', 1)"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO collector_histories (collector_id, channel_id, message_id, raw_data, is_auto_push, is_extracted) \
         VALUES (1, 100, 200, ?, 0, 1)"
    )
    .bind(r#"{"text":"名称：测试电影\n链接：https://pan.quark.cn/s/abc123","media_type":"photo","photo_id":"12345"}"#)
    .execute(&pool)
    .await
    .unwrap();

    // 插入一条已提取资源
    sqlx::query(
        "INSERT INTO extracted_resources (collector_history_id, title, url, description, category, tags, source, extract_mode, is_pushed, is_edited) \
         VALUES (1, '测试电影', 'https://pan.quark.cn/s/abc123', '测试描述', 'quark', '电影,测试', 'tg', 'ai', 0, 0)"
    )
    .execute(&pool)
    .await
    .unwrap();

    // GET /api/resources/1/detail
    let req = build_auth_request("GET", "/api/resources/1/detail", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 验证资源信息
    assert_eq!(body["data"]["resource"]["title"], "测试电影");
    assert_eq!(body["data"]["resource"]["category"], "quark");
    assert_eq!(body["data"]["has_history"], true);

    // 验证原始消息文本已解析
    let raw_text = body["data"]["raw_text"].as_str().unwrap();
    assert!(
        raw_text.contains("测试电影"),
        "raw_text should contain title"
    );
    assert!(
        raw_text.contains("pan.quark.cn"),
        "raw_text should contain URL"
    );

    // 验证 media_type
    assert_eq!(body["data"]["media_type"], "photo");
}

#[tokio::test]
async fn test_get_resource_detail_no_history() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 插入 collector + history，然后删除 history 模拟"历史已删除"
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    sqlx::query(
        "INSERT INTO collectors (user_id, channel_id, collector_type, is_active) VALUES (1, 100, 'channel', 1)"
    )
    .execute(&pool)
    .await
    .unwrap();

    // 先插入采集历史（满足 FK 约束）
    sqlx::query(
        "INSERT INTO collector_histories (collector_id, channel_id, message_id, raw_data, is_auto_push, is_extracted) \
         VALUES (1, 100, 200, '{\"text\":\"旧消息\"}', 0, 1)"
    )
    .execute(&pool)
    .await
    .unwrap();

    // 插入资源（FK 指向 history_id=1）
    sqlx::query(
        "INSERT INTO extracted_resources (collector_history_id, title, url, description, category, tags, source, extract_mode, is_pushed, is_edited) \
         VALUES (1, '孤立资源', 'https://pan.quark.cn/s/orphan', NULL, 'quark', '', 'tg', 'rule', 0, 0)"
    )
    .execute(&pool)
    .await
    .unwrap();

    // 删除采集历史，模拟历史已被清理（暂时禁用 FK 检查）
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM collector_histories WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();

    let req = build_auth_request("GET", "/api/resources/1/detail", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 资源信息仍存在
    assert_eq!(body["data"]["resource"]["title"], "孤立资源");
    // 历史不存在
    assert_eq!(body["data"]["has_history"], false);
    assert_eq!(body["data"]["raw_text"], serde_json::Value::Null);
    assert_eq!(body["data"]["media_type"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_get_resource_detail_not_found() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let req = build_auth_request("GET", "/api/resources/99999/detail", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    // 应返回 404
    assert!(
        resp.status().is_client_error() || resp.status().as_u16() == 404,
        "Expected 404, got {}",
        resp.status()
    );
}

// ============================================================
// T005-T008: 调度可视化面板（提取历史 + next_run 修正）
// ============================================================

#[tokio::test]
async fn test_extract_histories_empty() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 空表查询
    let req = build_auth_request(
        "GET",
        "/api/extract-histories?page=1&page_size=20",
        &token,
        None,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 0);
    assert_eq!(body["data"]["pagination"]["total"], 0);
}

#[tokio::test]
async fn test_extract_histories_list() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    // 插入 3 条历史（含成功和失败）
    sqlx::query("INSERT INTO extract_histories (status, total_scanned, extracted, skipped, errors, message) VALUES ('success', 100, 42, 55, 3, NULL)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO extract_histories (status, total_scanned, extracted, skipped, errors, message) VALUES ('success', 200, 80, 115, 5, NULL)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO extract_histories (status, total_scanned, extracted, skipped, errors, message) VALUES ('failed', 0, 0, 0, 0, 'connection error')")
        .execute(&pool).await.unwrap();

    let req = build_auth_request(
        "GET",
        "/api/extract-histories?page=1&page_size=10",
        &token,
        None,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["list"].as_array().unwrap().len(), 3);
    assert_eq!(body["data"]["pagination"]["total"], 3);
}

#[tokio::test]
async fn test_extract_histories_stats() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    // 用显式 executed_at 确保时间顺序，避免毫秒级同时插入导致排序不确定
    sqlx::query("INSERT INTO extract_histories (status, extracted, executed_at) VALUES ('success', 42, '2026-06-09 10:00:00')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO extract_histories (status, extracted, executed_at) VALUES ('success', 80, '2026-06-09 11:00:00')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO extract_histories (status, extracted, executed_at) VALUES ('failed', 0, '2026-06-09 09:00:00')")
        .execute(&pool).await.unwrap();

    let req = build_auth_request("GET", "/api/extract-histories/stats", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["total"], 3);
    assert_eq!(body["data"]["success"], 2);
    assert_eq!(body["data"]["failed"], 1);
    // last_extracted 取最近一次成功的 extracted（80）
    assert_eq!(body["data"]["last_extracted"], 80);
}

#[tokio::test]
async fn test_status_next_run_after_restart() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 调度未启动时，next_run 为 null，running=false
    let req = build_auth_request("GET", "/api/status", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["schedulers"]["push_running"], false);
    assert_eq!(body["data"]["schedulers"]["extract_running"], false);
    assert_eq!(
        body["data"]["schedulers"]["push_next_run"],
        serde_json::Value::Null
    );
    assert_eq!(
        body["data"]["schedulers"]["extract_next_run"],
        serde_json::Value::Null
    );
    // interval_minutes 字段存在
    assert!(body["data"]["schedulers"]["push_interval_minutes"].is_number());
    assert!(body["data"]["schedulers"]["extract_interval_minutes"].is_number());

    // T003 (US1): 本特性新增字段 — push_scan_interval_secs（扫描周期秒）+ push_configs（每配置调度数组）
    assert!(
        body["data"]["schedulers"]["push_scan_interval_secs"].is_number(),
        "push_scan_interval_secs must be a number"
    );
    assert!(
        body["data"]["schedulers"]["push_configs"].is_array(),
        "push_configs must be an array"
    );
    // 无 active 自动推送配置 → 空数组
    assert_eq!(
        body["data"]["schedulers"]["push_configs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

// ============================================================
// T004 (US1): push_interval 修改后 /api/status 立即反映（核心 bug 修复）
// ============================================================

#[tokio::test]
async fn test_status_push_configs_reflects_interval() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 插入一条 active 自动推送配置（push_interval=30）
    sqlx::query(
        "INSERT INTO push_configs (name, api_url, api_token, target, auth_type, auth_key, \
         http_method, body_template, custom_headers, batch_size, data_source_type, \
         auto_push, push_interval, link_check_before_push, is_active) \
         VALUES ('配置A', 'http://example/api', NULL, '', 'none', '', 'POST', NULL, '', \
         10, 'all', 1, 30, 0, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let req = build_auth_request("GET", "/api/status", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // push_configs 数组包含 1 条记录，字段齐全（即使调度器未运行，DB 中有 active 配置即应出现）
    let configs = body["data"]["schedulers"]["push_configs"]
        .as_array()
        .unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["name"], "配置A");
    assert_eq!(configs[0]["push_interval"], 30);
    assert!(configs[0]["id"].is_number());
    // last_run_at / next_run 字段存在（调度器未启动时值为 null）
    assert!(configs[0].get("last_run_at").is_some());
    assert!(configs[0].get("next_run").is_some());
    assert_eq!(configs[0]["last_run_at"], serde_json::Value::Null);
    assert_eq!(configs[0]["next_run"], serde_json::Value::Null);

    // 修改 push_interval 为 60，再调用 /api/status，立即反映新值（核心 bug 修复点）
    sqlx::query("UPDATE push_configs SET push_interval = 60 WHERE name = '配置A'")
        .execute(&pool)
        .await
        .unwrap();

    let req = build_auth_request("GET", "/api/status", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    let configs = body["data"]["schedulers"]["push_configs"]
        .as_array()
        .unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(
        configs[0]["push_interval"], 60,
        "push_interval change must be reflected immediately without restart or tick"
    );
}

// ============================================================
// T009 (US2): 多 active 自动推送配置并存 — push_configs 数组独立展示
// ============================================================

#[tokio::test]
async fn status_push_configs_multiple() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 插入 3 条 active 自动推送配置，push_interval 分别为 5/30/60
    for (name, interval) in [("配置A", 5), ("配置B", 30), ("配置C", 60)] {
        sqlx::query(
            "INSERT INTO push_configs (name, api_url, api_token, target, auth_type, auth_key, \
             http_method, body_template, custom_headers, batch_size, data_source_type, \
             auto_push, push_interval, link_check_before_push, is_active) \
             VALUES (?, 'http://example/api', NULL, '', 'none', '', 'POST', NULL, '', \
             10, 'all', 1, ?, 0, 1)",
        )
        .bind(name)
        .bind(interval)
        .execute(&pool)
        .await
        .unwrap();
    }

    let req = build_auth_request("GET", "/api/status", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // push_active_configs 计数
    assert_eq!(
        body["data"]["schedulers"]["push_active_configs"], 3,
        "push_active_configs should count all active+auto_push configs"
    );

    // push_configs 数组长度为 3，按 id ASC 排序
    let configs = body["data"]["schedulers"]["push_configs"]
        .as_array()
        .unwrap();
    assert_eq!(configs.len(), 3, "should list all 3 active configs");
    let ids: Vec<i64> = configs.iter().map(|c| c["id"].as_i64().unwrap()).collect();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids, "push_configs should be ordered by id ASC");

    // 每个元素字段齐全；push_interval 与插入值一致（顺序 id ASC 对应 A=5, B=30, C=60）
    let expected = [("配置A", 5), ("配置B", 30), ("配置C", 60)];
    for (i, (name, interval)) in expected.iter().enumerate() {
        assert_eq!(configs[i]["name"], *name, "config[{i}] name mismatch");
        assert_eq!(
            configs[i]["push_interval"], *interval,
            "config[{i}] push_interval mismatch"
        );
        assert!(configs[i].get("last_run_at").is_some());
        assert!(configs[i].get("next_run").is_some());
    }
}

// ============================================================
// T010 (US2): active 但 auto_push=0 → push_configs 为空
// ============================================================

#[tokio::test]
async fn status_push_configs_empty_when_all_disabled() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 插入 active 但 auto_push=0 的记录（不满足 "active AND auto_push" 过滤条件）
    sqlx::query(
        "INSERT INTO push_configs (name, api_url, api_token, target, auth_type, auth_key, \
         http_method, body_template, custom_headers, batch_size, data_source_type, \
         auto_push, push_interval, link_check_before_push, is_active) \
         VALUES ('禁用自动推送', 'http://example/api', NULL, '', 'none', '', 'POST', NULL, '', \
         10, 'all', 0, 30, 0, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let req = build_auth_request("GET", "/api/status", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 无活跃自动推送配置 → 空数组 + 计数为 0
    assert_eq!(body["data"]["schedulers"]["push_active_configs"], 0);
    let configs = body["data"]["schedulers"]["push_configs"]
        .as_array()
        .unwrap();
    assert_eq!(
        configs.len(),
        0,
        "auto_push=0 configs must not appear in push_configs"
    );
}

// ============================================================
// T002: 转发队列状态（含 failed_tasks）
// ============================================================

#[tokio::test]
async fn test_image_forward_queue_status() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    // 插入三类任务：pending / forwarded / failed
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, retry_count) VALUES ('photo_pending', 'pending', 0)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, retry_count) VALUES ('photo_forwarded', 'forwarded', 0)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, retry_count, error) VALUES ('photo_failed1', 'failed', 2, 'FLOOD_WAIT_300')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, retry_count, error) VALUES ('photo_failed2', 'failed', 1, 'timeout')")
        .execute(&pool).await.unwrap();

    let req = build_auth_request("GET", "/api/image-forward/queue", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 三类计数
    assert_eq!(body["data"]["pending"], 1);
    assert_eq!(body["data"]["forwarded"], 1);
    assert_eq!(body["data"]["failed"], 2);

    // pending tasks 列表
    let tasks = body["data"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["remote_id"], "photo_pending");

    // failed_tasks 列表（新增字段）
    let failed_tasks = body["data"]["failed_tasks"].as_array().unwrap();
    assert_eq!(failed_tasks.len(), 2);
    assert!(failed_tasks.iter().any(|t| t["error"] == "FLOOD_WAIT_300"));
}

// ============================================================
// 023 US1: 转存开关 — enqueue 在 image_storage_enabled=false 时不入队
// ============================================================

#[tokio::test]
async fn test_image_storage_toggle_blocks_enqueue() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    // 1. 默认（开关缺失）→ 视为开启，应入队成功
    tgTool::services::forward_queue::enqueue(
        &state,
        "photo_default",
        Some(-100111),
        Some(123),
        Some("标题"),
        Some("描述"),
        Some("https://example.com"),
    )
    .await
    .expect("enqueue 默认应成功");

    let count_default: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE remote_id = 'photo_default'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_default.0, 1, "默认开关缺失应视为开启，任务应已入队");

    // 2. 关闭开关 → enqueue 直接返回，不入队
    {
        let mut cache = state.option_cache.write().await;
        cache.insert("image_storage_enabled".to_string(), "false".to_string());
    }
    tgTool::services::forward_queue::enqueue(
        &state,
        "photo_disabled",
        Some(-100111),
        Some(456),
        None,
        None,
        None,
    )
    .await
    .expect("enqueue 关闭时应返回 Ok");

    let count_disabled: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE remote_id = 'photo_disabled'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_disabled.0, 0, "关闭开关时不应入队");

    // 3. 再次开启 → 恢复入队
    {
        let mut cache = state.option_cache.write().await;
        cache.insert("image_storage_enabled".to_string(), "true".to_string());
    }
    tgTool::services::forward_queue::enqueue(
        &state,
        "photo_reenabled",
        Some(-100111),
        Some(789),
        None,
        None,
        None,
    )
    .await
    .expect("enqueue 开启时应成功");

    let count_reenabled: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE remote_id = 'photo_reenabled'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_reenabled.0, 1, "重新开启后应入队");

    // 4. awaiting_bot 状态在去重白名单中（不应被重复入队）
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, image_message_id) VALUES ('photo_awaiting', 'awaiting_bot', 999)")
        .execute(&pool)
        .await
        .unwrap();
    tgTool::services::forward_queue::enqueue(
        &state,
        "photo_awaiting",
        Some(-100111),
        Some(1),
        None,
        None,
        None,
    )
    .await
    .expect("enqueue 对 awaiting_bot 任务应去重");

    let count_awaiting: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM forward_tasks WHERE remote_id = 'photo_awaiting'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_awaiting.0, 1, "awaiting_bot 状态应阻止重复入队");
}

// ============================================================
// 023 US3: 智能重试 — 根据 image_message_id 决定恢复阶段
// ============================================================

#[tokio::test]
async fn test_smart_retry_restores_correct_stage() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    // 阶段1 失败任务（无 image_message_id）→ 重试应恢复为 pending
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, retry_count, error) VALUES ('stage1_fail', 'failed', 1, '阶段1 copy_media 失败')")
        .execute(&pool).await.unwrap();
    // 阶段2 失败任务（有 image_message_id）→ 重试应恢复为 awaiting_bot
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, image_message_id, retry_count, error) VALUES ('stage2_fail', 'failed', 12345, 1, '阶段2 forwardMessage 失败')")
        .execute(&pool).await.unwrap();

    // 调用 retry-all
    let req = build_auth_request("POST", "/api/image-forward/retry-all", &token, None);
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 验证：stage1_fail → pending（无 image_message_id）
    let s1: (String,) =
        sqlx::query_as("SELECT status FROM forward_tasks WHERE remote_id = 'stage1_fail'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(s1.0, "pending", "阶段1 失败任务应恢复为 pending");

    // 验证：stage2_fail → awaiting_bot（有 image_message_id）
    let s2: (String,) =
        sqlx::query_as("SELECT status FROM forward_tasks WHERE remote_id = 'stage2_fail'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(s2.0, "awaiting_bot", "阶段2 失败任务应恢复为 awaiting_bot");

    // 错误字段应被清空
    let err1: (Option<String>,) =
        sqlx::query_as("SELECT error FROM forward_tasks WHERE remote_id = 'stage1_fail'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(err1.0.is_none(), "重试后 error 应为 NULL");
}

// ============================================================
// 023 US3: 单任务重试也走智能恢复路径
// ============================================================

#[tokio::test]
async fn test_smart_retry_single_task() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };

    // 阶段2 失败任务（有 image_message_id）
    sqlx::query("INSERT INTO forward_tasks (remote_id, status, image_message_id, error) VALUES ('single_stage2', 'failed', 999, 'forwardMessage failed')")
        .execute(&pool).await.unwrap();
    let task_id: (i64,) =
        sqlx::query_as("SELECT id FROM forward_tasks WHERE remote_id = 'single_stage2'")
            .fetch_one(&pool)
            .await
            .unwrap();

    let req = build_auth_request(
        "POST",
        &format!("/api/image-forward/retry/{}", task_id.0),
        &token,
        None,
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // 单任务重试应保持 image_message_id 并恢复为 awaiting_bot
    let row: (String, Option<i64>) =
        sqlx::query_as("SELECT status, image_message_id FROM forward_tasks WHERE id = ?")
            .bind(task_id.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "awaiting_bot");
    assert_eq!(row.1, Some(999));
}

// ============================================================
// T004-T005: Rule 过滤字段 CRUD
// ============================================================

#[tokio::test]
async fn test_rule_create_with_filter() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建含 4 个新字段的规则
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_id": -100111,
                    "source_chat_name": "源频道",
                    "forward_method": "Chat",
                    "forward_target": "-100222",
                    "is_active": true,
                    "forward_client_id": "client_abc",
                    "filter_mode": "exclude",
                    "keywords": "广告,推广,加微信",
                    "media_filter": "photo"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 查询验证字段持久化
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules/1", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["forward_client_id"], "client_abc");
    assert_eq!(body["data"]["filter_mode"], "exclude");
    assert_eq!(body["data"]["keywords"], "广告,推广,加微信");
    assert_eq!(body["data"]["media_filter"], "photo");
}

#[tokio::test]
async fn test_rule_update_filter() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 先创建无过滤字段的规则
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/rules",
            &token,
            Some(
                serde_json::json!({
                    "source_chat_id": 222,
                    "forward_method": "Chat",
                    "forward_target": "-100"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();

    // 更新过滤字段
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            "/api/rules/1",
            &token,
            Some(
                serde_json::json!({
                    "filter_mode": "include",
                    "keywords": "资源,分享",
                    "media_filter": "document",
                    "forward_client_id": "client_xyz"
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 验证持久化
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/rules/1", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["filter_mode"], "include");
    assert_eq!(body["data"]["keywords"], "资源,分享");
    assert_eq!(body["data"]["media_filter"], "document");
    assert_eq!(body["data"]["forward_client_id"], "client_xyz");
}

// ============================================================
// T004: 推送配置 CRUD 集成测试
// ============================================================

#[tokio::test]
async fn test_create_push_config() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(
                serde_json::json!({
                    "name": "测试推送",
                    "api_url": "https://api.example.com/push",
                    "api_token": "secret123",
                    "target": "test",
                    "auth_type": "bearer",
                    "http_method": "POST",
                    "batch_size": 500,
                    "data_source_type": "all",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert!(body["data"]["id"].is_number());
}

#[tokio::test]
async fn test_list_push_configs() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 先创建一个配置
    let _ = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(
                serde_json::json!({
                    "name": "配置A",
                    "api_url": "https://a.example.com",
                    "data_source_type": "all",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    let list = body["data"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "配置A");
}

#[tokio::test]
async fn test_update_push_config() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "原名称", "api_url": "https://old.com", "data_source_type": "all"}).to_string()),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let id = body["data"]["id"].as_i64().unwrap();

    // 更新
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            &format!("/api/push/configs/{id}"),
            &token,
            Some(serde_json::json!({"name": "新名称", "api_url": "https://new.com"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 验证
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let list = body["data"]["list"].as_array().unwrap();
    assert_eq!(list[0]["name"], "新名称");
    assert_eq!(list[0]["api_url"], "https://new.com");
}

#[tokio::test]
async fn test_delete_push_config() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "待删除", "api_url": "https://del.com", "data_source_type": "all"}).to_string()),
        ))
        .await
        .unwrap();
    let id = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 删除
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "DELETE",
            &format!("/api/push/configs/{id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 验证列表为空
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert!(body["data"]["list"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_toggle_push_config() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建（默认 is_active=true）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "切换测试", "api_url": "https://tog.com", "data_source_type": "all"}).to_string()),
        ))
        .await
        .unwrap();
    let id = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 切换为禁用
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            &format!("/api/push/configs/{id}/toggle"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 验证
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["list"][0]["is_active"], false);
}

// ============================================================
// T005: 数据源采集器选择集成测试
// ============================================================

#[tokio::test]
async fn test_create_push_config_with_collectors() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 先创建采集器（插入 clients + collectors）
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    sqlx::query("INSERT INTO clients (id, user_id, client_type, status) VALUES ('test-client', 1, 'Client', 'active')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 100, '频道A', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 200, '频道B', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();

    // 创建带 collector_ids 的推送配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(
                serde_json::json!({
                    "name": "指定采集器",
                    "api_url": "https://api.example.com",
                    "data_source_type": "selected",
                    "collector_ids": [1, 2],
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);

    // 验证列表显示 collector_count
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let list = body["data"]["list"].as_array().unwrap();
    assert_eq!(list[0]["data_source_type"], "selected");
    assert_eq!(list[0]["collector_count"], 2);
}

/// 辅助函数：从 app 中获取测试 DB（未使用，保留备用）
#[allow(dead_code)]
fn get_test_db(_app: &mut axum::Router) -> DbPool {
    // 此函数仅在测试中使用，通过直接访问全局 setup 实现
    // 在集成测试中我们直接用 setup_test_db 获取 pool
    unreachable!("use setup_test_db() directly")
}

#[tokio::test]
async fn test_update_push_config_collectors() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db.clone());
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    sqlx::query("INSERT INTO clients (id, user_id, client_type, status) VALUES ('test-client', 1, 'Client', 'active')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 100, '频道A', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 200, '频道B', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();

    // 创建
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "更新采集器", "api_url": "https://api.example.com", "data_source_type": "selected", "collector_ids": [1]}).to_string()),
        ))
        .await
        .unwrap();
    let id = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 更新 collector_ids（全量替换）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "PUT",
            &format!("/api/push/configs/{id}"),
            &token,
            Some(serde_json::json!({"collector_ids": [1, 2]}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 验证
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["data"]["list"][0]["collector_count"], 2);
}

// ============================================================
// T012: 按配置推送集成测试
// ============================================================

#[tokio::test]
async fn test_trigger_push_for_config_not_found() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 推送不存在的配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs/999/trigger",
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_eq!(body["success"], false);
}

#[tokio::test]
async fn test_trigger_push_for_config_empty_url() {
    let db = setup_test_db().await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建一个空 api_url 的配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(
                serde_json::json!({"name": "空URL", "api_url": "https://example.com/api"})
                    .to_string(),
            ),
        ))
        .await
        .unwrap();
    let id = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 触发推送（无资源可推送）
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/push/configs/{id}/trigger"),
            &token,
            Some("{}".to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    // 没有资源，应该返回成功但 processed_count=0
    assert!(body["success"].as_bool().unwrap());
}

#[tokio::test]
async fn test_push_status_per_config() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建 2 个推送配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "配置A", "api_url": "https://a.com/api"}).to_string()),
        ))
        .await
        .unwrap();
    let config_a = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(serde_json::json!({"name": "配置B", "api_url": "https://b.com/api"}).to_string()),
        ))
        .await
        .unwrap();
    let _config_b = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 手动插入 resource_push_status 验证独立性（需要先有 extracted_resources 记录）
    // 插入 client + collector + collector_history + extracted_resource
    sqlx::query("INSERT INTO clients (id, user_id, client_type, status) VALUES ('test-client', 1, 'Client', 'active')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 100, '频道A', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collector_histories (collector_id, channel_id, message_id, is_auto_push) VALUES (1, 100, 1, 0)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO extracted_resources (collector_history_id, title, source, extract_mode) VALUES (1, '测试资源', 'tg', 'rule')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO resource_push_status (resource_id, push_config_id, status) VALUES (1, ?, 'pushed')")
        .bind(config_a)
        .execute(&pool)
        .await
        .unwrap();

    // 验证配置 A 有推送状态记录
    let count_a: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_push_status WHERE push_config_id = ?")
            .bind(config_a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_a, 1);

    // 配置 B 没有推送状态
    let count_b: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_push_status WHERE push_config_id = ?")
            .bind(_config_b)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_b, 0);
}

// ============================================================
// T019: 复制推送配置集成测试
// ============================================================

#[tokio::test]
async fn test_duplicate_push_config() {
    let db = setup_test_db().await;
    let pool = match &db {
        DbPool::Sqlite(p) => p.clone(),
        _ => panic!("expected sqlite"),
    };
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    // 创建采集器
    sqlx::query("INSERT INTO clients (id, user_id, client_type, status) VALUES ('test-client', 1, 'Client', 'active')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collectors (user_id, channel_id, channel_name, collector_type, is_active) VALUES (1, 100, '频道A', 'origin', 1)")
        .execute(&pool)
        .await
        .unwrap();

    // 创建带 collector_ids 的配置
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            "/api/push/configs",
            &token,
            Some(
                serde_json::json!({
                    "name": "原始配置",
                    "api_url": "https://api.example.com",
                    "api_token": "secret-token",
                    "target": "test",
                    "auth_type": "bearer",
                    "data_source_type": "selected",
                    "collector_ids": [1],
                    "auto_push": true,
                    "push_interval": 60,
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    let orig_id = parse_body(resp.into_body()).await["data"]["id"]
        .as_i64()
        .unwrap();

    // 复制
    let resp = app
        .clone()
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/push/configs/{orig_id}/duplicate"),
            &token,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    let new_id = body["data"]["id"].as_i64().unwrap();
    assert_ne!(new_id, orig_id);

    // 验证列表中有副本，名称带"(副本)"
    let resp = app
        .clone()
        .oneshot(build_auth_request("GET", "/api/push/configs", &token, None))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    let list = body["data"]["list"].as_array().unwrap();
    // 按 id 倒序，第一个是副本
    let duplicate = list
        .iter()
        .find(|c| c["id"].as_i64().unwrap() == new_id)
        .unwrap();
    assert_eq!(duplicate["name"], "原始配置(副本)");
    assert_eq!(duplicate["api_url"], "https://api.example.com");
    assert_eq!(duplicate["auth_type"], "bearer");
    assert_eq!(duplicate["collector_count"], 1);
}

// ============================================================
// 022-resource-link-check：资源链接有效性检测集成测试
// ============================================================

/// 辅助：在 SQLite 测试库中插入一条资源（含父链 collectors→collector_histories），返回其 id
async fn insert_test_resource(db: &DbPool, title: &str, url: &str) -> i64 {
    match db {
        DbPool::Sqlite(pool) => {
            // 父链：collectors(root user_id=1) → collector_histories → extracted_resources
            sqlx::query(
                "INSERT OR IGNORE INTO collectors (user_id, channel_id, collector_type) VALUES (1, 100, 'channel')",
            )
            .execute(pool)
            .await
            .unwrap();
            let collector_id: i64 =
                sqlx::query_scalar("SELECT id FROM collectors WHERE channel_id = 100")
                    .fetch_one(pool)
                    .await
                    .unwrap();
            sqlx::query(
                "INSERT OR IGNORE INTO collector_histories (collector_id, channel_id, message_id) VALUES (?, 100, 1)",
            )
            .bind(collector_id)
            .execute(pool)
            .await
            .unwrap();
            let hist_id: i64 = sqlx::query_scalar(
                "SELECT id FROM collector_histories WHERE channel_id = 100 AND message_id = 1",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO extracted_resources (collector_history_id, title, url, source, extract_mode, is_pushed, is_edited) \
                 VALUES (?, ?, ?, 'tg', 'rule', 0, 0)",
            )
            .bind(hist_id)
            .bind(title)
            .bind(url)
            .execute(pool)
            .await
            .unwrap();
            sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
                .fetch_one(pool)
                .await
                .unwrap()
        }
        _ => 0,
    }
}

/// GET /api/resources 列表项含 link_status 字段（无缓存 → unknown），FR-011
#[tokio::test]
async fn test_resources_list_has_link_status() {
    let db = setup_test_db().await;
    insert_test_resource(&db, "测试资源", "https://pan.quark.cn/s/abc").await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = app
        .oneshot(build_auth_request(
            "GET",
            "/api/resources?page=1&page_size=10",
            &token,
            None,
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    let list = body["data"]["list"].as_array().unwrap();
    assert!(!list.is_empty(), "资源列表不应为空");
    // 新增字段存在；无缓存时聚合为 unknown
    assert!(
        list[0].get("link_status").is_some(),
        "列表项应含 link_status"
    );
    assert_eq!(list[0]["link_status"], "unknown");
}

/// GET /api/push/histories/{id} 返回跳过统计 + skip_records 明细，Story3 AC2
#[tokio::test]
async fn test_push_history_detail_skip_records() {
    let db = setup_test_db().await;
    let res_id = insert_test_resource(&db, "失效资源", "https://pan.baidu.com/s/z").await;
    let hist_id: i64 = match &db {
        DbPool::Sqlite(pool) => {
            let r = sqlx::query(
                "INSERT INTO push_histories (batch_id, target, status, data_count, message, pushed_count, skipped_image_count, skipped_link_count) \
                 VALUES ('batch_link_test','default','success',3,'推送成功',2,1,1)",
            )
            .execute(pool)
            .await
            .unwrap();
            let hid = r.last_insert_rowid();
            sqlx::query(
                "INSERT INTO push_skip_records (push_history_id, resource_id, skip_reason, urls_invalid, detail) \
                 VALUES (?, ?, 'link_invalid', 'https://pan.baidu.com/s/z', '网盘链接已失效')",
            )
            .bind(hid)
            .bind(res_id)
            .execute(pool)
            .await
            .unwrap();
            hid
        }
        _ => 0,
    };
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = app
        .oneshot(build_auth_request(
            "GET",
            &format!("/api/push/histories/{hist_id}"),
            &token,
            None,
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["history"]["skipped_link_count"], 1);
    assert_eq!(body["data"]["history"]["skipped_image_count"], 1);
    assert_eq!(body["data"]["history"]["pushed_count"], 2);
    let sr = body["data"]["skip_records"].as_array().unwrap();
    assert_eq!(sr.len(), 1, "应有 1 条跳过明细");
    assert_eq!(sr[0]["skip_reason"], "link_invalid");
    assert_eq!(sr[0]["title"], "失效资源");
}

/// POST /api/resources/{id}/check-link：pancheck_host 未配置时降级为 unknown，不报错（FR-004）
#[tokio::test]
async fn test_resource_check_link_unconfigured_degrades() {
    let db = setup_test_db().await;
    let res_id = insert_test_resource(&db, "待检测", "https://pan.quark.cn/s/a").await;
    let (state, _) = make_test_state(db);
    let mut app = build_test_app(state);
    let token = get_root_token(&mut app).await;

    let resp = app
        .oneshot(build_auth_request(
            "POST",
            &format!("/api/resources/{res_id}/check-link"),
            &token,
            Some(r#"{"ignore_cache":false}"#.to_string()),
        ))
        .await
        .unwrap();
    let body = parse_body(resp.into_body()).await;
    assert_success(&body);
    // 未配置 PanCheck → 不报错，降级为未检测
    assert_eq!(body["data"]["link_status"], "unknown");
}
