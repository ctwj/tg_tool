use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 提取出的资源记录 — 从采集历史中通过规则/AI 提取的结构化资源
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExtractedResource {
    pub id: i64,
    pub collector_history_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub img: Option<String>,
    pub source: String,
    pub extra: Option<String>,
    pub extract_mode: String,
    pub is_pushed: bool,
    pub is_edited: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// 封面图转发状态，由 SQL 子查询填充：None/Some("pending")/Some("forwarded")/Some("failed")
    #[sqlx(default)]
    pub img_forward_status: Option<String>,
    /// 图床群组A 的消息ID（阶段1 copy_media 完成后写入），由 SQL 子查询填充
    #[sqlx(default)]
    pub image_message_id: Option<i64>,
    /// Bot 二次 forwardMessage 后获取到的图片 file_id（阶段2 完成后写入），由 SQL 子查询填充
    #[sqlx(default)]
    pub file_id: Option<String>,
}

/// 用于创建新资源记录的参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewExtractedResource {
    pub collector_history_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub img: Option<String>,
    pub source: String,
    pub extra: Option<String>,
    pub extract_mode: String,
}

/// 用于更新资源记录的参数
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateExtractedResource {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub category: Option<String>,
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_resource_serialization() {
        let new = NewExtractedResource {
            collector_history_id: 1,
            title: "测试资源".to_string(),
            url: Some("https://pan.quark.cn/s/xxx".to_string()),
            description: Some("资源描述".to_string()),
            category: Some("quark".to_string()),
            tags: Some("电影,动作".to_string()),
            img: None,
            source: "tg".to_string(),
            extra: None,
            extract_mode: "rule".to_string(),
        };
        let json = serde_json::to_string(&new).unwrap();
        assert!(json.contains("\"title\":\"测试资源\""));
        assert!(json.contains("\"category\":\"quark\""));
    }

    #[test]
    fn test_update_resource_partial() {
        let update = UpdateExtractedResource {
            title: Some("新标题".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"title\":\"新标题\""));
        assert!(json.contains("\"description\":null"));
    }
}
