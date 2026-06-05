use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: i32,
    pub status: i32,
    pub access_token: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
    pub role: Option<i32>,
    pub status: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub captcha_key: Option<String>,
    pub captcha_code: Option<String>,
}

/// User info returned to client (no password)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: i32,
    pub status: i32,
    pub access_token: Option<String>,
    pub created_at: NaiveDateTime,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            email: u.email,
            role: u.role,
            status: u.status,
            access_token: u.access_token,
            created_at: u.created_at,
        }
    }
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role >= 10
    }

    pub fn is_root(&self) -> bool {
        self.role >= 100
    }

    pub fn is_enabled(&self) -> bool {
        self.status == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_now() -> NaiveDateTime {
        chrono::DateTime::from_timestamp(1700000000, 0)
            .unwrap()
            .naive_utc()
    }

    fn make_user(role: i32, status: i32) -> User {
        User {
            id: 1,
            username: "testuser".to_string(),
            password: "hash".to_string(),
            display_name: None,
            email: None,
            role,
            status,
            access_token: None,
            created_at: naive_now(),
            updated_at: naive_now(),
        }
    }

    // --- Role checks ---

    #[test]
    fn test_is_admin_role_10() {
        let user = make_user(10, 1);
        assert!(user.is_admin());
    }

    #[test]
    fn test_is_admin_role_100() {
        let user = make_user(100, 1);
        assert!(user.is_admin());
        assert!(user.is_root());
    }

    #[test]
    fn test_is_admin_role_1_not_admin() {
        let user = make_user(1, 1);
        assert!(!user.is_admin());
        assert!(!user.is_root());
    }

    #[test]
    fn test_is_admin_role_0_not_admin() {
        let user = make_user(0, 1);
        assert!(!user.is_admin());
    }

    #[test]
    fn test_is_root_role_100() {
        let user = make_user(100, 1);
        assert!(user.is_root());
    }

    #[test]
    fn test_is_root_role_99_not_root() {
        let user = make_user(99, 1);
        assert!(!user.is_root());
        assert!(user.is_admin()); // 99 >= 10
    }

    // --- Status checks ---

    #[test]
    fn test_is_enabled_status_1() {
        let user = make_user(1, 1);
        assert!(user.is_enabled());
    }

    #[test]
    fn test_is_enabled_status_0() {
        let user = make_user(1, 0);
        assert!(!user.is_enabled());
    }

    // --- UserInfo conversion ---

    #[test]
    fn test_user_to_user_info() {
        let user = User {
            id: 42,
            username: "alice".to_string(),
            password: "secret_hash".to_string(),
            display_name: Some("Alice".to_string()),
            email: Some("alice@example.com".to_string()),
            role: 10,
            status: 1,
            access_token: Some("tok123".to_string()),
            created_at: naive_now(),
            updated_at: naive_now(),
        };

        let info: UserInfo = user.into();

        assert_eq!(info.id, 42);
        assert_eq!(info.username, "alice");
        assert_eq!(info.display_name, Some("Alice".to_string()));
        assert_eq!(info.email, Some("alice@example.com".to_string()));
        assert_eq!(info.role, 10);
        assert_eq!(info.status, 1);
        assert_eq!(info.access_token, Some("tok123".to_string()));
        // password is NOT in UserInfo
    }

    // --- Serialization ---

    #[test]
    fn test_login_request_deserialize() {
        let json = r#"{"username":"root","password":"123456"}"#;
        let req: LoginRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "root");
        assert_eq!(req.password, "123456");
    }

    #[test]
    fn test_create_user_request_deserialize() {
        let json = r#"{"username":"bob","password":"pass123","email":"bob@test.com","display_name":"Bob"}"#;
        let req: CreateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "bob");
        assert_eq!(req.password, "pass123");
        assert_eq!(req.email, Some("bob@test.com".to_string()));
        assert_eq!(req.display_name, Some("Bob".to_string()));
    }

    #[test]
    fn test_create_user_request_optional_fields() {
        let json = r#"{"username":"bob","password":"pass123"}"#;
        let req: CreateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.email, None);
        assert_eq!(req.display_name, None);
    }

    #[test]
    fn test_user_info_serialization() {
        let info = UserInfo {
            id: 1,
            username: "root".to_string(),
            display_name: None,
            email: None,
            role: 100,
            status: 1,
            access_token: None,
            created_at: naive_now(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["username"], "root");
        assert_eq!(json["role"], 100);
        assert!(json.get("password").is_none()); // no password field
    }
}
