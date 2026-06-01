use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Rule {
    pub id: i64,
    pub user_id: i64,
    pub source_chat_id: i64,
    pub source_chat_name: Option<String>,
    pub forward_method: String,
    pub forward_config: Option<String>,
    pub forward_target: Option<String>,
    pub is_active: bool,
    pub remark: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRuleRequest {
    pub source_chat_id: i64,
    pub source_chat_name: Option<String>,
    pub forward_method: String,
    pub forward_config: Option<String>,
    pub forward_target: Option<String>,
    pub is_active: Option<bool>,
    pub remark: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rule_request_deserialize_full() {
        let json = r#"{"source_chat_id":123456,"source_chat_name":"Test Chat","forward_method":"Chat","forward_config":null,"forward_target":"-100123","is_active":true,"remark":"test rule"}"#;
        let req: CreateRuleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.source_chat_id, 123456);
        assert_eq!(req.source_chat_name, Some("Test Chat".to_string()));
        assert_eq!(req.forward_method, "Chat");
        assert_eq!(req.is_active, Some(true));
        assert_eq!(req.remark, Some("test rule".to_string()));
    }

    #[test]
    fn test_create_rule_request_minimal() {
        let json = r#"{"source_chat_id":111,"forward_method":"Webhook"}"#;
        let req: CreateRuleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.source_chat_id, 111);
        assert_eq!(req.forward_method, "Webhook");
        assert_eq!(req.source_chat_name, None);
        assert_eq!(req.is_active, None);
    }

    fn naive_now() -> NaiveDateTime {
        chrono::DateTime::from_timestamp(1700000000, 0)
            .unwrap()
            .naive_utc()
    }

    #[test]
    fn test_rule_serialization() {
        let rule = Rule {
            id: 1,
            user_id: 1,
            source_chat_id: 123456,
            source_chat_name: Some("My Channel".to_string()),
            forward_method: "Chat".to_string(),
            forward_config: None,
            forward_target: Some("-100999".to_string()),
            is_active: true,
            remark: None,
            created_at: naive_now(),
            updated_at: naive_now(),
        };
        let val = serde_json::to_value(&rule).unwrap();
        assert_eq!(val["source_chat_id"], 123456);
        assert_eq!(val["forward_method"], "Chat");
        assert!(val["is_active"].as_bool().unwrap());
    }
}
