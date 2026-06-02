// AI 大模型增强提取器 — 调用 OpenAI 兼容 API 进行结构化资源提取
// 支持多端点轮询、失败回退到规则结果

use crate::services::extractor::ResourceDraft;
use crate::state::OptionCache;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 全局轮询计数器 — 用于多端点轮询选择
static ENDPOINT_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// AI API 端点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiEndpoint {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_ai_type")]
    pub ai_type: String,
    pub url: String,
    pub key: String,
    pub model: String,
    #[serde(default = "default_true")]
    pub enable: bool,
    #[serde(default)]
    pub request_delay: u64,
}

fn default_ai_type() -> String {
    "openai".to_string()
}

fn default_true() -> bool {
    true
}

/// OpenAI 兼容 API 响应中的资源结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiExtractResult {
    pub title: String,
    #[serde(default)]
    pub url: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: String,
}

/// 从 option_cache 中解析 AI 端点列表（异步版）
pub async fn parse_ai_endpoints_async(option_cache: &OptionCache) -> Vec<AiEndpoint> {
    let cache = option_cache.read().await;
    let endpoints_json = match cache.get("push_ai_endpoints") {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return Vec::new(),
    };
    drop(cache);

    serde_json::from_str(&endpoints_json).unwrap_or_default()
}

/// 轮询选择下一个启用的端点
pub fn select_endpoint(endpoints: &[AiEndpoint]) -> Option<&AiEndpoint> {
    let enabled: Vec<&AiEndpoint> = endpoints.iter().filter(|e| e.enable).collect();
    if enabled.is_empty() {
        return None;
    }
    let idx = ENDPOINT_COUNTER.fetch_add(1, Ordering::Relaxed) % enabled.len();
    Some(enabled[idx])
}

/// 默认提示词模板
const DEFAULT_PROMPT: &str = "从以下 Telegram 消息中提取结构化资源信息。请返回 JSON 格式：{\"title\":\"资源标题\",\"url\":[\"链接列表\"],\"description\":\"描述\",\"category\":\"网盘类型\",\"tags\":\"标签,逗号分隔\"}\n\n消息内容：\n";

/// 调用 OpenAI 兼容 API 进行提取
pub async fn call_ai_api(
    endpoint: &AiEndpoint,
    prompt: &str,
    message: &str,
) -> Result<AiExtractResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let api_url = format!("{}/chat/completions", endpoint.url.trim_end_matches('/'));

    // 请求延迟：避免 API 限流
    if endpoint.request_delay > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(endpoint.request_delay)).await;
    }

    let system_prompt = if prompt.is_empty() {
        DEFAULT_PROMPT.to_string()
    } else {
        prompt.to_string()
    };

    let body = serde_json::json!({
        "model": endpoint.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": message}
        ],
        "temperature": 0.3,
    });

    let resp = client
        .post(&api_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", endpoint.key))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("AI API 请求失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("AI API 返回错误: status={status}, body={body}"));
    }

    let resp_json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 AI API 响应失败: {e}"))?;

    // 提取 content 字段
    let content = resp_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "AI API 响应格式异常: 无法提取 content".to_string())?;

    // 尝试从 content 中提取 JSON（可能包含 markdown 代码块）
    let json_str = extract_json_from_content(content);

    let result: AiExtractResult =
        serde_json::from_str(&json_str).map_err(|e| format!("解析 AI 返回 JSON 失败: {e}"))?;

    Ok(result)
}

/// 从 AI 返回的 content 中提取 JSON（处理 markdown 代码块包裹的情况）
fn extract_json_from_content(content: &str) -> String {
    let trimmed = content.trim();

    // 尝试直接解析
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }

    // 尝试提取 markdown 代码块中的 JSON
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim().to_string();
        }
    }

    // 尝试提取 ``` 代码块
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        // 跳过可能的语言标识行
        let after_ticks = &trimmed[json_start..];
        let json_start = after_ticks
            .find('{')
            .map(|pos| json_start + pos)
            .unwrap_or(json_start);
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim().to_string();
        }
    }

    // 尝试找第一个 { 到最后一个 }
    if let Some(start) = trimmed.find('{')
        && let Some(end) = trimmed.rfind('}')
        && end > start
    {
        return trimmed[start..=end].to_string();
    }

    trimmed.to_string()
}

/// AI 提取主入口 — 调用 API 增强规则提取结果，失败回退到 rule_result
pub async fn ai_extract(
    raw_data: &str,
    rule_result: &ResourceDraft,
    option_cache: &OptionCache,
) -> ResourceDraft {
    let endpoints = parse_ai_endpoints_async(option_cache).await;
    if endpoints.is_empty() {
        tracing::warn!("AI 提取模式已启用但未配置端点，回退到规则结果");
        return rule_result.clone();
    }

    let prompt = {
        let cache = option_cache.read().await;
        cache.get("push_ai_prompt").cloned().unwrap_or_default()
    };

    // 尝试轮询选择端点，故障自动切换到下一个
    let mut last_error = String::new();
    for _ in 0..endpoints.len().min(3) {
        let endpoint = match select_endpoint(&endpoints) {
            Some(ep) => ep.clone(),
            None => break,
        };

        match call_ai_api(&endpoint, &prompt, raw_data).await {
            Ok(ai_result) => {
                tracing::info!(
                    "AI 提取成功: endpoint={}, title={}",
                    endpoint.url,
                    ai_result.title
                );
                return ResourceDraft {
                    title: if ai_result.title.is_empty() {
                        rule_result.title.clone()
                    } else {
                        ai_result.title
                    },
                    url: if ai_result.url.is_empty() {
                        rule_result.url.clone()
                    } else {
                        ai_result.url
                    },
                    description: if ai_result.description.is_empty() {
                        rule_result.description.clone()
                    } else {
                        ai_result.description
                    },
                    category: if ai_result.category.is_empty() {
                        rule_result.category.clone()
                    } else {
                        ai_result.category
                    },
                    tags: if ai_result.tags.is_empty() {
                        rule_result.tags.clone()
                    } else {
                        ai_result.tags
                    },
                    source: "tg".to_string(),
                };
            }
            Err(e) => {
                tracing::warn!(
                    "AI 提取失败 (endpoint={}): {e}，尝试下一个端点",
                    endpoint.url
                );
                last_error = e;
            }
        }
    }

    tracing::warn!("所有 AI 端点均失败，回退到规则结果: {last_error}");
    rule_result.clone()
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_option_cache(map: HashMap<String, String>) -> OptionCache {
        Arc::new(RwLock::new(map))
    }

    // --- T011: Mock 成功的 AI 响应 ---

    #[tokio::test]
    async fn test_parse_ai_endpoints_valid() {
        let mut map = HashMap::new();
        map.insert(
            "push_ai_endpoints".to_string(),
            r#"[{"url":"https://api.openai.com","key":"sk-test","model":"gpt-4o"}]"#.to_string(),
        );
        let cache = make_option_cache(map);
        let endpoints = parse_ai_endpoints_async(&cache).await;
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].url, "https://api.openai.com");
        assert_eq!(endpoints[0].key, "sk-test");
        assert_eq!(endpoints[0].model, "gpt-4o");
    }

    #[tokio::test]
    async fn test_parse_ai_endpoints_empty() {
        let map = HashMap::new();
        let cache = make_option_cache(map);
        let endpoints = parse_ai_endpoints_async(&cache).await;
        assert!(endpoints.is_empty());
    }

    #[tokio::test]
    async fn test_parse_ai_endpoints_invalid_json() {
        let mut map = HashMap::new();
        map.insert("push_ai_endpoints".to_string(), "not json".to_string());
        let cache = make_option_cache(map);
        let endpoints = parse_ai_endpoints_async(&cache).await;
        assert!(endpoints.is_empty());
    }

    #[test]
    fn test_extract_json_from_content_plain() {
        let content = r#"{"title":"测试","url":[],"description":"","category":"","tags":""}"#;
        let result = extract_json_from_content(content);
        assert!(result.starts_with('{'));
        let parsed: AiExtractResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.title, "测试");
    }

    #[test]
    fn test_extract_json_from_content_markdown_block() {
        let content = "```json\n{\"title\":\"MD测试\",\"url\":[],\"description\":\"\",\"category\":\"\",\"tags\":\"\"}\n```";
        let result = extract_json_from_content(content);
        let parsed: AiExtractResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.title, "MD测试");
    }

    #[test]
    fn test_extract_json_from_content_with_prefix() {
        let content = "这是AI的回复：\n{\"title\":\"带前缀\",\"url\":[\"https://example.com\"],\"description\":\"\",\"category\":\"\",\"tags\":\"\"}\n结束";
        let result = extract_json_from_content(content);
        let parsed: AiExtractResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.title, "带前缀");
    }

    // --- T012: Mock 超时/错误 → 回退到规则结果 ---

    #[tokio::test]
    async fn test_ai_extract_fallback_on_empty_endpoints() {
        let map = HashMap::new();
        let cache = make_option_cache(map);
        let rule_result = ResourceDraft {
            title: "规则标题".to_string(),
            url: vec!["https://pan.quark.cn/s/test".to_string()],
            description: "规则描述".to_string(),
            category: "quark".to_string(),
            tags: "标签".to_string(),
            source: "tg".to_string(),
        };
        let result = ai_extract("原始消息", &rule_result, &cache).await;
        assert_eq!(result.title, "规则标题");
        assert_eq!(result.category, "quark");
    }

    // --- T013: 轮询选择 ---

    #[test]
    fn test_select_endpoint_round_robin() {
        let endpoints = vec![
            AiEndpoint {
                url: "https://api1.example.com".to_string(),
                key: "key1".to_string(),
                model: "model1".to_string(),
            },
            AiEndpoint {
                url: "https://api2.example.com".to_string(),
                key: "key2".to_string(),
                model: "model2".to_string(),
            },
            AiEndpoint {
                url: "https://api3.example.com".to_string(),
                key: "key3".to_string(),
                model: "model3".to_string(),
            },
        ];

        // 重置计数器
        ENDPOINT_COUNTER.store(0, Ordering::Relaxed);

        let ep1 = select_endpoint(&endpoints).unwrap();
        assert_eq!(ep1.url, "https://api1.example.com");

        let ep2 = select_endpoint(&endpoints).unwrap();
        assert_eq!(ep2.url, "https://api2.example.com");

        let ep3 = select_endpoint(&endpoints).unwrap();
        assert_eq!(ep3.url, "https://api3.example.com");

        // 循环回第一个
        let ep4 = select_endpoint(&endpoints).unwrap();
        assert_eq!(ep4.url, "https://api1.example.com");
    }

    #[test]
    fn test_select_endpoint_empty() {
        let endpoints: Vec<AiEndpoint> = vec![];
        assert!(select_endpoint(&endpoints).is_none());
    }

    #[test]
    fn test_ai_extract_result_deserialize() {
        let json = r#"{"title":"标题","url":["https://example.com"],"description":"描述","category":"quark","tags":"电影,动作"}"#;
        let result: AiExtractResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "标题");
        assert_eq!(result.url.len(), 1);
        assert_eq!(result.category, "quark");
    }
}
