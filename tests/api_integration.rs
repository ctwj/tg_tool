//! API 集成测试 — 使用 SQLite 内存数据库测试完整路由

use axum::body::Body;
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use tgTool::state::{AppState, DbPool};
use tgTool::config::Config;
use tgTool::services::crypto;
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
    let migration_sql = include_str!("../migrations/001_init.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("Failed to run test migrations");

    // 插入 root 用户（使用当前 bcrypt 版本生成 hash）
    let hash = crypto::hash_password("123456").expect("Failed to hash root password");
    sqlx::query("INSERT INTO users (username, password, role, status) VALUES ('root', ?, 100, 1)")
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("Failed to insert root user");

    DbPool::Sqlite(pool)
}

/// 创建测试用的 AppState
fn make_test_state(db: DbPool) -> AppState {
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
    };
    AppState::new(db, config)
}

/// 构建 axum 测试 Router
fn build_test_app(state: AppState) -> axum::Router {
    tgTool::routes::build_router(state)
        .layer(tgTool::middleware::cors::cors_layer())
}

/// 从 Response body 读取并解析 JSON
async fn parse_body(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// 构建一个 HTTP request
fn build_request(method: &str, uri: &str, body: Option<String>) -> axum::http::Request<Body> {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder.body(body.map_or_else(Body::empty, Body::from)).unwrap()
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
    let app = build_test_app(state);

    let req_body = serde_json::json!({"username": "dup", "password": "pass123"}).to_string();

    // 第一次注册成功
    let resp1 = app
        .clone()
        .oneshot(build_request("POST", "/api/auth/register", Some(req_body.clone())))
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
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
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 创建用户
    let response = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/users",
            Some(serde_json::json!({"username": "newuser", "password": "pass123", "role": 1}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除用户 id=2（id=1 是 root）
    let response = app
        .oneshot(build_request("DELETE", "/api/users/2", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_delete_root_user_forbidden() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request("DELETE", "/api/users/1", None))
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn test_list_users() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    let response = app
        .oneshot(build_request("GET", "/api/users?page=1&page_size=10", None))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_rule_crud() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 创建规则
    let response = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/rules",
            Some(serde_json::json!({
                "source_chat_id": 123456,
                "source_chat_name": "Test Channel",
                "forward_method": "Chat",
                "forward_target": "-100999",
                "is_active": true
            }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);

    // 列出规则
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/rules?page=1&page_size=10", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 切换规则状态
    let response = app
        .clone()
        .oneshot(build_request("PUT", "/api/rules/1/toggle", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除规则
    let response = app
        .oneshot(build_request("DELETE", "/api/rules/1", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_collector_crud() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 创建采集器
    let response = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/collectors",
            Some(serde_json::json!({
                "channel_id": 999888,
                "channel_name": "News Channel",
                "collector_type": "origin",
                "is_active": true,
                "remark": "test collector"
            }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 列出采集器
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/collectors?page=1&page_size=10", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 切换采集器状态
    let response = app
        .clone()
        .oneshot(build_request("PUT", "/api/collectors/1/toggle", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除采集器
    let response = app
        .oneshot(build_request("DELETE", "/api/collectors/1", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_client_add_and_remove() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 添加客户端
    let response = app
        .clone()
        .oneshot(build_request(
            "POST",
            "/api/clients",
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
        .oneshot(build_request("GET", "/api/clients", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 获取客户端状态（应为 new，因为 TgClientMap 中没有）
    let response = app
        .clone()
        .oneshot(build_request("GET", &format!("/api/clients/{client_id}"), None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_eq!(body["data"]["status"], "new");

    // 删除客户端
    let response = app
        .oneshot(build_request("DELETE", &format!("/api/clients/{client_id}"), None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_options_crud() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 获取初始 options
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/options", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 更新 options
    let response = app
        .clone()
        .oneshot(build_request(
            "PUT",
            "/api/options",
            Some(serde_json::json!({"push_api_url": "https://example.com", "push_interval": "30"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 再次获取 options，应该包含更新后的值
    let response = app
        .oneshot(build_request("GET", "/api/options", None))
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
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 推送统计
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/push/stats", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = parse_body(response.into_body()).await;
    assert_success(&body);
    assert_eq!(body["data"]["total"], 0);

    // 推送历史
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/push/histories?page=1&page_size=10", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 触发推送
    let response = app
        .clone()
        .oneshot(build_request("POST", "/api/push/trigger", Some("{}".to_string())))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 更新调度
    let response = app
        .oneshot(build_request("PUT", "/api/push/scheduler", Some("{}".to_string())))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_file_endpoints() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
    let app = build_test_app(state);

    // 列出文件
    let response = app
        .clone()
        .oneshot(build_request("GET", "/api/files?page=1&page_size=10", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // 删除不存在的文件（SQLite DELETE on missing row = no error）
    let response = app
        .oneshot(build_request("DELETE", "/api/files/999", None))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_404_for_unknown_api() {
    let db = setup_test_db().await;
    let state = make_test_state(db);
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
    let state = make_test_state(db);
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
