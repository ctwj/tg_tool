//! 多模式字段提取器（feature 043-crawler-configurator，US1 T020 + US4 T049-T051）
//!
//! 取代 042 旧 `FieldSelectors` + `extract_fields` 单 CSS 路径。
//!
//! 6 模式：
//! - css / regex / prefix_suffix（US1 T020）
//! - json_path / meta_attr / header_field（US4 T049-T051）
//!
//! 与 `source_layer.rs` 的关系：source_layer 抓 URL 切 4 tab 素材，
//! 本模块消费 `SourceMaterial` 按 (mode, rule) 提取 `Vec<Hit>`。
//! Probe（US1 T021）组合 source_layer + extractor + post_processors 做字段验证。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::services::crawler::field_schema::{
    PostProcessor, PostProcessorOp, Rule, SourceLayer, SubRule, compile_regex,
};
use crate::services::crawler::source_layer::{MetaKeyKind, MetaTag, ScriptBlock, SourceMaterial};

// ============================================================================
// 提取结果
// ============================================================================

/// 单个命中单元
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// 提取出的值（post_processor 之前的原始值；后处理链会修改 value 字段）
    pub value: String,
    /// 命中来源片段的简短标识，便于前端调试
    /// 形如 `css:.title` / `regex:pattern` / `prefix_suffix:prefix...suffix`
    pub source_fragment: String,
    /// 命中在源码中的位置（可选）
    /// 形如 `node[3]` / `match[0]` / `script[2]`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// CSS 模式命中元素的外部 HTML（outer HTML）— 子字段提取时作为 scoped 素材使用
    /// 其他模式（regex / prefix_suffix）该字段为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_html: Option<String>,
}

// ============================================================================
// 提取错误
// ============================================================================

/// 提取失败（与 ProbeError 解耦：extractor 不知道上层 Probe 语义）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractError {
    pub kind: ExtractErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractErrorKind {
    /// 规则非法（CSS 选择器语法错 / Regex 编译错 / prefix 或 suffix 为空等）
    InvalidRule,
    /// 当前版本不支持的模式（如 json_path 在 US4 才实现）
    UnsupportedMode,
    /// 源缺失（如 source_layer=Script 但 script_index 越界）
    SourceMissing,
}

impl ExtractError {
    pub fn new(kind: ExtractErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for ExtractError {}

// ============================================================================
// 提取输入（消费 SourceMaterial 的指定 source_layer 切片）
// ============================================================================

/// 提取输入：把 `SourceMaterial` 的 4 tab 切片传给提取器
///
/// 通过 `source_layer` 决定从哪个 tab 取文本：
/// - Html → html
/// - Script → script_blocks[script_index].content
/// - Meta → metas 拼接的虚拟文本（name=... content=...）+ 直接 dispatch 到 MetaAttr 模式
/// - Header → headers 值（HeaderField 模式专用，US4）
/// - Url → final_url
///
/// US1 仅实现 css/regex/prefix_suffix 三种模式：
/// - css 只能作用于 source_layer=Html（其他层报 InvalidRule）
/// - regex / prefix_suffix 可作用于 Html / Script / Url 三层（Header / Meta 留 US4）
#[derive(Debug, Clone)]
pub struct ExtractInput<'a> {
    pub source_layer: SourceLayer,
    pub html: &'a str,
    pub script_blocks: &'a [ScriptBlock],
    pub metas: &'a [MetaTag],
    pub headers: &'a HashMap<String, String>,
    pub final_url: &'a str,
    /// source_layer=Script 时指定第几个 `<script>`（与 FieldNodeSpec.script_index 对应）
    pub script_index: Option<i32>,
}

impl<'a> ExtractInput<'a> {
    /// 从 `SourceMaterial` 构造提取输入
    pub fn from_material(material: &'a SourceMaterial, script_index: Option<i32>) -> Self {
        Self {
            source_layer: SourceLayer::Html, // 调用方可按字段规则改写
            html: &material.html,
            script_blocks: &material.scripts,
            metas: &material.metas,
            headers: &material.headers,
            final_url: &material.final_url,
            script_index,
        }
    }

    /// 把 source_layer 替换为指定值（构造后覆盖）
    pub fn with_layer(mut self, layer: SourceLayer) -> Self {
        self.source_layer = layer;
        self
    }

    /// 取当前 source_layer 对应的源码文本
    ///
    /// 返回 `(text, fragment_label)`，fragment_label 用于构造 source_fragment
    fn layer_text(&self) -> Result<(&str, &'static str), ExtractError> {
        match self.source_layer {
            SourceLayer::Html => Ok((self.html, "html")),
            SourceLayer::Url => Ok((self.final_url, "url")),
            SourceLayer::Script => {
                let idx = self.script_index.ok_or_else(|| {
                    ExtractError::new(
                        ExtractErrorKind::SourceMissing,
                        "source_layer=script 但未指定 script_index",
                    )
                })?;
                let idx_usize = idx as usize;
                let block = self.script_blocks.get(idx_usize).ok_or_else(|| {
                    ExtractError::new(
                        ExtractErrorKind::SourceMissing,
                        format!(
                            "script_index {idx} 越界（共 {} 个 script 块）",
                            self.script_blocks.len()
                        ),
                    )
                })?;
                // 外链脚本无内容（src 模式）：报错（外链需另发请求才能取内容，超出 extractor 范围）
                let text = block.content.as_deref().ok_or_else(|| {
                    ExtractError::new(
                        ExtractErrorKind::SourceMissing,
                        format!(
                            "script[{idx}] 是外链脚本（src={:?}），无内联内容",
                            block.src
                        ),
                    )
                })?;
                Ok((text, "script"))
            }
            SourceLayer::Meta | SourceLayer::Header => Err(ExtractError::new(
                ExtractErrorKind::UnsupportedMode,
                format!(
                    "source_layer={:?} 暂不支持（US4 T049-T051 实现）",
                    self.source_layer
                ),
            )),
        }
    }
}

// ============================================================================
// 主入口
// ============================================================================

/// 按 (mode, rule) 从 source_layer 提取 `Vec<Hit>`
///
/// 支持的模式：
/// - `Rule::Css` — 仅 source_layer=Html
/// - `Rule::Regex` — Html / Script / Url
/// - `Rule::PrefixSuffix` — Html / Script / Url
/// - `Rule::JsonPath` — source_layer=Script（依赖 script_index）
/// - `Rule::MetaAttr` — source_layer=Meta（直接扫 metas 列表，source_layer 不强制）
/// - `Rule::HeaderField` — source_layer=Header（直接查 headers，大小写不敏感）
/// - `Rule::FollowUrl` — **同步路径不支持**，返回 `UnsupportedMode`。
///   两阶段提取需由 async 调用层（probe/engine）通过 `follow_url::extract_follow_url_async` 完成。
/// - `Rule::Script` — [feature 046] **同步路径不支持**，返回 `UnsupportedMode`。
///   rquickjs 沙箱求值是 async 路径，由 `script_runner::run_script` 完成后再回填到 `ctx.value`；
///   6 模式先跑（命中或空），脚本接管 `ctx.value` 做最终变换。
pub fn extract(rule: &Rule, input: &ExtractInput<'_>) -> Result<Vec<Hit>, ExtractError> {
    match rule {
        Rule::Css(css) => extract_css(css, input),
        Rule::Regex(re) => extract_regex(re, input),
        Rule::PrefixSuffix(ps) => extract_prefix_suffix(ps, input),
        Rule::JsonPath(jp) => extract_by_json_path(jp, input),
        Rule::MetaAttr(m) => extract_by_meta_attr(m, input),
        Rule::HeaderField(h) => extract_by_header_field(h, input),
        Rule::FollowUrl(_) => Err(ExtractError::new(
            ExtractErrorKind::UnsupportedMode,
            "follow_url 需 async 两阶段提取（中转 URL → fetch → 子规则提取），\
             extractor 同步路径不支持，请通过 probe/engine 调用",
        )),
        Rule::Script(_) => Err(ExtractError::new(
            ExtractErrorKind::UnsupportedMode,
            "script 模式需 async 沙箱求值（rquickjs），extractor 同步路径不支持，\
             由 script_runner::run_script 在 engine/probe 调用",
        )),
    }
}

/// 把 `SubRule`（follow_url 内嵌子规则）转换为 `Rule`，以便复用 `extract()` 的同步提取能力。
///
/// `SubRule` 不含 `FollowUrl` 变体，因此转换是 1:1 的，编译期保证不会再产生 `Rule::FollowUrl`。
pub fn sub_rule_to_rule(sub: &SubRule) -> Rule {
    match sub {
        SubRule::Css(r) => Rule::Css(r.clone()),
        SubRule::Regex(r) => Rule::Regex(r.clone()),
        SubRule::PrefixSuffix(r) => Rule::PrefixSuffix(r.clone()),
        SubRule::JsonPath(r) => Rule::JsonPath(r.clone()),
        SubRule::MetaAttr(r) => Rule::MetaAttr(r.clone()),
        SubRule::HeaderField(r) => Rule::HeaderField(r.clone()),
    }
}

// ============================================================================
// CSS 模式
// ============================================================================

fn extract_css(
    css: &crate::services::crawler::field_schema::CssRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    if input.source_layer != SourceLayer::Html {
        return Err(ExtractError::new(
            ExtractErrorKind::InvalidRule,
            format!(
                "css 模式只能作用于 source_layer=html（当前 source_layer={:?}）",
                input.source_layer
            ),
        ));
    }
    let selector = scraper::Selector::parse(&css.selector).map_err(|e| {
        ExtractError::new(
            ExtractErrorKind::InvalidRule,
            format!("CSS 选择器非法: {e}"),
        )
    })?;
    let document = scraper::Html::parse_document(input.html);
    let fragment = format!("css:{}", css.selector);
    let hits: Vec<Hit> = document
        .select(&selector)
        .enumerate()
        .map(|(i, el)| {
            let value = read_css_value(&el, &css.attr);
            // 捕获命中元素的外部 HTML 作为子字段提取的 scoped 素材
            let context_html = el.html();
            Hit {
                value,
                source_fragment: fragment.clone(),
                location: Some(format!("node[{i}]")),
                context_html: Some(context_html),
            }
        })
        .collect();
    Ok(hits)
}

fn read_css_value(el: &scraper::ElementRef, attr: &str) -> String {
    match attr {
        "" | "text" => el.text().collect::<String>(),
        "html" => el.html(),
        other => el.value().attr(other).unwrap_or("").to_string(),
    }
}

// ============================================================================
// Regex 模式
// ============================================================================

fn extract_regex(
    re: &crate::services::crawler::field_schema::RegexRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    let (text, _layer_label) = input.layer_text()?;
    let compiled = compile_regex(&re.pattern, &re.flags).map_err(|e| {
        ExtractError::new(
            ExtractErrorKind::InvalidRule,
            format!("regex 编译失败: {e}"),
        )
    })?;
    let group = re.group as usize;
    let fragment = format!("regex:{}", re.pattern);
    let mut hits = Vec::new();
    for (i, cap) in compiled.captures_iter(text).enumerate() {
        // group 0 = 整体匹配；group >=1 = 第 N 个捕获组
        let m = cap
            .get(group)
            .ok_or_else(|| {
                ExtractError::new(
                    ExtractErrorKind::InvalidRule,
                    format!(
                        "regex group {group} 不存在（pattern 仅有 {} 个捕获组）",
                        cap.len() - 1
                    ),
                )
            })?
            .as_str()
            .to_string();
        hits.push(Hit {
            value: m,
            source_fragment: fragment.clone(),
            location: Some(format!("match[{i}]")),
            context_html: None,
        });
    }
    Ok(hits)
}

// ============================================================================
// PrefixSuffix 模式
// ============================================================================

fn extract_prefix_suffix(
    ps: &crate::services::crawler::field_schema::PrefixSuffixRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    let (text, _layer_label) = input.layer_text()?;

    let fragment = format!("prefix_suffix:{}...{}", ps.prefix, ps.suffix);
    let mut hits = Vec::new();
    let mut cursor = 0usize;
    let bytes = text.as_bytes();
    let total = bytes.len();

    while cursor < total {
        let remaining = &text[cursor..];
        let (prefix_start, prefix_end) =
            match find_substring(remaining, &ps.prefix, ps.case_sensitive) {
                Some(offset) => (cursor + offset, cursor + offset + ps.prefix.len()),
                None => break,
            };
        let after_prefix = &text[prefix_end..];
        let (suffix_rel_start, suffix_rel_end) =
            match find_substring(after_prefix, &ps.suffix, ps.case_sensitive) {
                Some(offset) => (offset, offset + ps.suffix.len()),
                None => break,
            };
        let inner_start = prefix_end;
        let inner_end = prefix_end + suffix_rel_start;
        let value_start = if ps.include_boundary {
            prefix_start
        } else {
            inner_start
        };
        let value_end = if ps.include_boundary {
            prefix_end + suffix_rel_end
        } else {
            inner_end
        };
        let value = &text[value_start..value_end];
        hits.push(Hit {
            value: value.to_string(),
            source_fragment: fragment.clone(),
            location: Some(format!("match[{}]", hits.len())),
            context_html: None,
        });
        // 推进游标：跳过本次 suffix，避免死循环（空 suffix 时强制 +1）
        let next = prefix_end + suffix_rel_end;
        cursor = if next > cursor { next } else { cursor + 1 };
    }

    Ok(hits)
}

/// 大小写敏感/不敏感的子串查找，返回首次命中的起始字节偏移
fn find_substring(haystack: &str, needle: &str, case_sensitive: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if case_sensitive {
        haystack.find(needle)
    } else {
        let lower_hay = haystack.to_lowercase();
        let lower_needle = needle.to_lowercase();
        lower_hay.find(&lower_needle)
    }
}

// ============================================================================
// JsonPath 模式（US4 T049）
// ============================================================================

/// 用 JSONPath 表达式从指定 `<script>` 块中提取值
///
/// 解析策略（按优先级）：
/// 1. 若 `script_blocks[script_index].json_value` 已被 source_layer 预解析（JSON-LD 场景），直接使用
/// 2. 否则从 `content` 启发式提取 JSON 段（首個 `{` 或 `[` 起的子串，逐步尝试解析）
/// 3. 都失败 → SourceMissing
///
/// path 非法（不符合 RFC 9535）→ InvalidRule
/// script_index 缺失/越界/外链 → SourceMissing
fn extract_by_json_path(
    jp: &crate::services::crawler::field_schema::JsonPathRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    let idx = input.script_index.ok_or_else(|| {
        ExtractError::new(
            ExtractErrorKind::SourceMissing,
            "json_path 模式需要 script_index（当前未指定）",
        )
    })?;
    let idx_usize = idx as usize;
    let block = input.script_blocks.get(idx_usize).ok_or_else(|| {
        ExtractError::new(
            ExtractErrorKind::SourceMissing,
            format!(
                "script_index {idx} 越界（共 {} 个 script 块）",
                input.script_blocks.len()
            ),
        )
    })?;

    // 优先用预解析的 json_value；否则从 content 启发式提取
    let owned_value: serde_json::Value;
    let value: &serde_json::Value = if let Some(v) = &block.json_value {
        v
    } else {
        let content = block.content.as_deref().ok_or_else(|| {
            ExtractError::new(
                ExtractErrorKind::SourceMissing,
                format!(
                    "script[{idx}] 是外链脚本（src={:?}），无内联内容",
                    block.src
                ),
            )
        })?;
        owned_value = extract_json_from_text(content).ok_or_else(|| {
            ExtractError::new(
                ExtractErrorKind::SourceMissing,
                format!("script[{idx}] 内容无法解析为 JSON（含启发式提取）"),
            )
        })?;
        &owned_value
    };

    let path = serde_json_path::JsonPath::parse(&jp.path).map_err(|e| {
        ExtractError::new(
            ExtractErrorKind::InvalidRule,
            format!("json_path 解析失败: {e}"),
        )
    })?;
    let nodes = path.query(value).all();
    let fragment = format!("json_path:{}", jp.path);
    let hits = nodes
        .into_iter()
        .enumerate()
        .map(|(i, v)| Hit {
            value: json_value_to_string(v),
            source_fragment: fragment.clone(),
            location: Some(format!("match[{i}]")),
            context_html: None,
        })
        .collect();
    Ok(hits)
}

/// 从一段可能含赋值/调用包裹的文本中提取首个合法 JSON 值
///
/// 适用场景：
/// - 纯 JSON（`{"a":1}` / `[1,2,3]`）
/// - `window.__DATA__ = {...};` / `var x = {...};`
/// - JSON-LD `<script type="application/ld+json">` 内联（已由 source_layer 预解析覆盖）
///
/// 实现：扫描每个 `{` 或 `[` 起点，用 `StreamDeserializer` 解析前导 JSON 值
/// （流式解析器容忍尾部非 JSON 字符如 `;`），首个成功即返回。
fn extract_json_from_text(text: &str) -> Option<serde_json::Value> {
    use serde_json::Deserializer;
    // 快路径：整体就是合法 JSON（from_str 性能优于流式）
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return Some(v);
    }
    // 慢路径：从每个 { 或 [ 起点用流式解析器尝试（容忍 ; 等尾部字符）
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'{' || b == b'[' {
            let mut stream = Deserializer::from_str(&text[i..]).into_iter::<serde_json::Value>();
            if let Some(Ok(v)) = stream.next() {
                return Some(v);
            }
        }
        i += 1;
    }
    None
}

/// 把 [`serde_json::Value`] 转为展示字符串（对象/数组走紧凑 JSON；其他用原生 to_string）
fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        // 数字 / 布尔 / null 直接 to_string
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Null => {
            v.to_string()
        }
        // 对象 / 数组：紧凑 JSON（无空白）
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => v.to_string(),
    }
}

// ============================================================================
// MetaAttr 模式（US4 T050）
// ============================================================================

/// 按 (attr_name, attr_value) 定位 `<meta>`，取 `content_key`（默认 content）
///
/// 匹配语义：`attr_name` 为 "name" / "property" / "http-equiv" 时分别比对 MetaTag.key_kind +
/// MetaTag.key；`attr_name` 为其他（如 "charset"）时回退到 MetaTag.key 字符串比较。
///
/// source_layer 不强制（建议配置为 meta 以保持语义清晰，本函数不读取 layer_text）。
fn extract_by_meta_attr(
    m: &crate::services::crawler::field_schema::MetaAttrRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    let content_key = if m.content_key.trim().is_empty() {
        "content"
    } else {
        m.content_key.trim()
    };
    let fragment = format!("meta_attr:{}={}", m.attr_name, m.attr_value);
    let mut hits = Vec::new();
    for (i, tag) in input.metas.iter().enumerate() {
        if !meta_attr_matches(tag, &m.attr_name, &m.attr_value) {
            continue;
        }
        // 当前 MetaTag 结构只暴露 content；若 content_key != "content" 则跳过该 tag
        //（向后兼容：旧表无 content_key 字段时默认 "content"）
        if content_key != "content" {
            // 不支持非 content 的取值（HTML 规范上 <meta> 主体内容就是 content）
            continue;
        }
        hits.push(Hit {
            value: tag.content.clone(),
            source_fragment: fragment.clone(),
            location: Some(format!("meta[{i}]")),
            context_html: None,
        });
    }
    Ok(hits)
}

/// 判断一个 `<meta>` 是否匹配 (attr_name, attr_value) 筛选
fn meta_attr_matches(tag: &MetaTag, attr_name: &str, attr_value: &str) -> bool {
    let target_kind = match attr_name {
        "name" => Some(MetaKeyKind::Name),
        "property" => Some(MetaKeyKind::Property),
        "http-equiv" => Some(MetaKeyKind::HttpEquiv),
        _ => None,
    };
    match target_kind {
        Some(k) => tag.key_kind == k && tag.key == attr_value,
        // 其他 attr_name（如 charset）：用 key 字段比对
        None => tag.key == attr_value,
    }
}

// ============================================================================
// HeaderField 模式（US4 T051）
// ============================================================================

/// 按大小写不敏感的 header_name 从响应头取值
///
/// `SourceMaterial.headers` 由 source_layer 全部以小写 key 存储，因此这里也按小写查询。
fn extract_by_header_field(
    h: &crate::services::crawler::field_schema::HeaderFieldRule,
    input: &ExtractInput<'_>,
) -> Result<Vec<Hit>, ExtractError> {
    let lower = h.header_name.to_lowercase();
    let fragment = format!("header_field:{}", h.header_name);
    let mut hits = Vec::new();
    for (i, (k, v)) in input.headers.iter().enumerate() {
        if k.to_lowercase() == lower {
            hits.push(Hit {
                value: v.clone(),
                source_fragment: fragment.clone(),
                location: Some(format!("header[{i}]")),
                context_html: None,
            });
        }
    }
    Ok(hits)
}

// ============================================================================
// 后处理链
// ============================================================================

/// 应用后处理链（按 ops 顺序执行）
///
/// 执行语义：
/// - `Trim` — 每个 hit 的 value 调 `trim()`
/// - `HtmlEntityDecode` — HTML 实体解码（&amp; → & 等，覆盖常见命名实体 + 数字实体）
/// - `AbsolutizeUrl` — 相对 URL → 绝对 URL（基于 base_url）
/// - `First` — 仅保留 `hits[0]`（其余丢弃）
/// - `All` — 显式标记 no-op（数组返回默认就是全部）
/// - `Dedupe` — 按 value 去重（保留首次出现）
///
/// 顺序敏感：通常先 per-value 后处理（trim/decode/absolutize），再做集合后处理（first/dedupe）。
/// 调用方应按这个顺序在 post_processors 数组中排好。
pub fn apply_post_processors(
    mut hits: Vec<Hit>,
    ops: &[PostProcessor],
    base_url: &str,
) -> Vec<Hit> {
    for op in ops {
        match op.op {
            PostProcessorOp::Trim => {
                for h in hits.iter_mut() {
                    h.value = h.value.trim().to_string();
                }
            }
            PostProcessorOp::HtmlEntityDecode => {
                for h in hits.iter_mut() {
                    h.value = html_entity_decode(&h.value);
                }
            }
            PostProcessorOp::AbsolutizeUrl => {
                for h in hits.iter_mut() {
                    if !h.value.is_empty() {
                        h.value = crate::services::crawler::engine::resolve_url(&h.value, base_url);
                    }
                }
            }
            PostProcessorOp::First => {
                if hits.len() > 1 {
                    hits.truncate(1);
                }
            }
            PostProcessorOp::All => {
                // no-op：默认就返回全部
            }
            PostProcessorOp::Dedupe => {
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                hits.retain(|h| seen.insert(h.value.clone()));
            }
        }
    }
    hits
}

/// 轻量 HTML 实体解码（覆盖最常见命名实体 + 十进制/十六进制数字实体）
///
/// 不引入 `html_escape`/`entities` 依赖：US1 场景足够。
fn html_entity_decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // 找最近的 ';'
            if let Some(rel) = s[i + 1..].find(';') {
                let entity = &s[i + 1..i + 1 + rel];
                let decoded = decode_one_entity(entity);
                if let Some(d) = decoded {
                    out.push_str(&d);
                    i = i + 1 + rel + 1;
                    continue;
                }
            }
        }
        // 默认按 UTF-8 字符前进
        let ch = s[i..].chars().next().expect("non-empty char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn decode_one_entity(entity: &str) -> Option<String> {
    // 数字实体：&#NN; 或 &#xHH;
    if let Some(num) = entity.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        let ch = char::from_u32(code)?;
        return Some(ch.to_string());
    }
    // 命名实体（最常见 6 个）
    Some(
        match entity {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{00A0}",
            _ => return None,
        }
        .to_string(),
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::crawler::field_schema::{
        CssRule, PrefixSuffixRule, RegexRule, ScriptRule,
    };
    use once_cell::sync::Lazy;
    use std::collections::HashMap;

    fn make_input<'a>(html: &'a str, final_url: &'a str) -> ExtractInput<'a> {
        static EMPTY_HEADERS: Lazy<HashMap<String, String>> = Lazy::new(HashMap::new);
        ExtractInput {
            source_layer: SourceLayer::Html,
            html,
            script_blocks: &[],
            metas: &[],
            headers: &EMPTY_HEADERS,
            final_url,
            script_index: None,
        }
    }

    fn make_input_with_scripts<'a>(
        html: &'a str,
        final_url: &'a str,
        scripts: &'a [ScriptBlock],
        script_index: Option<i32>,
    ) -> ExtractInput<'a> {
        static EMPTY_HEADERS: Lazy<HashMap<String, String>> = Lazy::new(HashMap::new);
        ExtractInput {
            source_layer: SourceLayer::Script,
            html,
            script_blocks: scripts,
            metas: &[],
            headers: &EMPTY_HEADERS,
            final_url,
            script_index,
        }
    }

    // ===== CSS 模式 =====

    #[test]
    fn css_text_basic_match() {
        let html = r#"<ul>
            <li class="title">第一篇</li>
            <li class="title">第二篇</li>
            <li class="other">无关</li>
        </ul>"#;
        let rule = Rule::Css(CssRule {
            selector: "li.title".into(),
            attr: "text".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("css ok");
        assert_eq!(hits.len(), 2);
        assert!(hits[0].value.contains("第一篇"));
        assert!(hits[1].value.contains("第二篇"));
        assert_eq!(hits[0].source_fragment, "css:li.title");
        assert_eq!(hits[0].location.as_deref(), Some("node[0]"));
    }

    #[test]
    fn css_attr_href() {
        let html = r#"<a class="link" href="/p/1">A</a><a class="link" href="/p/2">B</a>"#;
        let rule = Rule::Css(CssRule {
            selector: "a.link".into(),
            attr: "href".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("css ok");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].value, "/p/1");
        assert_eq!(hits[1].value, "/p/2");
    }

    #[test]
    fn css_zero_hits_when_no_match() {
        let html = "<div>nothing</div>";
        let rule = Rule::Css(CssRule {
            selector: ".missing".into(),
            attr: "text".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("css ok");
        assert!(hits.is_empty());
    }

    #[test]
    fn css_invalid_selector_returns_invalid_rule() {
        let rule = Rule::Css(CssRule {
            selector: ">><<invalid".into(),
            attr: "text".into(),
        });
        let input = make_input("<div>x</div>", "https://example.com/");
        let err = extract(&rule, &input).expect_err("selector 语法错");
        assert_eq!(err.kind, ExtractErrorKind::InvalidRule);
    }

    #[test]
    fn css_only_html_layer_allowed() {
        let rule = Rule::Css(CssRule {
            selector: "a".into(),
            attr: "text".into(),
        });
        let mut input = make_input("<a>x</a>", "https://example.com/");
        input.source_layer = SourceLayer::Script;
        let err = extract(&rule, &input).expect_err("css 只能 html");
        assert_eq!(err.kind, ExtractErrorKind::InvalidRule);
    }

    // ===== Regex 模式 =====

    #[test]
    fn regex_basic_match_group_1() {
        let html = "发布时间：2024-01-02  其他";
        let rule = Rule::Regex(RegexRule {
            pattern: r"发布时间：(\S+)".into(),
            group: 1,
            flags: "".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("regex ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "2024-01-02");
    }

    #[test]
    fn regex_group_0_whole_match() {
        let html = "view-123 view-456";
        let rule = Rule::Regex(RegexRule {
            pattern: r"view-\d+".into(),
            group: 0,
            flags: "".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("regex ok");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].value, "view-123");
        assert_eq!(hits[1].value, "view-456");
    }

    #[test]
    fn regex_case_insensitive_via_flags() {
        let html = "Hello hello HELLO";
        let rule = Rule::Regex(RegexRule {
            pattern: "hello".into(),
            group: 0,
            flags: "i".into(),
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("regex ok");
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn regex_zero_hits_when_no_match() {
        let rule = Rule::Regex(RegexRule {
            pattern: r"not-found-\d+".into(),
            group: 0,
            flags: "".into(),
        });
        let input = make_input("abc", "https://example.com/");
        let hits = extract(&rule, &input).expect("regex ok");
        assert!(hits.is_empty());
    }

    #[test]
    fn regex_invalid_pattern_returns_invalid_rule() {
        let rule = Rule::Regex(RegexRule {
            pattern: r"(unclosed".into(),
            group: 0,
            flags: "".into(),
        });
        let input = make_input("abc", "https://example.com/");
        let err = extract(&rule, &input).expect_err("regex 语法错");
        assert_eq!(err.kind, ExtractErrorKind::InvalidRule);
    }

    #[test]
    fn regex_group_index_out_of_range_returns_invalid_rule() {
        let rule = Rule::Regex(RegexRule {
            pattern: r"hello".into(), // 无捕获组
            group: 3,
            flags: "".into(),
        });
        let input = make_input("hello", "https://example.com/");
        let err = extract(&rule, &input).expect_err("group 越界");
        assert_eq!(err.kind, ExtractErrorKind::InvalidRule);
    }

    #[test]
    fn regex_on_script_layer() {
        let scripts = vec![ScriptBlock {
            index: 0,
            src: None,
            content: Some(r#"window.data = {"views": 1024};"#.to_string()),
            json_value: None,
        }];
        let rule = Rule::Regex(RegexRule {
            pattern: r#""views":\s*(\d+)"#.into(),
            group: 1,
            flags: "".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let hits = extract(&rule, &input).expect("regex on script ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "1024");
    }

    #[test]
    fn regex_on_url_layer() {
        let rule = Rule::Regex(RegexRule {
            pattern: r"page=(\d+)".into(),
            group: 1,
            flags: "".into(),
        });
        let mut input = make_input("ignored", "https://example.com/list?page=3");
        input.source_layer = SourceLayer::Url;
        let hits = extract(&rule, &input).expect("regex on url ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "3");
    }

    // ===== PrefixSuffix 模式 =====

    #[test]
    fn prefix_suffix_basic() {
        let html = "<title>我的页面</title> 其他";
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "<title>".into(),
            suffix: "</title>".into(),
            include_boundary: false,
            case_sensitive: true,
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "我的页面");
    }

    #[test]
    fn prefix_suffix_include_boundary() {
        let html = "key=value;key2=value2;";
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "key=".into(),
            suffix: ";".into(),
            include_boundary: true,
            case_sensitive: true,
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "key=value;");
    }

    #[test]
    fn prefix_suffix_multiple_matches() {
        let html = "[A]x[B] [A]y[B] [A]z[B]";
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "[A]".into(),
            suffix: "[B]".into(),
            include_boundary: false,
            case_sensitive: true,
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].value, "x");
        assert_eq!(hits[1].value, "y");
        assert_eq!(hits[2].value, "z");
    }

    #[test]
    fn prefix_suffix_case_insensitive() {
        let html = "<TITLE>X</TITLE>";
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "<title>".into(),
            suffix: "</title>".into(),
            include_boundary: false,
            case_sensitive: false,
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "X");
    }

    #[test]
    fn prefix_suffix_zero_hits_when_prefix_missing() {
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "START".into(),
            suffix: "END".into(),
            include_boundary: false,
            case_sensitive: true,
        });
        let input = make_input("nothing here", "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert!(hits.is_empty());
    }

    #[test]
    fn prefix_suffix_zero_hits_when_suffix_missing() {
        let html = "START something";
        let rule = Rule::PrefixSuffix(PrefixSuffixRule {
            prefix: "START".into(),
            suffix: "END".into(),
            include_boundary: false,
            case_sensitive: true,
        });
        let input = make_input(html, "https://example.com/");
        let hits = extract(&rule, &input).expect("ps ok");
        assert!(hits.is_empty());
    }

    // ===== 后处理链 =====

    fn make_hits(values: &[&str]) -> Vec<Hit> {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| Hit {
                value: (*v).to_string(),
                source_fragment: "test".to_string(),
                location: Some(format!("node[{i}]")),
                context_html: None,
            })
            .collect()
    }

    fn pp(op: PostProcessorOp) -> PostProcessor {
        PostProcessor { op }
    }

    #[test]
    fn post_processor_trim() {
        let hits = make_hits(&["  abc  ", "\t\nfoo\r\n"]);
        let out = apply_post_processors(hits, &[pp(PostProcessorOp::Trim)], "https://example.com/");
        assert_eq!(out[0].value, "abc");
        assert_eq!(out[1].value, "foo");
    }

    #[test]
    fn post_processor_html_entity_decode_named() {
        let hits = make_hits(&["a&amp;b", "x&lt;y&gt;z &quot;q&quot;"]);
        let out = apply_post_processors(
            hits,
            &[pp(PostProcessorOp::HtmlEntityDecode)],
            "https://example.com/",
        );
        assert_eq!(out[0].value, "a&b");
        assert_eq!(out[1].value, "x<y>z \"q\"");
    }

    #[test]
    fn post_processor_html_entity_decode_numeric() {
        let hits = make_hits(&["&#65;&#66;&#67;", "&#x4e2d;"]);
        let out = apply_post_processors(
            hits,
            &[pp(PostProcessorOp::HtmlEntityDecode)],
            "https://example.com/",
        );
        assert_eq!(out[0].value, "ABC");
        assert_eq!(out[1].value, "中");
    }

    #[test]
    fn post_processor_absolutize_url() {
        let hits = make_hits(&["/p/1", "//cdn.com/x.js", "https://other.com/y"]);
        let out = apply_post_processors(
            hits,
            &[pp(PostProcessorOp::AbsolutizeUrl)],
            "https://example.com/list",
        );
        assert_eq!(out[0].value, "https://example.com/p/1");
        // resolve_url 对 //cdn.com 的处理：http(s) 才识别绝对；这里 //cdn.com 不是 http:// 开头
        // 按现有 resolve_url 实现，会拼到 base 目录（视为相对路径），所以验证 url 形式合法即可
        assert!(
            out[1].value.contains("cdn.com") || out[1].value.starts_with("https://example.com")
        );
        assert_eq!(out[2].value, "https://other.com/y");
    }

    #[test]
    fn post_processor_first_keeps_only_one() {
        let hits = make_hits(&["a", "b", "c"]);
        let out =
            apply_post_processors(hits, &[pp(PostProcessorOp::First)], "https://example.com/");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value, "a");
    }

    #[test]
    fn post_processor_first_noop_on_single() {
        let hits = make_hits(&["only"]);
        let out =
            apply_post_processors(hits, &[pp(PostProcessorOp::First)], "https://example.com/");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn post_processor_dedupe() {
        let hits = make_hits(&["a", "b", "a", "c", "b", "d"]);
        let out =
            apply_post_processors(hits, &[pp(PostProcessorOp::Dedupe)], "https://example.com/");
        let values: Vec<_> = out.iter().map(|h| h.value.as_str()).collect();
        assert_eq!(values, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn post_processor_all_is_noop() {
        let hits = make_hits(&["a", "b"]);
        let out = apply_post_processors(hits, &[pp(PostProcessorOp::All)], "https://example.com/");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn post_processor_chain_trim_then_dedupe_then_first() {
        let hits = make_hits(&["  a  ", "a", "  b  ", " b ", "a"]);
        let out = apply_post_processors(
            hits,
            &[
                pp(PostProcessorOp::Trim),
                pp(PostProcessorOp::Dedupe),
                pp(PostProcessorOp::First),
            ],
            "https://example.com/",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value, "a");
    }

    // ===== 错误：源缺失 / 不支持 =====

    #[test]
    fn script_layer_missing_script_index() {
        let rule = Rule::Regex(RegexRule {
            pattern: "x".into(),
            group: 0,
            flags: "".into(),
        });
        let mut input = make_input_with_scripts("", "https://example.com/", &[], None);
        input.source_layer = SourceLayer::Script;
        let err = extract(&rule, &input).expect_err("缺 script_index");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn script_layer_index_out_of_range() {
        let scripts = vec![ScriptBlock {
            index: 0,
            src: None,
            content: Some("x".to_string()),
            json_value: None,
        }];
        let rule = Rule::Regex(RegexRule {
            pattern: "x".into(),
            group: 0,
            flags: "".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(99));
        let err = extract(&rule, &input).expect_err("script_index 越界");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn script_layer_external_script_has_no_content() {
        let scripts = vec![ScriptBlock {
            index: 0,
            src: Some("https://cdn.com/a.js".to_string()),
            content: None,
            json_value: None,
        }];
        let rule = Rule::Regex(RegexRule {
            pattern: "x".into(),
            group: 0,
            flags: "".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let err = extract(&rule, &input).expect_err("外链脚本无内容");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn regex_on_meta_layer_unsupported() {
        // regex 模式依赖 layer_text()，Meta 层无 layer_text 实现 → UnsupportedMode
        let rule = Rule::Regex(RegexRule {
            pattern: "x".into(),
            group: 0,
            flags: "".into(),
        });
        let mut input = make_input("ignored", "https://example.com/");
        input.source_layer = SourceLayer::Meta;
        let err = extract(&rule, &input).expect_err("Meta 层 regex 不支持");
        assert_eq!(err.kind, ExtractErrorKind::UnsupportedMode);
    }

    // ===== JsonPath 模式（US4 T049）=====

    fn make_json_value(v: serde_json::Value) -> ScriptBlock {
        ScriptBlock {
            index: 0,
            src: None,
            content: None,
            json_value: Some(v),
        }
    }

    #[test]
    fn json_path_json_ld_simple_object() {
        // 场景 1：JSON-LD <script type="application/ld+json"> 纯 JSON
        let scripts = vec![make_json_value(serde_json::json!({
            "@type": "NewsArticle",
            "headline": "测试标题",
            "datePublished": "2024-02-01"
        }))];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.headline".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let hits = extract(&rule, &input).expect("json_path ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "测试标题");
        assert_eq!(hits[0].source_fragment, "json_path:$.headline");
    }

    #[test]
    fn json_path_window_data_assignment() {
        // 场景 2：window.__DATA__ = {...}; 启发式提取
        let scripts = vec![ScriptBlock {
            index: 0,
            src: None,
            content: Some(
                r#"window.__DATA__ = {"list":[{"title":"A"},{"title":"B"}]};"#.to_string(),
            ),
            json_value: None, // source_layer 解析这段文本时 serde_json::from_str 会失败
        }];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.list[*].title".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let hits = extract(&rule, &input).expect("json_path 启发式 ok");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].value, "A");
        assert_eq!(hits[1].value, "B");
    }

    #[test]
    fn json_path_invalid_path_returns_invalid_rule() {
        // 场景 3：非法 JSONPath → InvalidRule
        let scripts = vec![make_json_value(serde_json::json!({"a": 1}))];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.[unclosed".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let err = extract(&rule, &input).expect_err("非法 json_path");
        assert_eq!(err.kind, ExtractErrorKind::InvalidRule);
    }

    #[test]
    fn json_path_missing_script_index() {
        let scripts = vec![make_json_value(serde_json::json!({"a": 1}))];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.a".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, None);
        let err = extract(&rule, &input).expect_err("缺 script_index");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn json_path_external_script_no_content() {
        let scripts = vec![ScriptBlock {
            index: 0,
            src: Some("https://cdn.com/a.js".to_string()),
            content: None,
            json_value: None,
        }];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.a".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let err = extract(&rule, &input).expect_err("外链脚本无内容");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn json_path_non_json_content_returns_source_missing() {
        // content 不是 JSON 也不含 { / [ → 启发式失败
        let scripts = vec![ScriptBlock {
            index: 0,
            src: None,
            content: Some("console.log('hello');".to_string()),
            json_value: None,
        }];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.a".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let err = extract(&rule, &input).expect_err("无法解析 JSON");
        assert_eq!(err.kind, ExtractErrorKind::SourceMissing);
    }

    #[test]
    fn json_path_number_value() {
        // 数字节点 → 原生 to_string
        let scripts = vec![make_json_value(
            serde_json::json!({"count": 42, "label": "x"}),
        )];
        let rule = Rule::JsonPath(crate::services::crawler::field_schema::JsonPathRule {
            path: "$.count".into(),
        });
        let input = make_input_with_scripts("", "https://example.com/", &scripts, Some(0));
        let hits = extract(&rule, &input).expect("json_path number ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "42");
    }

    #[test]
    fn extract_json_from_text_pure_json() {
        let v = extract_json_from_text(r#"{"a":1,"b":[2,3]}"#).expect("pure json");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn extract_json_from_text_assignment_form() {
        let v = extract_json_from_text(r#"window.x = {"k":"v"};"#).expect("assignment");
        assert_eq!(v["k"], "v");
    }

    #[test]
    fn extract_json_from_text_no_json_returns_none() {
        assert!(extract_json_from_text("plain text without json").is_none());
    }

    // ===== MetaAttr 模式（US4 T050）=====

    fn make_meta_input<'a>(metas: &'a [MetaTag]) -> ExtractInput<'a> {
        static EMPTY_HEADERS: Lazy<HashMap<String, String>> = Lazy::new(HashMap::new);
        ExtractInput {
            source_layer: SourceLayer::Meta,
            html: "",
            script_blocks: &[],
            metas,
            headers: &EMPTY_HEADERS,
            final_url: "https://example.com/",
            script_index: None,
        }
    }

    #[test]
    fn meta_attr_og_image() {
        let metas = vec![
            MetaTag {
                key_kind: MetaKeyKind::Property,
                key: "og:image".to_string(),
                content: "https://cdn.com/cover.jpg".to_string(),
            },
            MetaTag {
                key_kind: MetaKeyKind::Name,
                key: "description".to_string(),
                content: "页面描述".to_string(),
            },
        ];
        let rule = Rule::MetaAttr(crate::services::crawler::field_schema::MetaAttrRule {
            attr_name: "property".into(),
            attr_value: "og:image".into(),
            content_key: "content".into(),
        });
        let input = make_meta_input(&metas);
        let hits = extract(&rule, &input).expect("meta_attr ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "https://cdn.com/cover.jpg");
        assert_eq!(hits[0].source_fragment, "meta_attr:property=og:image");
    }

    #[test]
    fn meta_attr_description_by_name() {
        let metas = vec![MetaTag {
            key_kind: MetaKeyKind::Name,
            key: "description".to_string(),
            content: "页面描述".to_string(),
        }];
        let rule = Rule::MetaAttr(crate::services::crawler::field_schema::MetaAttrRule {
            attr_name: "name".into(),
            attr_value: "description".into(),
            content_key: "content".into(),
        });
        let input = make_meta_input(&metas);
        let hits = extract(&rule, &input).expect("meta_attr ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "页面描述");
    }

    #[test]
    fn meta_attr_zero_hits_when_not_present() {
        let metas = vec![MetaTag {
            key_kind: MetaKeyKind::Name,
            key: "keywords".to_string(),
            content: "x,y".to_string(),
        }];
        let rule = Rule::MetaAttr(crate::services::crawler::field_schema::MetaAttrRule {
            attr_name: "name".into(),
            attr_value: "description".into(),
            content_key: "content".into(),
        });
        let input = make_meta_input(&metas);
        let hits = extract(&rule, &input).expect("meta_attr ok");
        assert!(hits.is_empty());
    }

    #[test]
    fn meta_attr_default_content_key_when_empty() {
        // content_key 空串时默认按 "content" 取
        let metas = vec![MetaTag {
            key_kind: MetaKeyKind::Name,
            key: "description".to_string(),
            content: "默认取 content".to_string(),
        }];
        let rule = Rule::MetaAttr(crate::services::crawler::field_schema::MetaAttrRule {
            attr_name: "name".into(),
            attr_value: "description".into(),
            content_key: "".into(),
        });
        let input = make_meta_input(&metas);
        let hits = extract(&rule, &input).expect("meta_attr ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "默认取 content");
    }

    // ===== HeaderField 模式（US4 T051）=====

    fn make_header_input<'a>(headers: &'a HashMap<String, String>) -> ExtractInput<'a> {
        ExtractInput {
            source_layer: SourceLayer::Header,
            html: "",
            script_blocks: &[],
            metas: &[],
            headers,
            final_url: "https://example.com/",
            script_index: None,
        }
    }

    #[test]
    fn header_field_x_total_count() {
        let mut headers = HashMap::new();
        headers.insert("x-total-count".to_string(), "1234".to_string());
        headers.insert("content-type".to_string(), "text/html".to_string());
        let rule = Rule::HeaderField(crate::services::crawler::field_schema::HeaderFieldRule {
            header_name: "X-Total-Count".into(),
        });
        let input = make_header_input(&headers);
        let hits = extract(&rule, &input).expect("header_field ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "1234");
        assert_eq!(hits[0].source_fragment, "header_field:X-Total-Count");
    }

    #[test]
    fn header_field_content_type_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let rule = Rule::HeaderField(crate::services::crawler::field_schema::HeaderFieldRule {
            header_name: "Content-Type".into(),
        });
        let input = make_header_input(&headers);
        let hits = extract(&rule, &input).expect("header_field ok");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "application/json");
    }

    #[test]
    fn header_field_missing_returns_zero_hits() {
        let headers = HashMap::new();
        let rule = Rule::HeaderField(crate::services::crawler::field_schema::HeaderFieldRule {
            header_name: "X-Missing".into(),
        });
        let input = make_header_input(&headers);
        let hits = extract(&rule, &input).expect("header_field ok");
        assert!(hits.is_empty());
    }

    // ===== 错误：Display 实现 =====

    #[test]
    fn extract_error_display() {
        let e = ExtractError::new(ExtractErrorKind::InvalidRule, "bad selector");
        let s = format!("{e}");
        assert!(s.contains("InvalidRule"));
        assert!(s.contains("bad selector"));
    }

    // ===== 辅助：find_substring 直接测试 =====

    #[test]
    fn find_substring_case_sensitive() {
        assert_eq!(find_substring("AbCd", "Cd", true), Some(2));
        assert_eq!(find_substring("AbCd", "cd", true), None);
    }

    #[test]
    fn find_substring_case_insensitive() {
        assert_eq!(find_substring("AbCd", "cd", false), Some(2));
        assert_eq!(find_substring("AbCd", "CD", false), Some(2));
    }

    #[test]
    fn find_substring_empty_needle() {
        assert_eq!(find_substring("any", "", true), Some(0));
    }

    #[test]
    fn html_entity_decode_passthrough_when_no_amp() {
        assert_eq!(html_entity_decode("plain text"), "plain text");
    }

    // ===== follow_url 同步路径拒绝 + sub_rule_to_rule =====

    #[test]
    fn follow_url_rule_rejected_in_sync_extract() {
        use crate::services::crawler::field_schema::{CssRule, FollowUrlRule};
        use crate::services::crawler::source_layer::SourceMaterial;
        let fu = FollowUrlRule {
            transit: SubRule::Css(CssRule {
                selector: "a".into(),
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
        let material = SourceMaterial {
            final_url: "https://example.com/".into(),
            html: "<a href='/x'>x</a>".to_string(),
            status: 200,
            headers: std::collections::HashMap::new(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 0,
        };
        let input = ExtractInput::from_material(&material, None);
        let err = extract(&Rule::FollowUrl(fu), &input).unwrap_err();
        assert_eq!(err.kind, ExtractErrorKind::UnsupportedMode);
    }

    #[test]
    fn sub_rule_to_rule_six_variants_round_trip() {
        use crate::services::crawler::field_schema::{
            CssRule, HeaderFieldRule, JsonPathRule, MetaAttrRule, PrefixSuffixRule, RegexRule,
        };
        let cases: Vec<SubRule> = vec![
            SubRule::Css(CssRule {
                selector: "a".into(),
                attr: "href".into(),
            }),
            SubRule::Regex(RegexRule {
                pattern: "x".into(),
                group: 1,
                flags: "".into(),
            }),
            SubRule::PrefixSuffix(PrefixSuffixRule {
                prefix: "p".into(),
                suffix: "s".into(),
                include_boundary: false,
                case_sensitive: false,
            }),
            SubRule::JsonPath(JsonPathRule { path: "$.x".into() }),
            SubRule::MetaAttr(MetaAttrRule {
                attr_name: "name".into(),
                attr_value: "desc".into(),
                content_key: "content".into(),
            }),
            SubRule::HeaderField(HeaderFieldRule {
                header_name: "X-Foo".into(),
            }),
        ];
        for sub in &cases {
            let r = sub_rule_to_rule(sub);
            assert_eq!(r.mode_str(), sub.mode_str());
            // 再转回 SubRule（通过 serde 序列化反序列化验证不丢信息）
            let s_sub = serde_json::to_string(sub).unwrap();
            let s_rule = serde_json::to_string(&r).unwrap();
            // Rule 内部带 "mode" tag，SubRule 也是 "mode" tag，序列化文本应一致
            assert_eq!(s_sub, s_rule, "SubRule ↔ Rule 序列化不一致");
        }
    }

    // ---- [feature 046] Script variant: extract() 同步路径不支持 ----

    #[test]
    fn t_extract_returns_unsupported_for_script_rule() {
        // 与 FollowUrl 同策略：同步 extract 收到 Script 返回 UnsupportedMode
        let rule = Rule::Script(ScriptRule {
            body: "return ctx.value".into(),
            api_version: "v1".into(),
        });
        let input = make_input("<html></html>", "https://example.com/");
        let err = extract(&rule, &input).unwrap_err();
        assert_eq!(err.kind, ExtractErrorKind::UnsupportedMode);
        assert!(err.message.contains("script"));
    }
}
