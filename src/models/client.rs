use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Client {
    pub id: String,
    pub user_id: i64,
    pub client_type: String,
    pub phone: Option<String>,
    pub token: Option<String>,
    pub status: String,
    pub session_path: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddClientRequest {
    pub client_type: String,
    pub phone: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientAuthRequest {
    pub auth_type: String, // "code" or "password"
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_client_request_deserialize() {
        let json = r#"{"client_type":"Client","phone":"+1234567890","token":null}"#;
        let req: AddClientRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.client_type, "Client");
        assert_eq!(req.phone, Some("+1234567890".to_string()));
        assert_eq!(req.token, None);
    }

    #[test]
    fn test_client_auth_request_code() {
        let json = r#"{"auth_type":"code","value":"12345"}"#;
        let req: ClientAuthRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.auth_type, "code");
        assert_eq!(req.value, "12345");
    }

    #[test]
    fn test_client_auth_request_password() {
        let json = r#"{"auth_type":"password","value":"mypassword"}"#;
        let req: ClientAuthRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.auth_type, "password");
        assert_eq!(req.value, "mypassword");
    }

    fn naive_now() -> NaiveDateTime {
        chrono::DateTime::from_timestamp(1700000000, 0)
            .unwrap()
            .naive_utc()
    }

    #[test]
    fn test_client_serialization_roundtrip() {
        let client = Client {
            id: "abc123".to_string(),
            user_id: 1,
            client_type: "Client".to_string(),
            phone: Some("+123".to_string()),
            token: None,
            status: "active".to_string(),
            session_path: Some("./sessions/abc123".to_string()),
            created_at: naive_now(),
            updated_at: naive_now(),
        };
        let json = serde_json::to_string(&client).unwrap();
        let parsed: Client = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "abc123");
        assert_eq!(parsed.status, "active");
    }
}
