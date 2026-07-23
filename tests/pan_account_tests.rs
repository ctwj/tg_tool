//! 网盘账号服务层测试（feature 047 US1 — T013）
//! SQLite 内存库 + migration 020；用 uc/baidu 平台验证 DB/加密/脱敏逻辑，避免夸克真实网络调用

use base64::Engine;
use sqlx::sqlite::SqlitePoolOptions;
use tgTool::models::pan_account::{CreatePanAccount, UpdatePanAccount};
use tgTool::services::pan_account;
use tgTool::state::DbPool;

fn pan_key() -> String {
    base64::engine::general_purpose::STANDARD.encode([0x42u8; 32])
}

async fn setup() -> DbPool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect sqlite");
    let m = include_str!("../migrations/020_pan_management_sqlite.sql");
    sqlx::raw_sql(m)
        .execute(&pool)
        .await
        .expect("run migration 020");
    DbPool::Sqlite(pool)
}

fn uc_req() -> CreatePanAccount {
    CreatePanAccount {
        platform: "uc".into(),
        display_name: "UC号".into(),
        credential: "fake_cookie_value".into(),
        target_dir: "/tg/转存".into(),
    }
}

#[tokio::test]
async fn test_create_uc_marks_disabled_no_network() {
    // uc 驱动未实现：创建标记 disabled，且不触发夸克网络调用
    let db = setup().await;
    let view = pan_account::create_account(&db, &pan_key(), uc_req()).await.unwrap();
    assert_eq!(view.platform, "uc");
    assert_eq!(view.status, "disabled");
    assert_eq!(view.target_dir, "/tg/转存");
}

#[tokio::test]
async fn test_view_excludes_credential_and_plaintext() {
    // FR-002：脱敏视图不含密文/nonce/明文
    let db = setup().await;
    let view = pan_account::create_account(&db, &pan_key(), uc_req()).await.unwrap();
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("fake_cookie_value"));
    assert!(!json.contains("credential_cipher"));
    assert!(!json.contains("credential_nonce"));
}

#[tokio::test]
async fn test_list_and_get() {
    let db = setup().await;
    for i in 0..3 {
        let mut req = uc_req();
        req.display_name = format!("UC-{i}");
        pan_account::create_account(&db, &pan_key(), req).await.unwrap();
    }
    let list = pan_account::list_accounts(&db).await.unwrap();
    assert_eq!(list.len(), 3);
    let id = list[0].id;
    let one = pan_account::get_account_view(&db, id).await.unwrap();
    assert_eq!(one.display_name, list[0].display_name);
}

#[tokio::test]
async fn test_get_nonexistent_returns_err() {
    let db = setup().await;
    assert!(pan_account::get_account_view(&db, 9999).await.is_err());
}

#[tokio::test]
async fn test_update_fields_without_credential() {
    let db = setup().await;
    let created = pan_account::create_account(&db, &pan_key(), uc_req()).await.unwrap();
    let upd = UpdatePanAccount {
        display_name: Some("new-name".into()),
        credential: None,
        target_dir: Some("/new".into()),
    };
    let updated = pan_account::update_account(&db, &pan_key(), created.id, upd).await.unwrap();
    assert_eq!(updated.display_name, "new-name");
    assert_eq!(updated.target_dir, "/new");
}

#[tokio::test]
async fn test_delete_then_not_found() {
    let db = setup().await;
    let created = pan_account::create_account(&db, &pan_key(), uc_req()).await.unwrap();
    pan_account::delete_account(&db, created.id).await.unwrap();
    assert!(pan_account::get_account_view(&db, created.id).await.is_err());
    // 重复删除报错
    assert!(pan_account::delete_account(&db, created.id).await.is_err());
}

#[tokio::test]
async fn test_reject_unsupported_platform() {
    let db = setup().await;
    let mut req = uc_req();
    req.platform = "onedrive".into();
    assert!(pan_account::create_account(&db, &pan_key(), req).await.is_err());
}

#[tokio::test]
async fn test_reject_empty_credential() {
    let db = setup().await;
    let mut req = uc_req();
    req.credential = "   ".into();
    assert!(pan_account::create_account(&db, &pan_key(), req).await.is_err());
}

#[tokio::test]
async fn test_reject_bad_pan_key() {
    let db = setup().await;
    assert!(pan_account::create_account(&db, "!!!not-base64!!!", uc_req()).await.is_err());
}

#[tokio::test]
async fn test_check_unsupported_platform_err_no_network() {
    // uc 驱动未实现：check_account 报错，不触发网络
    let db = setup().await;
    let created = pan_account::create_account(&db, &pan_key(), uc_req()).await.unwrap();
    assert!(pan_account::check_account(&db, &pan_key(), created.id).await.is_err());
}
