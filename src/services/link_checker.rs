//! 可插拔链接检测器抽象（spec.md Q2）+ PanCheck 实现。
//!
//! 当前唯一实现：`PanCheckChecker`。未来可追加新的检测器实现而不改架构。
//! 使用 edition 2024 原生 async-fn-in-trait（脱糖为 `impl Future + Send`），无需 async-trait crate。
//!
//! 设计要点：HTTP 调用与响应解析分离 —— `parse_pancheck_response` 为纯函数，可脱离
//! 网络单测；`PanCheckChecker::check` 仅做 HTTP + 错误降级。

use crate::errors::AppError;
use std::future::Future;
use std::pin::Pin;

/// 单条 URL 的检测结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LinkStatus {
    /// PanCheck 判定有效
    Valid,
    /// PanCheck 判定失效
    Invalid,
    /// PanCheck 待检测（频率限制）
    Pending,
    /// 不可达/超时/未覆盖平台 → 视为「未检测」，不阻塞推送
    Unknown,
}

impl LinkStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LinkStatus::Valid => "valid",
            LinkStatus::Invalid => "invalid",
            LinkStatus::Pending => "pending",
            LinkStatus::Unknown => "unknown",
        }
    }
}

/// 单条 URL 的检测判定结果。
#[derive(Debug, Clone)]
pub struct LinkVerdict {
    pub url: String,
    pub status: LinkStatus,
    pub platform: Option<String>,
    pub fail_reason: Option<String>,
}

/// 可插拔链接检测器抽象。
///
/// 实现约定：
/// - `check` 接收一批**归一化** URL，返回每条 URL 的判定。
/// - 网络/超时/解析失败**不应**返回 `Err` 中断整批——应将受影响 URL 标 `Unknown` 返回；
///   仅当无法构造请求（如 host 未配置）才返回 `Err`。
/// - 单批同步语义（内部一次 HTTP 调用）；分块与并发由上层 `link_check::check_urls` 控制。
pub trait LinkChecker: Send + Sync {
    /// 检测一批归一化 URL。返回 boxed future（对象安全，支持 `Box<dyn LinkChecker>`
    /// 动态分发与运行时切换，见 `resolve_checker` 工厂）。
    fn check<'a>(
        &'a self,
        urls: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LinkVerdict>, AppError>> + Send + 'a>>;
}

// ─── PanCheck 实现 ────────────────────────────────────────────────────────────

/// PanCheck 支持的平台（FR-005 全选）。`mobile`=移动云盘，实际字符串以 T001 核实为准。
const PANCHECK_PLATFORMS: &[&str] = &[
    "quark", "uc", "baidu", "tianyi", "123pan", "115", "aliyun", "xunlei", "mobile",
];

/// PanCheck 链接检测器。
#[derive(Clone)]
pub struct PanCheckChecker {
    client: reqwest::Client,
    host: String,
}

impl PanCheckChecker {
    /// 构造检测器。单请求超时 60 秒（PanCheck 同步检测可能较慢）。
    pub fn new(host: &str) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| AppError::Internal(format!("创建 PanCheck HTTP 客户端失败: {e}")))?;
        Ok(Self {
            client,
            host: host.trim_end_matches('/').to_string(),
        })
    }
}

/// 把一组 URL 全部降级为 Unknown（用于 HTTP 失败/非 2xx 场景，FR-009 非阻塞）。
fn all_unknown(urls: &[String]) -> Vec<LinkVerdict> {
    urls.iter()
        .map(|u| LinkVerdict {
            url: u.clone(),
            status: LinkStatus::Unknown,
            platform: None,
            fail_reason: None,
        })
        .collect()
}

impl LinkChecker for PanCheckChecker {
    fn check<'a>(
        &'a self,
        urls: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<LinkVerdict>, AppError>> + Send + 'a>> {
        Box::pin(async move {
            if urls.is_empty() {
                return Ok(Vec::new());
            }
            let endpoint = format!("{}/api/v1/links/check", self.host);
            let body = serde_json::json!({
                "links": urls,
                "selected_platforms": PANCHECK_PLATFORMS,
            });
            tracing::info!("PanCheck 请求: endpoint={endpoint}, urls={urls:?}");
            match self.client.post(&endpoint).json(&body).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    tracing::info!("PanCheck 响应: status={status}, body={text}");
                    if !status.is_success() {
                        tracing::warn!("PanCheck 返回非 2xx: status={status}, body={text}");
                        return Ok(all_unknown(urls));
                    }
                    Ok(parse_pancheck_response(&text, urls))
                }
                Err(e) => {
                    tracing::warn!("PanCheck 调用失败，整批降级为 Unknown: {e}");
                    Ok(all_unknown(urls))
                }
            }
        })
    }
}

/// 根据配置 `link_checker_type` 解析检测器实例（工厂，无缝切换入口）。
/// - 默认 / `pancheck`：读取 `pancheck_host`，非空返回 PanCheck 检测器；空则未启用
/// - 未知类型 → 未启用（warn）
/// - **新增检测器**：实现 `LinkChecker` 后，在此 match 增加一个分支即可；
///   **切换**：后台改 `link_checker_type` 即生效（零代码、零重新部署逻辑）。
pub async fn resolve_checker(
    option_cache: &crate::state::OptionCache,
) -> Result<Option<Box<dyn LinkChecker>>, AppError> {
    let (kind, pancheck_host) = {
        let cache = option_cache.read().await;
        (
            cache.get("link_checker_type").cloned().unwrap_or_default(),
            cache.get("pancheck_host").cloned().unwrap_or_default(),
        )
    };
    match kind.as_str() {
        "pancheck" | "" => {
            if pancheck_host.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(Box::new(PanCheckChecker::new(&pancheck_host)?)))
            }
        }
        other => {
            tracing::warn!("未知 link_checker_type={other}，链接检测未启用");
            Ok(None)
        }
    }
}

// ─── 响应解析（纯函数，单测友好） ─────────────────────────────────────────────

/// 从对象中按候选键名取字符串值。
fn pick_str<'a>(obj: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_str()))
}

/// 从一个 JSON 数组中提取 (url, platform, reason) 三元组列表。
/// 支持两种格式：
/// - 字符串数组：`["url1", "url2"]`（PanCheck 实际格式）
/// - 对象数组：`[{"url":"...", "platform":"...", "reason":"..."}]`（预留兼容）
fn extract_array(arr: &serde_json::Value) -> Vec<(String, Option<String>, Option<String>)> {
    arr.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    // 字符串格式：直接是 URL
                    if let Some(url) = it.as_str() {
                        return Some((url.to_string(), None, None));
                    }
                    // 对象格式：按候选键提取
                    let url = pick_str(it, &["url", "link"])?.to_string();
                    let platform =
                        pick_str(it, &["platform", "service", "type"]).map(str::to_string);
                    let reason = pick_str(it, &["reason", "message", "msg"]).map(str::to_string);
                    Some((url, platform, reason))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 在响应对象中按候选键名找到首个数组字段并提取其元素。
fn find_array(
    obj: &serde_json::Value,
    keys: &[&str],
) -> Vec<(String, Option<String>, Option<String>)> {
    for k in keys {
        if let Some(arr) = obj.get(*k)
            && arr.is_array()
        {
            return extract_array(arr);
        }
    }
    Vec::new()
}

/// 解析 PanCheck 响应：按 valid/invalid/pending 三组数组（字段名容错）归类每个请求 URL。
/// 未出现在任何数组中的 URL → Unknown。响应非 JSON → 全部 Unknown。
///
/// 字段名容错映射见 `contracts/pancheck-api.md` §3（实现期以 T001 真实字段名为准）。
pub(crate) fn parse_pancheck_response(body: &str, requested: &[String]) -> Vec<LinkVerdict> {
    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("PanCheck 响应非有效 JSON，整批降级 Unknown: {e}");
            return all_unknown(requested);
        }
    };
    let valid = find_array(&v, &["valid_links", "available", "ok", "valid"]);
    let invalid = find_array(
        &v,
        &["invalid_links", "unavailable", "expired", "dead", "invalid"],
    );
    let pending = find_array(&v, &["pending_links", "pending", "checking"]);

    requested
        .iter()
        .map(|u| {
            let nu = crate::services::link_check::normalize_url(u);
            // invalid 优先（任一失效即标记）
            if let Some((_, platform, reason)) = invalid
                .iter()
                .find(|(url, _, _)| crate::services::link_check::normalize_url(url) == nu)
            {
                return LinkVerdict {
                    url: u.clone(),
                    status: LinkStatus::Invalid,
                    platform: platform.clone(),
                    fail_reason: reason.clone(),
                };
            }
            if let Some((_, platform, _)) = valid
                .iter()
                .find(|(url, _, _)| crate::services::link_check::normalize_url(url) == nu)
            {
                return LinkVerdict {
                    url: u.clone(),
                    status: LinkStatus::Valid,
                    platform: platform.clone(),
                    fail_reason: None,
                };
            }
            if pending
                .iter()
                .any(|(url, _, _)| crate::services::link_check::normalize_url(url) == nu)
            {
                return LinkVerdict {
                    url: u.clone(),
                    status: LinkStatus::Pending,
                    platform: None,
                    fail_reason: None,
                };
            }
            LinkVerdict {
                url: u.clone(),
                status: LinkStatus::Unknown,
                platform: None,
                fail_reason: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(i: usize) -> String {
        format!("https://pan.quark.cn/s/abc{i}")
    }

    #[test]
    fn test_parse_canonical_field_names() {
        // PanCheck 实际格式：字符串数组
        let body = r#"{
            "valid_links":   ["https://pan.quark.cn/s/abc1"],
            "invalid_links": ["https://pan.quark.cn/s/abc2"],
            "pending_links": ["https://pan.quark.cn/s/abc3"]
        }"#;
        let req = vec![url(1), url(2), url(3), url(4)];
        let v = parse_pancheck_response(body, &req);
        assert_eq!(v[0].status, LinkStatus::Valid);
        assert_eq!(v[1].status, LinkStatus::Invalid);
        assert_eq!(v[2].status, LinkStatus::Pending);
        assert_eq!(v[3].status, LinkStatus::Unknown); // 未出现在任何数组
    }

    #[test]
    fn test_parse_object_array_format() {
        // 对象数组格式（兼容）
        let body = r#"{
            "valid_links":   [{"url":"https://pan.quark.cn/s/abc1","platform":"quark"}],
            "invalid_links": [{"url":"https://pan.quark.cn/s/abc2","platform":"baidu","reason":"分享已失效"}],
            "pending_links": [{"url":"https://pan.quark.cn/s/abc3","platform":"115"}]
        }"#;
        let req = vec![url(1), url(2), url(3)];
        let v = parse_pancheck_response(body, &req);
        assert_eq!(v[0].status, LinkStatus::Valid);
        assert_eq!(v[0].platform.as_deref(), Some("quark"));
        assert_eq!(v[1].status, LinkStatus::Invalid);
        assert_eq!(v[1].fail_reason.as_deref(), Some("分享已失效"));
        assert_eq!(v[2].status, LinkStatus::Pending);
    }

    #[test]
    fn test_parse_variant_field_names() {
        // available / dead / pending（候选键名容错）— 对象数组格式
        let body = r#"{
            "available": [{"link":"https://pan.quark.cn/s/abc1","service":"quark"}],
            "dead":      [{"link":"https://pan.quark.cn/s/abc2","type":"baidu","msg":"失效"}],
            "pending":   [{"link":"https://pan.quark.cn/s/abc3"}]
        }"#;
        let req = vec![url(1), url(2), url(3)];
        let v = parse_pancheck_response(body, &req);
        assert_eq!(v[0].status, LinkStatus::Valid);
        assert_eq!(v[1].status, LinkStatus::Invalid);
        assert_eq!(v[1].fail_reason.as_deref(), Some("失效"));
        assert_eq!(v[2].status, LinkStatus::Pending);
    }

    #[test]
    fn test_parse_malformed_json_all_unknown() {
        let req = vec![url(1), url(2)];
        let v = parse_pancheck_response("not json", &req);
        assert!(v.iter().all(|x| x.status == LinkStatus::Unknown));
    }

    #[test]
    fn test_parse_empty_arrays_all_unknown() {
        let body = r#"{"valid_links":[],"invalid_links":[],"pending_links":[]}"#;
        let req = vec![url(1)];
        let v = parse_pancheck_response(body, &req);
        assert_eq!(v[0].status, LinkStatus::Unknown);
    }

    #[test]
    fn test_parse_invalid_takes_priority() {
        // 同一 URL 同时在 valid 与 invalid（异常但 defensive）→ 判定 invalid
        let body = r#"{
            "valid_links":   ["https://pan.quark.cn/s/abc1"],
            "invalid_links": ["https://pan.quark.cn/s/abc1"]
        }"#;
        let req = vec![url(1)];
        let v = parse_pancheck_response(body, &req);
        assert_eq!(v[0].status, LinkStatus::Invalid);
    }

    #[test]
    fn test_all_unknown_helper() {
        let urls = vec!["a".to_string(), "b".to_string()];
        let v = all_unknown(&urls);
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|x| x.status == LinkStatus::Unknown));
    }

    // --- 工厂 resolve_checker（无缝切换入口） ---

    fn empty_cache() -> crate::state::OptionCache {
        std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::<String, String>::new(),
        ))
    }

    #[tokio::test]
    async fn test_resolve_checker_none_when_unconfigured() {
        let cache = empty_cache();
        let c = resolve_checker(&cache).await.unwrap();
        assert!(c.is_none(), "未配置 host 应返回 None（未启用）");
    }

    #[tokio::test]
    async fn test_resolve_checker_pancheck_when_host_set() {
        let cache = empty_cache();
        {
            let mut m = cache.write().await;
            m.insert("pancheck_host".into(), "http://pancheck:6080".into());
        }
        let c = resolve_checker(&cache).await.unwrap();
        assert!(c.is_some(), "配置 host 后应返回 PanCheck 检测器");
    }

    #[tokio::test]
    async fn test_resolve_checker_unknown_type_returns_none() {
        let cache = empty_cache();
        {
            let mut m = cache.write().await;
            m.insert("link_checker_type".into(), "mystery".into());
            m.insert("pancheck_host".into(), "http://pancheck:6080".into());
        }
        let c = resolve_checker(&cache).await.unwrap();
        assert!(c.is_none(), "未知 type 应返回 None（回退未启用）");
    }
}
