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
const DEFAULT_PROMPT: &str = r#"你是一个专业的 Telegram 消息资源提取助手。请从以下 Telegram 消息中提取结构化资源信息，返回严格的 JSON 格式。

## 字段说明
- "title": 资源名称标题，不包含链接、标签或特殊符号
- "url": 网盘分享链接数组，只保留有效的网盘链接（忽略 t.me 等广告链接）
- "description": 资源描述或亮点，从"描述："、"亮点："、"简介："等关键词提取，没有则留空
- "category": 网盘类型，必须为以下之一：quark、aliyun、baidu、uc、115、123pan、tianyi、xunlei，无法识别则留空
- "tags": 标签，逗号分隔，最多5个，去除#前缀

## 处理规则
- 忽略 t.me 开头的广告推广链接
- 如果消息中出现"名称："、"标题："、"资源名称："等关键词，提取其后的内容作为 title
- 如果消息中没有明确标题，从内容中推断一个简短描述性的标题
- 处理中英文混合和格式混乱的消息

## 输出示例
{"title":"某部电影 4K 蓝光版","url":["https://pan.quark.cn/s/abc123"],"description":"4K 蓝光高清版本，中英双字","category":"quark","tags":"电影,4K,蓝光"}

只返回 JSON，不要包含任何其他文字。

消息内容：
"#;

/// 调用 OpenAI 兼容 API 进行提取
pub async fn call_ai_api(
    endpoint: &AiEndpoint,
    prompt: &str,
    message: &str,
    proxy_url: Option<&str>,
) -> Result<AiExtractResult, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .no_proxy(); // 默认禁用系统代理，避免被 Windows 系统代理干扰

    if let Some(proxy) = proxy_url {
        if !proxy.is_empty() {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }
    }

    let client = builder
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

    let (prompt, proxy_enabled, proxy_url) = {
        let cache = option_cache.read().await;
        let p = cache.get("push_ai_prompt").cloned().unwrap_or_default();
        let enabled = cache
            .get("push_ai_use_proxy")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
        let proxy = cache.get("proxy_url").cloned().unwrap_or_default();
        (p, enabled, proxy)
    };

    // 尝试轮询选择端点，故障自动切换到下一个
    let mut last_error = String::new();
    for _ in 0..endpoints.len().min(3) {
        let endpoint = match select_endpoint(&endpoints) {
            Some(ep) => ep.clone(),
            None => break,
        };

        let proxy_arg = if proxy_enabled && !proxy_url.is_empty() {
            Some(proxy_url.as_str())
        } else {
            None
        };
        match call_ai_api(&endpoint, &prompt, raw_data, proxy_arg).await {
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
                id: String::new(),
                name: String::new(),
                ai_type: default_ai_type(),
                url: "https://api1.example.com".to_string(),
                key: "key1".to_string(),
                model: "model1".to_string(),
                enable: true,
                request_delay: 0,
            },
            AiEndpoint {
                id: String::new(),
                name: String::new(),
                ai_type: default_ai_type(),
                url: "https://api2.example.com".to_string(),
                key: "key2".to_string(),
                model: "model2".to_string(),
                enable: true,
                request_delay: 0,
            },
            AiEndpoint {
                id: String::new(),
                name: String::new(),
                ai_type: default_ai_type(),
                url: "https://api3.example.com".to_string(),
                key: "key3".to_string(),
                model: "model3".to_string(),
                enable: true,
                request_delay: 0,
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

    // --- T007: 默认提示词包含字段约束 ---

    #[test]
    fn test_default_prompt_contains_field_constraints() {
        // 验证 DEFAULT_PROMPT 包含每个字段的约束描述
        assert!(
            DEFAULT_PROMPT.contains("title"),
            "提示词应包含 title 字段约束"
        );
        assert!(DEFAULT_PROMPT.contains("url"), "提示词应包含 url 字段约束");
        assert!(
            DEFAULT_PROMPT.contains("description"),
            "提示词应包含 description 字段约束"
        );
        assert!(
            DEFAULT_PROMPT.contains("category"),
            "提示词应包含 category 字段约束"
        );
        assert!(
            DEFAULT_PROMPT.contains("tags"),
            "提示词应包含 tags 字段约束"
        );
    }

    // --- T008: 默认提示词包含 JSON 输出示例 ---

    #[test]
    fn test_default_prompt_contains_example() {
        // 验证 DEFAULT_PROMPT 包含 JSON 输出示例（包含示例字段值）
        assert!(
            DEFAULT_PROMPT.contains("quark") || DEFAULT_PROMPT.contains("aliyun"),
            "提示词应包含网盘类型示例"
        );
        assert!(
            DEFAULT_PROMPT.contains("pan.quark") || DEFAULT_PROMPT.contains("example"),
            "提示词应包含链接示例"
        );
    }

    // --- T017: 嵌套花括号 JSON 解析 ---

    #[test]
    fn test_extract_json_from_content_nested_braces() {
        let content = r#"AI 返回结果如下：{"title":"测试资源","url":["https://pan.quark.cn/s/abc"],"description":"包含嵌套{括号}的描述","category":"quark","tags":"测试"}"#;
        let json_str = extract_json_from_content(content);
        let result: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(result["title"], "测试资源");
        assert_eq!(result["category"], "quark");
    }

    // --- T018: AI 部分字段回退合并 ---

    #[test]
    fn test_ai_extract_partial_field_fallback() {
        // 模拟 AI 返回部分字段为空
        let ai_json =
            r#"{"title":"AI标题","url":[],"description":"","category":"quark","tags":""}"#;
        let ai_result: AiExtractResult = serde_json::from_str(ai_json).unwrap();

        let rule_draft = ResourceDraft {
            title: "规则标题".to_string(),
            url: vec!["https://pan.quark.cn/s/abc".to_string()],
            description: "规则描述".to_string(),
            category: "quark".to_string(),
            tags: "电影".to_string(),
            source: "tg".to_string(),
        };

        // 合并逻辑：空字段回退到规则结果
        let merged = ResourceDraft {
            title: if ai_result.title.is_empty() {
                rule_draft.title.clone()
            } else {
                ai_result.title
            },
            url: if ai_result.url.is_empty() {
                rule_draft.url.clone()
            } else {
                ai_result.url
            },
            description: if ai_result.description.is_empty() {
                rule_draft.description.clone()
            } else {
                ai_result.description
            },
            category: if ai_result.category.is_empty() {
                rule_draft.category.clone()
            } else {
                ai_result.category
            },
            tags: if ai_result.tags.is_empty() {
                rule_draft.tags.clone()
            } else {
                ai_result.tags
            },
            source: "tg".to_string(),
        };

        assert_eq!(merged.title, "AI标题"); // AI 有值，用 AI
        assert_eq!(merged.url, vec!["https://pan.quark.cn/s/abc"]); // AI 为空，回退规则
        assert_eq!(merged.description, "规则描述"); // AI 为空，回退规则
        assert_eq!(merged.tags, "电影"); // AI 为空，回退规则
    }

    // --- T019: 端点过滤 disabled ---

    #[test]
    fn test_parse_ai_endpoints_with_disabled() {
        let endpoints_json = r#"[
            {"url":"https://api1.example.com","key":"sk-1","model":"gpt-4o","enable":true},
            {"url":"https://api2.example.com","key":"sk-2","model":"gpt-4o","enable":false},
            {"url":"https://api3.example.com","key":"sk-3","model":"gpt-4o"}
        ]"#;
        let endpoints: Vec<AiEndpoint> = serde_json::from_str(endpoints_json).unwrap();
        assert_eq!(endpoints.len(), 3);

        // select_endpoint 只选择 enable=true 的端点
        let selected = select_endpoint(&endpoints);
        assert!(selected.is_some());
        let ep = selected.unwrap();
        assert_ne!(ep.url, "https://api2.example.com"); // 不应选中 disabled 的
    }
}
