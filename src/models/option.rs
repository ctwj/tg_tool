use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SysOption {
    pub id: i64,
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOptionsRequest {
    #[serde(flatten)]
    pub options: std::collections::HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_options_request_flatten() {
        let json = r#"{"push_api_url":"https://example.com/api","push_interval":"30"}"#;
        let req: UpdateOptionsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.options.get("push_api_url").unwrap(), "https://example.com/api");
        assert_eq!(req.options.get("push_interval").unwrap(), "30");
    }

    #[test]
    fn test_sys_option_serialization() {
        let opt = SysOption {
            id: 1,
            key: "theme".to_string(),
            value: Some("dark".to_string()),
        };
        let val = serde_json::to_value(&opt).unwrap();
        assert_eq!(val["key"], "theme");
        assert_eq!(val["value"], "dark");
    }

    #[test]
    fn test_sys_option_null_value() {
        let opt = SysOption {
            id: 2,
            key: "empty".to_string(),
            value: None,
        };
        let val = serde_json::to_value(&opt).unwrap();
        assert!(val["value"].is_null());
    }
}
