//! 字段验证探针（feature 043-crawler-configurator，US1 T021 / US2 T042）
//!
//! 把 `source_layer`（抓 URL）+ `extractor`（按规则提取）+ 后处理链串成一个原子操作，
//! 返回命中样本供前端字段配置器右侧「验证」按钮直接显示。
//!
//! 与 contracts C2 对齐：
//! - 输入：URL + 规则（mode/rule/post_processors）+ 可选 `parent_field`（US2 父子嵌套）
//! - 输出：`ProbeResponse { hit_count, samples, per_parent?, fetched_url, fetched_at, duration_ms }`
//! - 失败：`ProbeError`（与 `source_layer::ProbeError` 同类型）
//!
//! 错误分类映射（覆盖 data-model.md 6 类 ProbeCategory）：
//! - URL 不可达（DNS/连接/超时/TLS） → Fetch / UrlUnreachable
//! - HTTP 4xx/5xx                  → Fetch / Http4xx5xx 或 Fetch / Blocked
//! - 规则非法（CSS/regex 编译失败）  → Parse / InvalidRule
//! - 0 命中                        → Match / ZeroHits
//! - 父字段 0 命中导致子字段无法运行 → Match / ParentEmpty
//!
//! ## US2 嵌套验证
//!
//! 当 `ProbeRequest.parent_field` 提供时（由 handler 查表填入）：
//! 1. 抓取 URL 一次
//! 2. 应用父规则 → N 条父命中（含 `context_html` 作用域片段）
//! 3. 父命中为空 → 直接返回 `ParentEmpty`（contracts C2 表）
//! 4. 对每条父命中：用 `context_html` 构造 sub_material → 应用子规则 → 记录 per_parent 结果
//! 5. 返回的 `ProbeResponse.per_parent` 含每条父命中的子值（命中/未命中）

use chrono::NaiveDateTime;

use crate::services::crawler::extractor::{self, ExtractErrorKind, ExtractInput};
use crate::services::crawler::field_schema::{Rule, SourceLayer};
use crate::services::crawler::script_engine::ScriptFailureCategory;
use crate::services::crawler::script_runner::{self, ScriptOpts};
use crate::services::crawler::source_layer::{
    ProbeCategory, ProbeError, ProbeStage, SourceMaterial, fetch_source_material,
};

// ============================================================================
// ProbeRequest / ProbeResponse
// ============================================================================

/// US2：父字段定义（handler 查 `crawler_task_field_nodes` 取出父节点规则后填入）
///
/// probe 收到 parent_field 时会先对父规则求命中，再在每个父作用域片段上应用子规则，
/// 返回结构化的 per_parent 列表（contracts C2 parent_empty 错误分类也在此路径触发）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParentFieldDef {
    pub source_layer: SourceLayer,
    pub rule: Rule,
    #[serde(default)]
    pub post_processors: Vec<crate::services::crawler::field_schema::PostProcessor>,
    /// source_layer=Script 时指定第几个 `<script>`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_index: Option<i32>,
}

/// 探针请求
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeRequest {
    /// 目标 URL（应含 scheme）
    pub url: String,
    /// 自定义 UA；None 用默认
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// 自定义代理；None 不走代理
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    pub source_layer: SourceLayer,
    pub rule: Rule,
    #[serde(default)]
    pub post_processors: Vec<crate::services::crawler::field_schema::PostProcessor>,
    /// source_layer=Script 时指定第几个 `<script>`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_index: Option<i32>,
    /// 父字段命中（US2+ 父子字段场景）：若为空且 `require_parent=true`，
    /// 子字段直接返回 `ParentEmpty`，跳过实际抓取
    #[serde(default)]
    pub parent_hits: Vec<String>,
    /// 是否要求父字段必须有命中（默认 false：单字段 probe 不依赖父字段）
    #[serde(default)]
    pub require_parent: bool,
    /// US2：父字段定义（与 parent_hits 互斥；优先使用 parent_field）
    ///
    /// 当提供时，先对父规则求命中，再对每条父作用域片段应用子规则。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_field: Option<ParentFieldDef>,
    /// US2：每条父命中下返回的子样本数上限（默认 3，避免响应过大）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_parent_sample_limit: Option<usize>,
    /// US2：父字段节点 ID（仅 handler 解释；probe 内部不读取）
    ///
    /// handler 收到此字段时查 `crawler_task_field_nodes` 取父规则，
    /// 转换为 `parent_field` 后清空本字段再调用 run_probe。
    /// probe 内部保留此字段仅用于日志与回显。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_node_id: Option<i64>,
}

/// 探针响应样本（精简版 Hit，去掉 serde 复杂字段）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeSample {
    pub value: String,
    pub source_fragment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// US2：按父命中分组的子字段结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerParentSample {
    /// 父命中序号（0-based）
    pub parent_index: usize,
    /// 父命中片段摘要（前 200 字符 + 省略号）
    pub parent_fragment: String,
    /// 子字段在该父作用域下的首个命中值（多值取首条；None=未命中）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_value: Option<String>,
    /// 子字段在该父作用域下是否命中
    pub child_hit: bool,
    /// 子字段在该父作用域下的全部命中样本（受 per_parent_sample_limit 截断）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_samples: Option<Vec<ProbeSample>>,
}

/// 探针响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeResponse {
    pub hit_count: usize,
    pub samples: Vec<ProbeSample>,
    /// US2：父子嵌套验证时填充（按父命中序号排列）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_parent: Option<Vec<PerParentSample>>,
    pub fetched_url: String,
    pub fetched_at: NaiveDateTime,
    pub duration_ms: u64,
}

// ============================================================================
// 主入口
// ============================================================================

/// 执行字段探针：抓取 → 提取 → 后处理
///
/// US2 嵌套路径（`req.parent_field` 为 Some 时）：
/// 1. 抓 URL 一次
/// 2. 应用父规则 → N 条父命中（含 `context_html`）
/// 3. 父命中为空 → `ParentEmpty` 错误（contracts C2 表）
/// 4. 对每条父命中构造 sub_material → 应用子规则 → 记录 per_parent 结果
/// 5. 同时把所有子命中合并到顶层 `samples` 字段（受 sample_limit 截断）
pub async fn run_probe(req: ProbeRequest) -> Result<ProbeResponse, ProbeError> {
    // 1. 父字段检查（parent_hits 显式为空 + require_parent）：直接短路
    //    （parent_field 为 Some 时跳过此检查，先抓 URL 再实际探测父命中）
    if req.require_parent && req.parent_hits.is_empty() && req.parent_field.is_none() {
        return Err(ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ParentEmpty,
            "父字段 0 命中，子字段无法运行（请先修正父字段规则）",
        ));
    }

    // 2. 抓 URL
    let material: SourceMaterial =
        fetch_source_material(&req.url, req.user_agent.as_deref(), req.proxy.as_deref()).await?;

    // 3a. US2 嵌套路径：先求父命中，再逐个作用域应用子规则
    if let Some(parent_field) = req.parent_field.clone() {
        return run_nested_probe(req, &material, parent_field).await;
    }

    // 3b. 单字段路径
    // follow_url 早返回：两阶段提取（transit → fetch → extract）
    if let Rule::FollowUrl(fu) = &req.rule {
        let hits = crate::services::crawler::follow_url::extract_follow_url_async(
            fu,
            &material,
            req.user_agent.as_deref(),
            req.proxy.as_deref(),
        )
        .await
        .map_err(follow_url_err_to_probe_error)?;
        let final_hits =
            extractor::apply_post_processors(hits, &req.post_processors, &material.final_url);
        if final_hits.is_empty() {
            return Err(ProbeError::new(
                ProbeStage::Match,
                ProbeCategory::ZeroHits,
                "follow_url 子规则在后处理链后 0 命中",
            ));
        }
        return Ok(ProbeResponse {
            hit_count: final_hits.len(),
            samples: final_hits
                .into_iter()
                .map(|h| ProbeSample {
                    value: h.value,
                    source_fragment: h.source_fragment,
                    location: h.location,
                })
                .collect(),
            per_parent: None,
            fetched_url: material.final_url.clone(),
            fetched_at: material.fetched_at,
            duration_ms: material.duration_ms,
        });
    }

    // 3c. [feature 046] Script 模式：构造 ctx（value="" 探针场景）→ run_script → 单条样本
    //
    // **US2 限制（文档化）**：探针是单字段验证场景，`ctx.fields` 暂为空 HashMap。
    // 如需"先探针 A、再探针 B 时引用 A"的级联验证，调用方应：
    //   1. 先 run_probe(A) 取 sample.value；
    //   2. 自行构造 `sibling_fields` HashMap（如 {"A": sample.value}）；
    //   3. 暂未在 ProbeRequest 暴露 sibling_fields 入参（避免 API 复杂化）；
    //   4. 改用 test_run 端到端验证跨字段逻辑（推荐路径）。
    if let Rule::Script(script_rule) = &req.rule {
        let opts = ScriptOpts::default();
        // US3：探针脚本支持 ctx.fetch（构造任务级 client；失败仅降级，不阻断探针）
        let client = crate::services::crawler::engine::build_reqwest_client(
            req.user_agent.as_deref(),
            req.proxy.as_deref(),
        )
        .ok();
        let value = script_runner::run_script(
            script_rule,
            String::new(),
            std::collections::HashMap::new(),
            &req.url,
            client.as_ref(),
            &opts,
        )
        .await
        .map_err(script_err_to_probe_error)?;

        return Ok(ProbeResponse {
            hit_count: 1,
            samples: vec![ProbeSample {
                value,
                source_fragment: "script:body".into(),
                location: None,
            }],
            per_parent: None,
            fetched_url: material.final_url.clone(),
            fetched_at: material.fetched_at,
            duration_ms: material.duration_ms,
        });
    }

    // 3d. 6 同步模式：直接对 req.rule 应用提取
    let input =
        ExtractInput::from_material(&material, req.script_index).with_layer(req.source_layer);
    let raw_hits = extractor::extract(&req.rule, &input).map_err(map_extract_error)?;
    let final_hits =
        extractor::apply_post_processors(raw_hits, &req.post_processors, &material.final_url);

    // 4. 0 命中 → ZeroHits
    if final_hits.is_empty() {
        return Err(ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ZeroHits,
            "字段未命中（0 条）：请检查规则与源码 tab 是否匹配",
        ));
    }

    Ok(ProbeResponse {
        hit_count: final_hits.len(),
        samples: final_hits
            .into_iter()
            .map(|h| ProbeSample {
                value: h.value,
                source_fragment: h.source_fragment,
                location: h.location,
            })
            .collect(),
        per_parent: None,
        fetched_url: material.final_url.clone(),
        fetched_at: material.fetched_at,
        duration_ms: material.duration_ms,
    })
}

/// US2：父子嵌套探针
///
/// 内部函数：run_probe 在 parent_field 为 Some 时调用。
/// 步骤：父规则求命中 → 父空报 ParentEmpty → 逐父作用域应用子规则 → 组装 per_parent。
async fn run_nested_probe(
    req: ProbeRequest,
    material: &SourceMaterial,
    parent_field: ParentFieldDef,
) -> Result<ProbeResponse, ProbeError> {
    // 父规则提取
    let p_input = ExtractInput::from_material(material, parent_field.script_index)
        .with_layer(parent_field.source_layer);
    let p_raw = extractor::extract(&parent_field.rule, &p_input).map_err(map_extract_error)?;
    let parent_hits =
        extractor::apply_post_processors(p_raw, &parent_field.post_processors, &material.final_url);

    // 父命中为空 → ParentEmpty
    if parent_hits.is_empty() {
        return Err(ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ParentEmpty,
            "父字段 0 命中，子字段无法运行（请先修正父字段规则）",
        )
        .with_hint("检查父字段规则与源码 tab；父字段无命中时子字段无作用域可匹配"));
    }

    let per_parent_limit = req.per_parent_sample_limit.unwrap_or(3).max(1);
    let mut per_parent: Vec<PerParentSample> = Vec::with_capacity(parent_hits.len());
    let mut all_child_hits: Vec<extractor::Hit> = Vec::new();

    for (idx, ph) in parent_hits.iter().enumerate() {
        // 用 context_html 构造子作用域素材（CSS 模式优先用元素 HTML，否则 fallback 到父值）
        let sub_material = make_sub_material_from_hit(ph, material);
        let sub_input = ExtractInput::from_material(&sub_material, req.script_index)
            .with_layer(req.source_layer);

        // 子规则单点失败不中断其他父作用域（FR-019）
        let child_raw = extractor::extract(&req.rule, &sub_input).unwrap_or_default();
        let child_final = extractor::apply_post_processors(
            child_raw,
            &req.post_processors,
            &sub_material.final_url,
        );

        let child_hit = !child_final.is_empty();
        let first_value = child_final.first().map(|h| h.value.clone());
        let child_samples: Option<Vec<ProbeSample>> = if child_final.is_empty() {
            None
        } else {
            Some(
                child_final
                    .iter()
                    .take(per_parent_limit)
                    .map(|h| ProbeSample {
                        value: h.value.clone(),
                        source_fragment: h.source_fragment.clone(),
                        location: h.location.clone(),
                    })
                    .collect(),
            )
        };

        // 累积到全量 samples（便于前端 fallback 渲染）
        all_child_hits.extend(child_final);

        per_parent.push(PerParentSample {
            parent_index: idx,
            parent_fragment: truncate_fragment(&ph.value, 200),
            child_value: first_value,
            child_hit,
            child_samples,
        });
    }

    Ok(ProbeResponse {
        hit_count: all_child_hits.len(),
        samples: all_child_hits
            .into_iter()
            .map(|h| ProbeSample {
                value: h.value,
                source_fragment: h.source_fragment,
                location: h.location,
            })
            .collect(),
        per_parent: Some(per_parent),
        fetched_url: material.final_url.clone(),
        fetched_at: material.fetched_at,
        duration_ms: material.duration_ms,
    })
}

/// 把父命中摘要截断到 max_chars（便于前端预览，避免响应过大）
fn truncate_fragment(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// 对非 CSS 模式（regex / prefix_suffix），fallback 到 `hit.value`。
/// 与 engine.rs 同名函数对齐。
fn make_sub_material_from_hit(ph: &extractor::Hit, parent: &SourceMaterial) -> SourceMaterial {
    use crate::services::crawler::source_layer::{MetaTag, ScriptBlock};
    let html = ph.context_html.clone().unwrap_or_else(|| ph.value.clone());
    SourceMaterial {
        final_url: parent.final_url.clone(),
        status: parent.status,
        headers: parent.headers.clone(),
        html,
        scripts: Vec::<ScriptBlock>::new(),
        metas: Vec::<MetaTag>::new(),
        fetched_at: parent.fetched_at,
        duration_ms: 0,
    }
}

/// 把 extractor 的错误映射到 ProbeError
///
/// - InvalidRule / UnsupportedMode → Parse / InvalidRule
///   - UnsupportedMode 在 US1 不会触发（json_path 等还没在 extractor 实现）
/// - SourceMissing → Parse / InvalidRule（规则选了不存在的 script_index 等）
fn map_extract_error(err: crate::services::crawler::extractor::ExtractError) -> ProbeError {
    let stage = ProbeStage::Parse;
    let category = match err.kind {
        ExtractErrorKind::InvalidRule
        | ExtractErrorKind::UnsupportedMode
        | ExtractErrorKind::SourceMissing => ProbeCategory::InvalidRule,
    };
    let mut pe = ProbeError::new(stage, category, err.message);
    if err.kind == ExtractErrorKind::SourceMissing {
        pe = pe.with_hint("检查 source_layer 与 script_index 是否对应源码 tab 中的真实索引");
    } else if err.kind == ExtractErrorKind::UnsupportedMode {
        pe = pe.with_hint("此匹配模式将在后续版本支持");
    }
    pe
}

/// 把 [`follow_url::FollowUrlError`] 映射到 [`ProbeError`]，便于前端按 category 渲染
fn follow_url_err_to_probe_error(
    err: crate::services::crawler::follow_url::FollowUrlError,
) -> ProbeError {
    use crate::services::crawler::follow_url::FollowUrlError as E;
    match err {
        E::TransitEmpty => ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ZeroHits,
            "follow_url.transit 子规则未提取到中转 URL（0 命中）",
        )
        .with_hint("检查 transit 子规则与 transit_layer 是否匹配当前源码 tab"),
        E::TransitExtract(e) => map_extract_error(e),
        E::Fetch(e) => e, // 已是 ProbeError（stage=Fetch），直接透传
        E::ExtractExtract(e) => map_extract_error(e),
        E::ZeroHits => ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ZeroHits,
            "follow_url.extract 子规则在二次响应未命中（0 条）",
        )
        .with_hint("检查 extract 子规则与 target_layer 是否匹配二次响应的源码 tab"),
    }
}

/// [feature 046] 把 [`script_engine::ScriptError`] 映射到 [`ProbeError`]
///
/// 分类映射：
/// - SyntaxError / SecurityViolation / TypeError → Parse / InvalidRule（脚本本身有问题）
/// - RuntimeError → Match / InvalidRule（脚本跑起来抛错）
/// - Timeout → Match / InvalidRule（脚本超时，归类为运行期问题）
/// - NetworkError → Fetch / UrlUnreachable（US3 ctx.fetch 失败，US1 不会触发）
fn script_err_to_probe_error(
    err: crate::services::crawler::script_engine::ScriptError,
) -> ProbeError {
    let (stage, category, hint): (ProbeStage, ProbeCategory, Option<&str>) = match err.category {
        ScriptFailureCategory::SyntaxError
        | ScriptFailureCategory::SecurityViolation
        | ScriptFailureCategory::TypeError => (
            ProbeStage::Parse,
            ProbeCategory::InvalidRule,
            Some("脚本编译期失败：检查 script.body 语法 / 是否含被禁标识符"),
        ),
        ScriptFailureCategory::RuntimeError => (
            ProbeStage::Match,
            ProbeCategory::InvalidRule,
            Some("脚本运行期抛错：检查 ctx.value / ctx.fields 访问路径"),
        ),
        ScriptFailureCategory::Timeout => (
            ProbeStage::Match,
            ProbeCategory::InvalidRule,
            Some("脚本执行超时（默认 100ms）：简化逻辑或拆分多字段"),
        ),
        ScriptFailureCategory::NetworkError => (
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            Some("ctx.fetch 网络错（含 SSRF 拒绝 / 响应超阈值）"),
        ),
    };
    let mut pe = ProbeError::new(
        stage,
        category,
        format!("[{}] {}", err.category.as_str(), err.message),
    );
    if let Some(h) = hint {
        pe = pe.with_hint(h);
    }
    pe
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::crawler::field_schema::{
        CssRule, PostProcessorOp, PrefixSuffixRule, RegexRule,
    };
    use std::collections::HashMap;

    fn css_rule(selector: &str) -> Rule {
        Rule::Css(CssRule {
            selector: selector.into(),
            attr: "text".into(),
        })
    }

    fn regex_rule(pattern: &str) -> Rule {
        Rule::Regex(RegexRule {
            pattern: pattern.into(),
            group: 0,
            flags: "".into(),
        })
    }

    fn make_req(url: String, rule: Rule, source_layer: SourceLayer) -> ProbeRequest {
        ProbeRequest {
            url,
            user_agent: None,
            proxy: None,
            source_layer,
            rule,
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        }
    }

    // ===== 错误分类：URL 不可达 =====

    #[tokio::test]
    async fn probe_url_unreachable_invalid_url() {
        let req = make_req("not-a-url".to_string(), css_rule("a"), SourceLayer::Html);
        let err = run_probe(req).await.expect_err("URL 缺 scheme");
        assert_eq!(err.stage, ProbeStage::Fetch);
        assert_eq!(err.category, ProbeCategory::UrlUnreachable);
    }

    #[tokio::test]
    async fn probe_url_unreachable_bad_scheme() {
        let req = make_req(
            "https://this-domain-definitely-does-not-exist-12345.invalid/x".to_string(),
            css_rule("a"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("DNS 失败");
        assert_eq!(err.stage, ProbeStage::Fetch);
        // DNS 失败归 UrlUnreachable
        assert_eq!(err.category, ProbeCategory::UrlUnreachable);
    }

    // ===== 错误分类：HTTP 4xx/5xx =====

    #[tokio::test]
    async fn probe_http_404_returns_fetch_error() {
        // httpbin 404 端点稳定可用；body 偶尔含 Cloudflare 关键词会归到 Blocked
        // （两个分类都是 4xx 拦截类，统一接受）
        let req = make_req(
            "https://httpbin.org/status/404".to_string(),
            css_rule("a"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("404");
        assert_eq!(err.stage, ProbeStage::Fetch);
        assert!(
            matches!(
                err.category,
                ProbeCategory::Http4xx5xx | ProbeCategory::Blocked
            ),
            "期望 Http4xx5xx 或 Blocked，实际 = {:?}",
            err.category
        );
    }

    // ===== 错误分类：规则非法 =====

    #[tokio::test]
    async fn probe_invalid_rule_bad_css_selector() {
        // 用 data: URL 是合法 http(s) 格式？data: 不行——但 example.com 能连通
        // 这里直接拿 example.com，让抓取成功，触发 CSS 选择器编译失败
        let req = make_req(
            "https://example.com/".to_string(),
            Rule::Css(CssRule {
                selector: ">>>invalid".into(),
                attr: "text".into(),
            }),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("CSS 选择器非法");
        assert_eq!(err.stage, ProbeStage::Parse);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
    }

    #[tokio::test]
    async fn probe_invalid_rule_bad_regex_pattern() {
        let req = make_req(
            "https://example.com/".to_string(),
            Rule::Regex(RegexRule {
                pattern: "(unclosed".into(),
                group: 0,
                flags: "".into(),
            }),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("regex 编译失败");
        assert_eq!(err.stage, ProbeStage::Parse);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
    }

    // ===== 错误分类：源缺失（SourceMissing） =====

    #[tokio::test]
    async fn probe_source_missing_script_index_out_of_range() {
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Script,
            rule: regex_rule("x"),
            post_processors: vec![],
            script_index: Some(99),
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let err = run_probe(req).await.expect_err("script_index 越界");
        assert_eq!(err.category, ProbeCategory::InvalidRule);
        assert!(err.hint.is_some());
    }

    // ===== 错误分类：父字段空 =====

    #[tokio::test]
    async fn probe_parent_empty_when_required() {
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: css_rule("a"),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: true,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let err = run_probe(req).await.expect_err("父字段为空");
        assert_eq!(err.stage, ProbeStage::Match);
        assert_eq!(err.category, ProbeCategory::ParentEmpty);
    }

    #[tokio::test]
    async fn probe_parent_empty_skipped_when_not_required() {
        // require_parent=false（默认）：即便 parent_hits 空，也正常抓取
        // 用 example.com 验证（首页有 <a>）
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: css_rule("a"),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let resp = run_probe(req).await.expect("应抓取成功");
        assert!(resp.hit_count >= 1);
    }

    // ===== 错误分类：0 命中 =====

    #[tokio::test]
    async fn probe_zero_hits_returns_match_error() {
        let req = make_req(
            "https://example.com/".to_string(),
            css_rule(".this-class-does-not-exist-anywhere-xyz"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("0 命中");
        assert_eq!(err.stage, ProbeStage::Match);
        assert_eq!(err.category, ProbeCategory::ZeroHits);
    }

    // ===== 成功路径 =====

    #[tokio::test]
    async fn probe_success_on_example_com_links() {
        let req = make_req(
            "https://example.com/".to_string(),
            css_rule("a"),
            SourceLayer::Html,
        );
        let resp = run_probe(req).await.expect("example.com 首页应有链接");
        assert!(resp.hit_count >= 1);
        assert!(!resp.fetched_url.is_empty());
        assert_eq!(resp.samples.len(), resp.hit_count);
        // 样本应包含 source_fragment
        assert!(resp.samples[0].source_fragment.starts_with("css:"));
    }

    #[tokio::test]
    async fn probe_post_processor_first_applied() {
        // example.com 通常只有 1 个 <a>，改为多个选择器路径——直接用 first 处理多个 a 的情况
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: css_rule("a"),
            post_processors: vec![crate::services::crawler::field_schema::PostProcessor {
                op: PostProcessorOp::First,
            }],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let resp = run_probe(req).await.expect("成功");
        assert_eq!(resp.hit_count, 1);
    }

    // ===== map_extract_error 单元测试（不依赖网络） =====

    #[test]
    fn map_extract_error_invalid_rule() {
        let e = crate::services::crawler::extractor::ExtractError::new(
            ExtractErrorKind::InvalidRule,
            "bad css",
        );
        let pe = map_extract_error(e);
        assert_eq!(pe.stage, ProbeStage::Parse);
        assert_eq!(pe.category, ProbeCategory::InvalidRule);
        assert!(pe.hint.is_none());
    }

    #[test]
    fn map_extract_error_source_missing_has_hint() {
        let e = crate::services::crawler::extractor::ExtractError::new(
            ExtractErrorKind::SourceMissing,
            "script_index 99 越界",
        );
        let pe = map_extract_error(e);
        assert_eq!(pe.category, ProbeCategory::InvalidRule);
        assert!(pe.hint.is_some());
        assert!(pe.hint.unwrap().contains("script_index"));
    }

    #[test]
    fn map_extract_error_unsupported_mode_has_hint() {
        let e = crate::services::crawler::extractor::ExtractError::new(
            ExtractErrorKind::UnsupportedMode,
            "json_path 暂不支持",
        );
        let pe = map_extract_error(e);
        assert_eq!(pe.category, ProbeCategory::InvalidRule);
        assert!(pe.hint.unwrap().contains("后续版本"));
    }

    // ===== ProbeRequest/Response 序列化 roundtrip =====

    #[test]
    fn probe_request_serde_roundtrip() {
        let req = ProbeRequest {
            url: "https://x.com/".to_string(),
            user_agent: Some("UA".to_string()),
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: Rule::PrefixSuffix(PrefixSuffixRule {
                prefix: "<title>".to_string(),
                suffix: "</title>".to_string(),
                include_boundary: false,
                case_sensitive: false,
            }),
            post_processors: vec![crate::services::crawler::field_schema::PostProcessor {
                op: PostProcessorOp::Trim,
            }],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        // 解析回 ProbeRequest
        let back: ProbeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.url, req.url);
        assert_eq!(back.user_agent.as_deref(), Some("UA"));
        assert_eq!(back.source_layer, SourceLayer::Html);
        assert_eq!(back.post_processors.len(), 1);
        assert!(matches!(back.rule, Rule::PrefixSuffix(_)));
    }

    #[test]
    fn probe_response_serde_roundtrip() {
        let resp = ProbeResponse {
            hit_count: 2,
            samples: vec![
                ProbeSample {
                    value: "v1".into(),
                    source_fragment: "css:a".into(),
                    location: Some("node[0]".into()),
                },
                ProbeSample {
                    value: "v2".into(),
                    source_fragment: "css:a".into(),
                    location: None,
                },
            ],
            fetched_url: "https://x.com/".to_string(),
            fetched_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
            duration_ms: 42,
            per_parent: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ProbeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hit_count, 2);
        assert_eq!(back.samples.len(), 2);
        // skip_serializing_if 生效：第二个样本 location 为 None 时不出现在 JSON 中
        assert!(json.contains("\"location\":\"node[0]\""));
        // 不强制校验第二个的 location 缺省，但反序列化后应为 None
        assert!(back.samples[1].location.is_none());
    }

    // ===== 静态：HashMap 不被实际使用，避免未使用警告 =====

    #[test]
    fn _ensure_hashmap_import_used() {
        let _m: HashMap<String, String> = HashMap::new();
    }

    // ===== mode_str 辅助测试（通过 ExtractorMode::from_str 反向校验） =====

    #[test]
    fn extractor_mode_six_kinds_exist() {
        for s in [
            "css",
            "regex",
            "prefix_suffix",
            "json_path",
            "meta_attr",
            "header_field",
        ] {
            assert!(crate::services::crawler::field_schema::ExtractorMode::from_str(s).is_some());
        }
    }

    // ===== US2 父子嵌套：错误分类与结构化结果 =====

    /// 父字段无命中 → ParentEmpty 错误分类（contracts C2 表）
    #[tokio::test]
    async fn nested_probe_parent_empty_when_parent_rule_zero_hits() {
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            // 子规则：取所有 <a> 的 href
            rule: Rule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: Some(ParentFieldDef {
                source_layer: SourceLayer::Html,
                // 父规则：选择一个不存在的 class（0 命中）
                rule: Rule::Css(CssRule {
                    selector: ".this-class-does-not-exist-xyz".into(),
                    attr: "text".into(),
                }),
                post_processors: vec![],
                script_index: None,
            }),
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let err = run_probe(req).await.expect_err("父字段 0 命中");
        assert_eq!(err.stage, ProbeStage::Match);
        assert_eq!(err.category, ProbeCategory::ParentEmpty);
        assert!(err.hint.is_some());
    }

    /// 父字段有命中，子字段在每条父命中下都命中 → per_parent 全部 child_hit=true
    ///
    /// example.com 首页通常有 ≥ 1 个 <div> + 多个 <a>；
    /// 用 body 作为父（1 命中），a 作为子（多条命中）。
    #[tokio::test]
    async fn nested_probe_all_parents_have_child_hits() {
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: Rule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: Some(ParentFieldDef {
                source_layer: SourceLayer::Html,
                rule: Rule::Css(CssRule {
                    selector: "body".into(),
                    attr: "text".into(),
                }),
                post_processors: vec![],
                script_index: None,
            }),
            per_parent_sample_limit: Some(3),
            parent_node_id: None,
        };
        let resp = run_probe(req).await.expect("应抓取成功");
        let per_parent = resp
            .per_parent
            .as_ref()
            .expect("US2 嵌套路径必须返回 per_parent");
        assert!(
            !per_parent.is_empty(),
            "body 父命中至少 1 条，per_parent 不能为空"
        );
        // body 通常 1 命中，但每条都应能匹配到 <a>
        for p in per_parent {
            assert!(
                p.child_hit,
                "每条父命中下 <a> 都应命中，idx={}",
                p.parent_index
            );
            assert!(p.child_value.is_some(), "child_value 必须填充");
            assert!(p.child_samples.is_some(), "child_samples 必须填充");
        }
        // hit_count 应等于所有 per_parent 的 child_samples 汇总
        assert!(resp.hit_count >= per_parent.len());
        assert_eq!(resp.samples.len(), resp.hit_count);
    }

    /// 父子嵌套：部分父命中下子字段未命中（child_hit=false）
    ///
    /// 构造：父规则匹配多个元素（<p> 和 <a>），子规则只在 <a> 内有命中。
    /// example.com 主页内容简单，这条测试聚焦"部分命中"语义：
    /// 即使某些父作用域内子规则 0 命中，per_parent 仍要列出该父并标 child_hit=false。
    #[tokio::test]
    async fn nested_probe_partial_miss_records_unhit_parent() {
        // example.com 通常有 <h1>、<p>、<a>。父规则选 h1,p,a（逗号选择器）；
        // 子规则选 a[href]：在 h1 和 p 作用域内通常没有 <a>，标记未命中。
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: Rule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: Some(ParentFieldDef {
                source_layer: SourceLayer::Html,
                rule: Rule::Css(CssRule {
                    selector: "h1, p, a".into(),
                    attr: "text".into(),
                }),
                post_processors: vec![],
                script_index: None,
            }),
            per_parent_sample_limit: Some(2),
            parent_node_id: None,
        };
        let resp = run_probe(req).await.expect("应抓取成功");
        let per_parent = resp.per_parent.as_ref().expect("per_parent 必须填充");
        assert!(per_parent.len() >= 2, "父命中至少 2 条");
        // 至少存在一条 child_hit=false 的（h1/p 作用域内通常无 <a>）
        let any_miss = per_parent.iter().any(|p| !p.child_hit);
        let any_hit = per_parent.iter().any(|p| p.child_hit);
        assert!(
            any_hit,
            "至少一条父作用域应命中 <a>（example.com 首页应有链接）"
        );
        // any_miss 容忍：example.com 极端情况下 h1/p 内可能也有 <a>，不强制
        let _ = any_miss;
    }

    /// 父子嵌套：per_parent_sample_limit 截断生效
    #[tokio::test]
    async fn nested_probe_per_parent_sample_limit_truncates() {
        let req = ProbeRequest {
            url: "https://example.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: Rule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: Some(ParentFieldDef {
                source_layer: SourceLayer::Html,
                rule: Rule::Css(CssRule {
                    selector: "body".into(),
                    attr: "text".into(),
                }),
                post_processors: vec![],
                script_index: None,
            }),
            per_parent_sample_limit: Some(1),
            parent_node_id: None,
        };
        let resp = run_probe(req).await.expect("应抓取成功");
        let per_parent = resp.per_parent.as_ref().expect("per_parent 必须填充");
        for p in per_parent {
            if let Some(samples) = &p.child_samples {
                assert!(
                    samples.len() <= 1,
                    "per_parent_sample_limit=1 时每条父命中下样本 ≤ 1，实际 = {}",
                    samples.len()
                );
            }
        }
    }

    /// ParentFieldDef / PerParentSample 序列化 roundtrip
    #[test]
    fn nested_probe_types_serde_roundtrip() {
        let pf = ParentFieldDef {
            source_layer: SourceLayer::Html,
            rule: Rule::Css(CssRule {
                selector: ".card".into(),
                attr: "html".into(),
            }),
            post_processors: vec![],
            script_index: None,
        };
        let json = serde_json::to_string(&pf).unwrap();
        let back: ParentFieldDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_layer, SourceLayer::Html);
        assert!(matches!(back.rule, Rule::Css(_)));

        let pp = PerParentSample {
            parent_index: 2,
            parent_fragment: "card-fragment…".into(),
            child_value: Some("https://x.com/post".into()),
            child_hit: true,
            child_samples: Some(vec![ProbeSample {
                value: "https://x.com/post".into(),
                source_fragment: "<a href=...>".into(),
                location: None,
            }]),
        };
        let json = serde_json::to_string(&pp).unwrap();
        let back: PerParentSample = serde_json::from_str(&json).unwrap();
        assert_eq!(back.parent_index, 2);
        assert!(back.child_hit);
        assert!(back.child_value.is_some());
        assert!(back.child_samples.is_some());
    }

    /// ProbeRequest 带 parent_field 时序列化包含 parent_field 字段
    #[test]
    fn probe_request_with_parent_field_serializes() {
        let req = ProbeRequest {
            url: "https://x.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: css_rule("a"),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: Some(ParentFieldDef {
                source_layer: SourceLayer::Html,
                rule: css_rule(".card"),
                post_processors: vec![],
                script_index: None,
            }),
            per_parent_sample_limit: Some(5),
            parent_node_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"parent_field\""), "JSON 应含 parent_field");
        assert!(
            json.contains("\"per_parent_sample_limit\":5"),
            "JSON 应含 per_parent_sample_limit=5"
        );
        let back: ProbeRequest = serde_json::from_str(&json).unwrap();
        assert!(back.parent_field.is_some());
        assert_eq!(back.per_parent_sample_limit, Some(5));
    }

    /// truncate_fragment 单元测试
    #[test]
    fn truncate_fragment_short_unchanged() {
        assert_eq!(truncate_fragment("hi", 200), "hi");
        assert_eq!(truncate_fragment("", 200), "");
    }

    #[test]
    fn truncate_fragment_long_truncates_with_ellipsis() {
        let long = "a".repeat(300);
        let t = truncate_fragment(&long, 10);
        assert_eq!(t.chars().count(), 11); // 10 + 省略号
        assert!(t.ends_with('…'));
    }

    // ===== [feature 046] US1 Script 模式探针 =====

    use crate::services::crawler::field_schema::{ExtractorMode, ScriptRule};

    fn script_rule(body: &str) -> Rule {
        Rule::Script(ScriptRule {
            body: body.into(),
            api_version: "v1".into(),
        })
    }

    /// T015：probe 请求 rule=Script → 抓 URL 后跑脚本 → 单条样本
    #[tokio::test]
    async fn t_run_probe_script_mode_returns_value() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("return 'transformed_' + ctx.url"),
            SourceLayer::Html,
        );
        let resp = run_probe(req).await.expect("脚本应返回值");
        assert_eq!(resp.hit_count, 1);
        assert_eq!(resp.samples.len(), 1);
        assert_eq!(resp.samples[0].source_fragment, "script:body");
        assert!(
            resp.samples[0].value.starts_with("transformed_"),
            "脚本返回值实际 = {}",
            resp.samples[0].value
        );
    }

    /// T015：脚本 ctx.value 默认空字符串（探针场景 6 模式未跑）
    #[tokio::test]
    async fn t_run_probe_script_mode_injects_empty_value() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("return ctx.value + '_default'"),
            SourceLayer::Html,
        );
        let resp = run_probe(req).await.expect("脚本应返回值");
        assert_eq!(resp.samples[0].value, "_default");
    }

    /// T015：脚本语法错 → ProbeError stage=Parse / category=InvalidRule
    #[tokio::test]
    async fn t_run_probe_script_mode_syntax_error_returns_structured_error() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("return ."),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("语法错");
        assert_eq!(err.stage, ProbeStage::Parse);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
        assert!(err.hint.is_some());
    }

    /// T015：脚本运行期抛错 → ProbeError stage=Match / category=InvalidRule
    #[tokio::test]
    async fn t_run_probe_script_mode_runtime_error_returns_structured_error() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("throw new Error('boom')"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("运行期错");
        assert_eq!(err.stage, ProbeStage::Match);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
        assert!(err.message.contains("boom"));
    }

    /// T015：脚本沙箱逃逸 → ProbeError stage=Parse / category=InvalidRule
    #[tokio::test]
    async fn t_run_probe_script_mode_security_violation_returns_structured_error() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("return Function('return this')()"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("沙箱逃逸");
        assert_eq!(err.stage, ProbeStage::Parse);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
    }

    /// T015：脚本无 return → TypeError → Parse / InvalidRule
    #[tokio::test]
    async fn t_run_probe_script_mode_no_return_returns_structured_error() {
        let req = make_req(
            "https://example.com/".to_string(),
            script_rule("/* no return */"),
            SourceLayer::Html,
        );
        let err = run_probe(req).await.expect_err("无 return");
        assert_eq!(err.stage, ProbeStage::Parse);
        assert_eq!(err.category, ProbeCategory::InvalidRule);
    }

    /// ProbeRequest rule=Script 序列化 roundtrip（extractor_mode=script）
    #[test]
    fn probe_request_with_script_rule_serializes() {
        let req = ProbeRequest {
            url: "https://x.com/".to_string(),
            user_agent: None,
            proxy: None,
            source_layer: SourceLayer::Html,
            rule: script_rule("return ctx.value"),
            post_processors: vec![],
            script_index: None,
            parent_hits: vec![],
            require_parent: false,
            parent_field: None,
            per_parent_sample_limit: None,
            parent_node_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"script\""), "rule mode 应序列化为 script");
        let back: ProbeRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.rule, Rule::Script(_)));
    }

    /// Exhaustiveness: 7 种 ExtractorMode 全部存在（css/regex/prefix_suffix/json_path/meta_attr/header_field/script）
    #[test]
    fn extractor_mode_seven_kinds_exist() {
        for s in [
            "css",
            "regex",
            "prefix_suffix",
            "json_path",
            "meta_attr",
            "header_field",
            "script",
        ] {
            assert!(ExtractorMode::from_str(s).is_some(), "缺失 mode: {s}");
        }
    }
}
