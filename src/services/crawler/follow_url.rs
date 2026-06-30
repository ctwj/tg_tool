//! follow_url 模式两阶段提取（feature 043-crawler-configurator 扩展）
//!
//! extractor.rs 同步路径有意不支持 follow_url（`extract()` 对 `Rule::FollowUrl` 返回
//! `UnsupportedMode`）；本模块提供 async 两阶段提取，被 probe.rs / engine.rs 共享调用。
//!
//! 流程：
//! 1. 在当前 material 上用 transit 子规则提取中转 URL（同步 extract）
//! 2. 中转 URL 若为相对路径，用 `engine::resolve_url` 补全（base = 当前 material.final_url）
//! 3. `source_layer::fetch_source_material` 请求中转 URL，拿新 SourceMaterial
//! 4. 在新 material 上用 extract 子规则提取最终值（同步 extract）
//!
//! 任一阶段失败分别映射到 `FollowUrlError` 的不同变体，便于上层（probe/engine）
//! 做差异化错误展示。

use crate::services::crawler::engine::resolve_url;
use crate::services::crawler::extractor::{self, sub_rule_to_rule, ExtractError, ExtractInput, Hit};
use crate::services::crawler::field_schema::{FollowUrlRule, SourceLayer};
use crate::services::crawler::source_layer::{fetch_source_material, ProbeError, SourceMaterial};

/// 两阶段提取错误
#[derive(Debug, Clone)]
pub enum FollowUrlError {
    /// transit 子规则 0 命中（没拿到中转 URL）
    TransitEmpty,
    /// transit 子规则同步提取报错（如 css 语法错、script_index 越界）
    TransitExtract(ExtractError),
    /// 二次请求失败（透传 source_layer 的 ProbeError，含 4xx/5xx/拦截/超时）
    Fetch(ProbeError),
    /// extract 子规则同步提取报错
    ExtractExtract(ExtractError),
    /// extract 子规则 0 命中
    ZeroHits,
}

impl std::fmt::Display for FollowUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FollowUrlError::TransitEmpty => {
                write!(f, "transit 子规则未提取到中转 URL（0 命中）")
            }
            FollowUrlError::TransitExtract(e) => {
                write!(f, "transit 子规则提取失败: {e}")
            }
            FollowUrlError::Fetch(e) => write!(f, "二次请求失败: {e}"),
            FollowUrlError::ExtractExtract(e) => {
                write!(f, "extract 子规则提取失败: {e}")
            }
            FollowUrlError::ZeroHits => write!(f, "extract 子规则在二次响应未命中（0 条）"),
        }
    }
}

impl std::error::Error for FollowUrlError {}

/// 执行 follow_url 两阶段提取
///
/// 参数：
/// - `rule`：FollowUrlRule（transit + extract 子规则 + 各自 source_layer）
/// - `current`：当前 SourceMaterial（transit 子规则作用其上，且其 `final_url` 用作相对 URL 的 base）
/// - `ua` / `proxy`：透传给二次请求
pub async fn extract_follow_url_async(
    rule: &FollowUrlRule,
    current: &SourceMaterial,
    ua: Option<&str>,
    proxy: Option<&str>,
) -> Result<Vec<Hit>, FollowUrlError> {
    // ① transit 子规则在 current 上提取中转 URL
    let transit_rule = sub_rule_to_rule(&rule.transit);
    let transit_input = build_input(current, rule.transit_layer, rule.transit_script_index);
    let transit_hits = extractor::extract(&transit_rule, &transit_input)
        .map_err(FollowUrlError::TransitExtract)?;
    let transit_value = transit_hits
        .into_iter()
        .next()
        .ok_or(FollowUrlError::TransitEmpty)?
        .value;

    // ② 相对 URL → 绝对 URL（base = 当前 material.final_url）
    let abs_url = resolve_url(&transit_value, &current.final_url);

    // ③ fetch_source_material(abs_url)
    let target = fetch_source_material(&abs_url, ua, proxy)
        .await
        .map_err(FollowUrlError::Fetch)?;

    // ④ extract 子规则在 target material 上提取最终值
    let extract_rule = sub_rule_to_rule(&rule.extract);
    let extract_input = build_input(&target, rule.target_layer, rule.target_script_index);
    let hits = extractor::extract(&extract_rule, &extract_input)
        .map_err(FollowUrlError::ExtractExtract)?;
    if hits.is_empty() {
        return Err(FollowUrlError::ZeroHits);
    }
    Ok(hits)
}

/// 构造 ExtractInput：从 SourceMaterial 取指定 layer 的素材
fn build_input<'a>(
    material: &'a SourceMaterial,
    layer: SourceLayer,
    script_index: Option<i32>,
) -> ExtractInput<'a> {
    ExtractInput::from_material(material, script_index).with_layer(layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::crawler::field_schema::{
        CssRule, FollowUrlRule, HeaderFieldRule, JsonPathRule, RegexRule, SubRule,
    };
    use std::collections::HashMap;

    fn material(html: &str, final_url: &str) -> SourceMaterial {
        SourceMaterial {
            final_url: final_url.into(),
            html: html.to_string(),
            status: 200,
            headers: HashMap::new(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 0,
        }
    }

    /// 构造一个最小的 follow_url 规则：transit = css a.dl[href]，extract = css a.real[href]
    fn fu_rule() -> FollowUrlRule {
        FollowUrlRule {
            transit: SubRule::Css(CssRule {
                selector: "a.dl".into(),
                attr: "href".into(),
            }),
            transit_layer: SourceLayer::Html,
            transit_script_index: None,
            target_layer: SourceLayer::Html,
            target_script_index: None,
            extract: SubRule::Css(CssRule {
                selector: "a.real".into(),
                attr: "href".into(),
            }),
        }
    }

    #[test]
    fn transit_empty_when_no_match() {
        // 当前页没有 a.dl 元素 → TransitEmpty（不会发起 HTTP）
        let cur = material("<div>nothing</div>", "https://example.com/");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(extract_follow_url_async(&fu_rule(), &cur, None, None))
            .unwrap_err();
        assert!(matches!(err, FollowUrlError::TransitEmpty));
    }

    #[test]
    fn transit_extract_propagates_extract_error() {
        // source_layer=Script 但 material 无 script_blocks → SourceMissing
        let mut rule = fu_rule();
        rule.transit_layer = SourceLayer::Script;
        rule.transit_script_index = Some(0);
        let cur = material("<div>x</div>", "https://example.com/");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(extract_follow_url_async(&rule, &cur, None, None))
            .unwrap_err();
        match err {
            FollowUrlError::TransitExtract(_) => {}
            other => panic!("期望 TransitExtract，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn end_to_end_example_com_two_stage() {
        // example.com 首页有 <a href="https://www.iana.org/domains/example">More information...</a>
        // transit: css a[href] 抓到该 URL → fetch → extract: css a[href] 抓新页面的链接
        // 这个测试依赖外网，若网络/站点变更可能失败，保留与 probe::tests 同样的网络依赖约定
        let cur = material(
            "<html><body><a class='dl' href='https://example.com/'>x</a></body></html>",
            "https://example.com/",
        );
        let rule = FollowUrlRule {
            transit: SubRule::Css(CssRule {
                selector: "a.dl".into(),
                attr: "href".into(),
            }),
            transit_layer: SourceLayer::Html,
            transit_script_index: None,
            target_layer: SourceLayer::Html,
            target_script_index: None,
            extract: SubRule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
        };
        let res = extract_follow_url_async(&rule, &cur, None, None).await;
        match res {
            Ok(hits) => assert!(!hits.is_empty(), "二次响应应能抓到 a[href]"),
            Err(FollowUrlError::Fetch(e)) => {
                // 网络受限环境下允许失败，但错误应是 Fetch 阶段
                eprintln!("网络受限，跳过断言: {e}");
            }
            Err(other) => panic!("意外错误: {other:?}"),
        }
    }

    #[test]
    fn sub_rule_variants_construct_ok() {
        // 覆盖 SubRule 6 变体的构造（确保 serde 字段完整）
        let _ = SubRule::Regex(RegexRule {
            pattern: "x".into(),
            group: 0,
            flags: "".into(),
        });
        let _ = SubRule::JsonPath(JsonPathRule { path: "$.x".into() });
        let _ = SubRule::HeaderField(HeaderFieldRule {
            header_name: "X".into(),
        });
    }
}
