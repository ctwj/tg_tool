use axum::Json;
use serde_json::{Value, json};

/// Build a success JSON response
pub fn success_response(data: Value) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": data,
    }))
}

/// Build a success JSON response with a message
pub fn success_message(message: &str) -> Json<Value> {
    Json(json!({
        "success": true,
        "message": message,
    }))
}

/// Build a paginated response
pub fn paginated_response<T: serde::Serialize>(
    list: Vec<T>,
    page: i64,
    page_size: i64,
    total: i64,
) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "list": list,
            "pagination": {
                "page": page,
                "page_size": page_size,
                "total": total,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_response() {
        let resp = success_response(json!({"id": 1, "name": "test"}));
        let val = resp.0;
        assert_eq!(val["success"], true);
        assert_eq!(val["data"]["id"], 1);
        assert_eq!(val["data"]["name"], "test");
    }

    #[test]
    fn test_success_message() {
        let resp = success_message("操作成功");
        let val = resp.0;
        assert_eq!(val["success"], true);
        assert_eq!(val["message"], "操作成功");
    }

    #[test]
    fn test_paginated_response() {
        let list = vec!["item1", "item2", "item3"];
        let resp = paginated_response(list, 1, 10, 100);
        let val = resp.0;

        assert_eq!(val["success"], true);
        assert_eq!(val["data"]["list"].as_array().unwrap().len(), 3);
        assert_eq!(val["data"]["pagination"]["page"], 1);
        assert_eq!(val["data"]["pagination"]["page_size"], 10);
        assert_eq!(val["data"]["pagination"]["total"], 100);
    }

    #[test]
    fn test_paginated_response_empty() {
        let list: Vec<String> = vec![];
        let resp = paginated_response(list, 2, 5, 0);
        let val = resp.0;

        assert_eq!(val["data"]["list"].as_array().unwrap().len(), 0);
        assert_eq!(val["data"]["pagination"]["total"], 0);
    }
}
