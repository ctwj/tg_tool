//! 字段树 Schema 与校验（feature 043-crawler-configurator）
//!
//! 定义字段配置器核心枚举（Scope / FieldType / SourceLayer / ExtractorMode）、
//! 6 模式 `Rule` 变体、`PostProcessor` 后处理链、`FieldNodeSpec` 与 `FieldTree`，
//! 提供 `validate_name()` / `validate_rule()` / `deserialize_rule()`。
//!
//! 对照 data-model.md E3 / E7 与 research.md R1。

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// 枚举（data-model.md E7）
// ============================================================================

/// 字段作用域：列表页 / 详情页
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    ListPage,
    DetailPage,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::ListPage => "list_page",
            Scope::DetailPage => "detail_page",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "list_page" => Some(Scope::ListPage),
            "detail_page" => Some(Scope::DetailPage),
            _ => None,
        }
    }
}

/// 字段类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Text,
    Url,
    Image,
    Number,
    Datetime,
    LinkCard,
    Pagination,
    Custom,
}

impl FieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Text => "text",
            FieldType::Url => "url",
            FieldType::Image => "image",
            FieldType::Number => "number",
            FieldType::Datetime => "datetime",
            FieldType::LinkCard => "link_card",
            FieldType::Pagination => "pagination",
            FieldType::Custom => "custom",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "string" => Some(FieldType::String),
            "text" => Some(FieldType::Text),
            "url" => Some(FieldType::Url),
            "image" => Some(FieldType::Image),
            "number" => Some(FieldType::Number),
            "datetime" => Some(FieldType::Datetime),
            "link_card" => Some(FieldType::LinkCard),
            "pagination" => Some(FieldType::Pagination),
            "custom" => Some(FieldType::Custom),
            _ => None,
        }
    }
}

/// 来源层：源码 tab 一致
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLayer {
    Html,
    Header,
    Script,
    Meta,
    Url,
}

impl SourceLayer {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceLayer::Html => "html",
            SourceLayer::Header => "header",
            SourceLayer::Script => "script",
            SourceLayer::Meta => "meta",
            SourceLayer::Url => "url",
        }
    }

    /// FollowUrlRule 中 `transit_layer` / `target_layer` 的 serde 默认值（Html）
    pub fn default_html() -> Self {
        SourceLayer::Html
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "html" => Some(SourceLayer::Html),
            "header" => Some(SourceLayer::Header),
            "script" => Some(SourceLayer::Script),
            "meta" => Some(SourceLayer::Meta),
            "url" => Some(SourceLayer::Url),
            _ => None,
        }
    }
}

/// 匹配模式：8 种（6 同步模式 + follow_url 异步两阶段 + script JS 沙箱求值）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractorMode {
    Css,
    Regex,
    PrefixSuffix,
    JsonPath,
    MetaAttr,
    HeaderField,
    /// 跟随 URL 两阶段：先抓中转 URL → 请求该 URL → 在响应上提取。
    /// 需 async 调用层（probe/engine）支持，extractor 同步路径会返回 UnsupportedMode。
    FollowUrl,
    /// [feature 046] JS 沙箱求值：把 `ScriptRule.body` 包装为
    /// `async function(__ctx) { ${body} }` 在 rquickjs 中执行，
    /// 注入 `ctx.value/fields/url/fetch`，返回字符串。
    /// 仅 detail_page 作用域合法（FR-020），单字段失败不中断。
    Script,
}

impl ExtractorMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtractorMode::Css => "css",
            ExtractorMode::Regex => "regex",
            ExtractorMode::PrefixSuffix => "prefix_suffix",
            ExtractorMode::JsonPath => "json_path",
            ExtractorMode::MetaAttr => "meta_attr",
            ExtractorMode::HeaderField => "header_field",
            ExtractorMode::FollowUrl => "follow_url",
            ExtractorMode::Script => "script",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "css" => Some(ExtractorMode::Css),
            "regex" => Some(ExtractorMode::Regex),
            "prefix_suffix" => Some(ExtractorMode::PrefixSuffix),
            "json_path" => Some(ExtractorMode::JsonPath),
            "meta_attr" => Some(ExtractorMode::MetaAttr),
            "header_field" => Some(ExtractorMode::HeaderField),
            "follow_url" => Some(ExtractorMode::FollowUrl),
            "script" => Some(ExtractorMode::Script),
            _ => None,
        }
    }
}

// ============================================================================
// Rule（6 模式）— 每个变体对应一种 extractor_mode
// ============================================================================

/// CSS 选择器模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CssRule {
    /// CSS 选择器（必填）
    pub selector: String,
    /// 取节点的属性：`text` / `html` / 任意属性名（如 href/src），默认 text
    #[serde(default = "default_css_attr")]
    pub attr: String,
}

fn default_css_attr() -> String {
    "text".to_string()
}

/// 正则模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegexRule {
    /// 正则 pattern（必填）
    pub pattern: String,
    /// 捕获组序号（0=整体匹配，1=第一个捕获组）
    #[serde(default = "default_regex_group")]
    pub group: u32,
    /// 正则 flags（如 "i" / "m" / "s"）
    #[serde(default)]
    pub flags: String,
}

fn default_regex_group() -> u32 {
    1
}

/// 前后缀定位模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrefixSuffixRule {
    pub prefix: String,
    pub suffix: String,
    #[serde(default)]
    pub include_boundary: bool,
    #[serde(default)]
    pub case_sensitive: bool,
}

/// JSON Path 模式（依赖 source_layer=script + script_index）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonPathRule {
    /// RFC 9535 JSONPath 表达式，如 `$.name` / `$.data.list[*].title`
    pub path: String,
}

/// `<meta>` 属性定位模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetaAttrRule {
    /// `<meta>` 标签的筛选属性名（name / property / http-equiv 等）
    pub attr_name: String,
    /// 筛选属性的值（如 description / og:title）
    pub attr_value: String,
    /// 取哪个属性的内容（默认 content）
    #[serde(default = "default_meta_content_key")]
    pub content_key: String,
}

fn default_meta_content_key() -> String {
    "content".to_string()
}

/// HTTP 响应头字段提取模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeaderFieldRule {
    /// HTTP 响应头名（大小写不敏感），如 X-Total-Count / Content-Type
    pub header_name: String,
}

/// [feature 046] JS 沙箱脚本规则（data-model.md E6 / contracts/script-runtime.md）
///
/// `body` 是一段 JS 函数体，被包装为 `async function(__ctx) { ${body} }`，
/// 在 rquickjs 沙箱中执行；通过 `ctx.value` / `ctx.fields` / `ctx.url` / `ctx.fetch`
/// 访问上游提取值与请求能力，最终 `return` 字符串。
///
/// 校验约束（FR-020）：
/// - body 非空（trim 后仍非空）
/// - body ≤ 64 KB（`crawler_script_max_body_size` 默认上限）
/// - api_version 当前固定 "v1"，未来向后兼容时新增枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRule {
    /// JS 函数体（不含外层 `function(ctx) { ... }` 包裹）
    pub body: String,
    /// API 版本，默认 "v1"
    #[serde(default = "default_script_api_version")]
    pub api_version: String,
}

impl Default for ScriptRule {
    fn default() -> Self {
        Self {
            body: String::new(),
            api_version: default_script_api_version(),
        }
    }
}

fn default_script_api_version() -> String {
    "v1".to_string()
}

/// 8 模式 Rule 联合（6 同步 + follow_url 异步两阶段 + script JS 沙箱；discriminated by `mode`，但实际持久化按 mode_json 分别解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", content = "spec", rename_all = "snake_case")]
pub enum Rule {
    Css(CssRule),
    Regex(RegexRule),
    PrefixSuffix(PrefixSuffixRule),
    JsonPath(JsonPathRule),
    MetaAttr(MetaAttrRule),
    HeaderField(HeaderFieldRule),
    /// 跟随 URL 两阶段提取：transit 子规则抓中转 URL → fetch → extract 子规则抓最终值
    FollowUrl(FollowUrlRule),
    /// [feature 046] JS 沙箱脚本：body 在 rquickjs 中求值
    Script(ScriptRule),
}

impl Rule {
    /// 对应的 extractor_mode 字符串
    pub fn mode_str(&self) -> &'static str {
        match self {
            Rule::Css(_) => "css",
            Rule::Regex(_) => "regex",
            Rule::PrefixSuffix(_) => "prefix_suffix",
            Rule::JsonPath(_) => "json_path",
            Rule::MetaAttr(_) => "meta_attr",
            Rule::HeaderField(_) => "header_field",
            Rule::FollowUrl(_) => "follow_url",
            Rule::Script(_) => "script",
        }
    }
}

// ============================================================================
// follow_url 专用子规则（禁止递归嵌套 follow_url，编译期保证）
// ============================================================================

/// follow_url 模式的子规则变体 —— 仅含 6 个同步模式，不含 FollowUrl，编译期杜绝无限递归。
///
/// 与 `Rule` 同构（少 FollowUrl 变体），内部直接复用 6 个 `pub` Rule 结构体定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", content = "spec", rename_all = "snake_case")]
pub enum SubRule {
    Css(CssRule),
    Regex(RegexRule),
    PrefixSuffix(PrefixSuffixRule),
    JsonPath(JsonPathRule),
    MetaAttr(MetaAttrRule),
    HeaderField(HeaderFieldRule),
}

impl SubRule {
    /// 对应的 extractor_mode 字符串
    pub fn mode_str(&self) -> &'static str {
        match self {
            SubRule::Css(_) => "css",
            SubRule::Regex(_) => "regex",
            SubRule::PrefixSuffix(_) => "prefix_suffix",
            SubRule::JsonPath(_) => "json_path",
            SubRule::MetaAttr(_) => "meta_attr",
            SubRule::HeaderField(_) => "header_field",
        }
    }

    /// 映射到 ExtractorMode（用于日志 / dispatch）
    pub fn to_extractor_mode(&self) -> ExtractorMode {
        match self {
            SubRule::Css(_) => ExtractorMode::Css,
            SubRule::Regex(_) => ExtractorMode::Regex,
            SubRule::PrefixSuffix(_) => ExtractorMode::PrefixSuffix,
            SubRule::JsonPath(_) => ExtractorMode::JsonPath,
            SubRule::MetaAttr(_) => ExtractorMode::MetaAttr,
            SubRule::HeaderField(_) => ExtractorMode::HeaderField,
        }
    }
}

/// follow_url 模式配置：中转 URL 子规则 + 二次请求后提取子规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUrlRule {
    /// 在当前 material 上提取中转 URL 的子规则（必填）
    pub transit: SubRule,
    /// transit 子规则作用的 source_layer（默认 Html）
    #[serde(default = "SourceLayer::default_html")]
    pub transit_layer: SourceLayer,
    /// source_layer=Script 时指定 script_index
    #[serde(default)]
    pub transit_script_index: Option<i32>,
    /// 二次请求后 extract 子规则作用的 source_layer（默认 Html）
    #[serde(default = "SourceLayer::default_html")]
    pub target_layer: SourceLayer,
    /// source_layer=Script 时指定 script_index
    #[serde(default)]
    pub target_script_index: Option<i32>,
    /// 在二次请求 material 上提取最终值的子规则（必填）
    pub extract: SubRule,
}

// ============================================================================
// PostProcessor 后处理链
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostProcessorOp {
    /// 去首尾空白
    Trim,
    /// HTML 实体解码（&amp; → &）
    HtmlEntityDecode,
    /// 相对 URL → 绝对 URL（依赖 base_url）
    AbsolutizeUrl,
    /// 取第一条
    First,
    /// 取全部（默认行为，显式标记）
    All,
    /// 去重
    Dedupe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostProcessor {
    pub op: PostProcessorOp,
}

// ============================================================================
// FieldNodeSpec / FieldTree
// ============================================================================

/// 字段节点规范（应用层中间表示，与 DB 行对应但解耦）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldNodeSpec {
    pub id: Option<i64>,
    pub task_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub scope: Scope,
    pub name: String,
    pub display_name: String,
    pub field_type: FieldType,
    pub source_layer: SourceLayer,
    pub extractor_mode: ExtractorMode,
    pub rule: Rule,
    #[serde(default)]
    pub post_processors: Vec<PostProcessor>,
    /// source_layer=Script 时指定的脚本块索引
    #[serde(default)]
    pub script_index: Option<i32>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true")]
    pub is_active: bool,
    /// [feature 046] 是否在消费性读取时按需刷新脚本字段（FR-019）。
    /// 仅 extractor_mode=Script 时允许为 true（`validate_field_node_spec` 拒绝其它模式）。
    /// 管理性读取（列表/详情/字段命中率面板）不受此字段影响，直接读库。
    #[serde(default)]
    pub refresh_on_read: bool,
}

fn default_true() -> bool {
    true
}

/// 字段树（应用层组装）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldTree {
    pub list_page: Vec<FieldTreeNode>,
    pub detail_page: Vec<FieldTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTreeNode {
    pub spec: FieldNodeSpec,
    pub children: Vec<FieldTreeNode>,
}

// ============================================================================
// 校验函数
// ============================================================================

/// name 合法性：`^[a-z][a-z0-9_]{0,31}$`（小写字母开头，后续字母/数字/下划线，总长 1-32）
static NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9_]{0,31}$").expect("invalid name regex"));

pub fn validate_name(name: &str) -> Result<(), String> {
    if NAME_RE.is_match(name) {
        Ok(())
    } else {
        Err(format!(
            "字段名 '{name}' 不合法（须匹配 ^[a-z][a-z0-9_]{{0,31}}$，1-32 字符，小写字母开头）"
        ))
    }
}

/// 校验给定 mode + rule_json 的一致性。
///
/// - 检查 JSON 可解析
/// - 检查必填字段非空（CSS selector / Regex pattern / PrefixSuffix prefix+suffix / JsonPath path /
///   MetaAttr attr_name+attr_value / HeaderField header_name / FollowUrl transit+extract 递归非空）
pub fn validate_rule(mode: ExtractorMode, rule_json: &str) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(rule_json).map_err(|e| format!("rule_json 不是合法 JSON: {e}"))?;
    match mode {
        ExtractorMode::Css => {
            let r: CssRule =
                serde_json::from_value(value).map_err(|e| format!("css 规则反序列化失败: {e}"))?;
            validate_css_rule(&r)
        }
        ExtractorMode::Regex => {
            let r: RegexRule = serde_json::from_value(value)
                .map_err(|e| format!("regex 规则反序列化失败: {e}"))?;
            validate_regex_rule(&r)
        }
        ExtractorMode::PrefixSuffix => {
            let r: PrefixSuffixRule = serde_json::from_value(value)
                .map_err(|e| format!("prefix_suffix 规则反序列化失败: {e}"))?;
            validate_prefix_suffix_rule(&r)
        }
        ExtractorMode::JsonPath => {
            let r: JsonPathRule = serde_json::from_value(value)
                .map_err(|e| format!("json_path 规则反序列化失败: {e}"))?;
            validate_json_path_rule(&r)
        }
        ExtractorMode::MetaAttr => {
            let r: MetaAttrRule = serde_json::from_value(value)
                .map_err(|e| format!("meta_attr 规则反序列化失败: {e}"))?;
            validate_meta_attr_rule(&r)
        }
        ExtractorMode::HeaderField => {
            let r: HeaderFieldRule = serde_json::from_value(value)
                .map_err(|e| format!("header_field 规则反序列化失败: {e}"))?;
            validate_header_field_rule(&r)
        }
        ExtractorMode::FollowUrl => {
            let r: FollowUrlRule = serde_json::from_value(value)
                .map_err(|e| format!("follow_url 规则反序列化失败: {e}"))?;
            validate_sub_rule(&r.transit, "follow_url.transit")?;
            validate_sub_rule(&r.extract, "follow_url.extract")?;
            // transit_layer=Script 必须有 transit_script_index
            if r.transit_layer == SourceLayer::Script && r.transit_script_index.is_none() {
                return Err(
                    "follow_url.transit_layer=script 时必须指定 transit_script_index".into(),
                );
            }
            if r.target_layer == SourceLayer::Script && r.target_script_index.is_none() {
                return Err("follow_url.target_layer=script 时必须指定 target_script_index".into());
            }
            Ok(())
        }
        ExtractorMode::Script => {
            let r: ScriptRule = serde_json::from_value(value)
                .map_err(|e| format!("script 规则反序列化失败: {e}"))?;
            validate_script_rule(&r)
        }
    }
}

/// [feature 046] 校验 ScriptRule（FR-020）：
/// - body trim 后非空
/// - body 长度 ≤ 64 KB（与 ScriptOpts::default::max_body_size 对齐）
/// - api_version 必须是 "v1"（未知版本拒绝，避免未来 silently 不兼容）
pub fn validate_script_rule(r: &ScriptRule) -> Result<(), String> {
    if r.body.trim().is_empty() {
        return Err("script.body 不能为空".into());
    }
    const MAX_BODY_BYTES: usize = 65_536;
    if r.body.len() > MAX_BODY_BYTES {
        return Err(format!(
            "script.body 长度 {} 超过上限 {} 字节",
            r.body.len(),
            MAX_BODY_BYTES
        ));
    }
    if r.api_version != "v1" {
        return Err(format!(
            "script.api_version 必须为 'v1'，实际 '{}'",
            r.api_version
        ));
    }
    Ok(())
}

/// [feature 046] 字段节点级交叉校验（FR-020）：
/// - scope=ListPage + extractor_mode=Script → 拒绝（脚本仅 detail 作用域合法）
/// - extractor_mode≠Script + refresh_on_read=true → 拒绝（refresh_on_read 仅脚本字段有意义）
///
/// 单独的 `validate_rule` 只校验单字段 rule，不感知 scope/refresh_on_read；
/// 本函数在字段树 CRUD 入口被调用（handler 层组装完 FieldNodeSpec 后调用）。
pub fn validate_field_node_spec(node: &FieldNodeSpec) -> Result<(), String> {
    if node.scope == Scope::ListPage && node.extractor_mode == ExtractorMode::Script {
        return Err("script 模式仅 detail_page 作用域合法（list_page 不支持脚本字段）".into());
    }
    if node.refresh_on_read && node.extractor_mode != ExtractorMode::Script {
        return Err(format!(
            "refresh_on_read=true 仅在 extractor_mode=script 时允许（当前 mode={}）",
            node.extractor_mode.as_str()
        ));
    }
    Ok(())
}

fn validate_css_rule(r: &CssRule) -> Result<(), String> {
    if r.selector.trim().is_empty() {
        return Err("css.selector 不能为空".into());
    }
    Ok(())
}

fn validate_regex_rule(r: &RegexRule) -> Result<(), String> {
    if r.pattern.is_empty() {
        return Err("regex.pattern 不能为空".into());
    }
    compile_regex(&r.pattern, &r.flags).map_err(|e| format!("regex.pattern 编译失败: {e}"))?;
    Ok(())
}

fn validate_prefix_suffix_rule(r: &PrefixSuffixRule) -> Result<(), String> {
    if r.prefix.is_empty() {
        return Err("prefix_suffix.prefix 不能为空".into());
    }
    if r.suffix.is_empty() {
        return Err("prefix_suffix.suffix 不能为空".into());
    }
    Ok(())
}

fn validate_json_path_rule(r: &JsonPathRule) -> Result<(), String> {
    if r.path.trim().is_empty() {
        return Err("json_path.path 不能为空".into());
    }
    if !r.path.starts_with('$') {
        return Err("json_path.path 须以 $ 开头（RFC 9535）".into());
    }
    Ok(())
}

fn validate_meta_attr_rule(r: &MetaAttrRule) -> Result<(), String> {
    if r.attr_name.trim().is_empty() {
        return Err("meta_attr.attr_name 不能为空".into());
    }
    if r.attr_value.trim().is_empty() {
        return Err("meta_attr.attr_value 不能为空".into());
    }
    Ok(())
}

fn validate_header_field_rule(r: &HeaderFieldRule) -> Result<(), String> {
    if r.header_name.trim().is_empty() {
        return Err("header_field.header_name 不能为空".into());
    }
    Ok(())
}

/// 校验 SubRule（follow_url 内嵌子规则）：按 mode 检查必填字段非空
fn validate_sub_rule(sub: &SubRule, path: &str) -> Result<(), String> {
    match sub {
        SubRule::Css(r) => validate_css_rule(r).map_err(|e| format!("{path}: {e}")),
        SubRule::Regex(r) => validate_regex_rule(r).map_err(|e| format!("{path}: {e}")),
        SubRule::PrefixSuffix(r) => {
            validate_prefix_suffix_rule(r).map_err(|e| format!("{path}: {e}"))
        }
        SubRule::JsonPath(r) => validate_json_path_rule(r).map_err(|e| format!("{path}: {e}")),
        SubRule::MetaAttr(r) => validate_meta_attr_rule(r).map_err(|e| format!("{path}: {e}")),
        SubRule::HeaderField(r) => {
            validate_header_field_rule(r).map_err(|e| format!("{path}: {e}"))
        }
    }
}

/// 把 DB 中的 (mode, rule_json) 反序列化为 [`Rule::Css`] 等（用于应用层 dispatch）
pub fn deserialize_rule(mode: ExtractorMode, rule_json: &str) -> Result<Rule, String> {
    let value: Value =
        serde_json::from_str(rule_json).map_err(|e| format!("rule_json 不是合法 JSON: {e}"))?;
    Ok(match mode {
        ExtractorMode::Css => {
            Rule::Css(serde_json::from_value(value).map_err(|e| format!("css 反序列化失败: {e}"))?)
        }
        ExtractorMode::Regex => Rule::Regex(
            serde_json::from_value(value).map_err(|e| format!("regex 反序列化失败: {e}"))?,
        ),
        ExtractorMode::PrefixSuffix => Rule::PrefixSuffix(
            serde_json::from_value(value)
                .map_err(|e| format!("prefix_suffix 反序列化失败: {e}"))?,
        ),
        ExtractorMode::JsonPath => Rule::JsonPath(
            serde_json::from_value(value).map_err(|e| format!("json_path 反序列化失败: {e}"))?,
        ),
        ExtractorMode::MetaAttr => Rule::MetaAttr(
            serde_json::from_value(value).map_err(|e| format!("meta_attr 反序列化失败: {e}"))?,
        ),
        ExtractorMode::HeaderField => Rule::HeaderField(
            serde_json::from_value(value).map_err(|e| format!("header_field 反序列化失败: {e}"))?,
        ),
        ExtractorMode::FollowUrl => Rule::FollowUrl(
            serde_json::from_value(value).map_err(|e| format!("follow_url 反序列化失败: {e}"))?,
        ),
        ExtractorMode::Script => Rule::Script(
            serde_json::from_value(value).map_err(|e| format!("script 反序列化失败: {e}"))?,
        ),
    })
}

/// 把 Rule 序列化为 (mode_str, rule_json) 二元组（持久化用）
///
/// Rule 的 serde 配置是 `tag = "mode", content = "spec"`，因此整体序列化输出形如
/// `{"mode":"css","spec":{"selector":".x","attr":"text"}}`。落库时我们只保留 spec 内层
/// 对象（`{"selector":".x","attr":"text"}`）作为 rule_json，mode 单独入 extractor_mode 列。
pub fn serialize_rule(rule: &Rule) -> (String, String) {
    let mode = rule.mode_str().to_string();
    let json = serde_json::to_string(rule).unwrap_or_else(|_| "{}".to_string());
    let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
    // 取 "spec" key 下的内容（不是 .values().next()，那会拿到 "mode" 字符串）
    let inner = value.get("spec").cloned().unwrap_or(Value::Null);
    (
        mode,
        serde_json::to_string(&inner).unwrap_or_else(|_| "{}".to_string()),
    )
}

/// 编译 regex —— 把 pattern 与 flags 拼接为 `(?flags:pattern)`
pub fn compile_regex(pattern: &str, flags: &str) -> Result<regex::Regex, regex::Error> {
    if flags.is_empty() {
        regex::Regex::new(pattern)
    } else {
        // 转换常见 flags：i/m/s/x → 内联 (?i)(?m)(?s)(?x)
        let mut inline = String::new();
        for ch in flags.chars() {
            match ch {
                'i' | 'm' | 's' | 'x' | 'U' => {
                    inline.push(ch);
                }
                _ => {}
            }
        }
        let assembled = format!("(?{inline}){pattern}");
        regex::Regex::new(&assembled)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- validate_name ----

    #[test]
    fn t_name_legal_simple() {
        assert!(validate_name("title").is_ok());
        assert!(validate_name("cover").is_ok());
    }

    #[test]
    fn t_name_legal_with_underscore_and_digits() {
        assert!(validate_name("cover_url").is_ok());
        assert!(validate_name("link_card_2").is_ok());
        assert!(validate_name("a1").is_ok());
    }

    #[test]
    fn t_name_legal_max_length_32() {
        let name = "a".to_string() + &"_b".repeat(15); // 1 + 30 = 31 chars
        assert_eq!(name.len(), 31);
        assert!(validate_name(&name).is_ok());

        let name32 = "a".to_string() + &"_b".repeat(15) + "c"; // 32 chars
        assert_eq!(name32.len(), 32);
        assert!(validate_name(&name32).is_ok());
    }

    #[test]
    fn t_name_illegal_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn t_name_illegal_uppercase() {
        assert!(validate_name("Title").is_err());
        assert!(validate_name("ABC").is_err());
    }

    #[test]
    fn t_name_illegal_starts_with_digit_or_underscore() {
        assert!(validate_name("1title").is_err());
        assert!(validate_name("_title").is_err());
        assert!(validate_name("-title").is_err());
    }

    #[test]
    fn t_name_illegal_too_long() {
        let name = "a".to_string() + &"b".repeat(32); // 33 chars
        assert_eq!(name.len(), 33);
        assert!(validate_name(&name).is_err());
    }

    #[test]
    fn t_name_illegal_dash_or_space() {
        assert!(validate_name("link-card").is_err());
        assert!(validate_name("link card").is_err());
    }

    // ---- validate_rule: css ----

    #[test]
    fn t_rule_css_legal() {
        let json = r#"{"selector":".post-title","attr":"text"}"#;
        assert!(validate_rule(ExtractorMode::Css, json).is_ok());
    }

    #[test]
    fn t_rule_css_legal_default_attr() {
        let json = r#"{"selector":".post-title"}"#;
        assert!(validate_rule(ExtractorMode::Css, json).is_ok());
    }

    #[test]
    fn t_rule_css_illegal_empty_selector() {
        let json = r#"{"selector":""}"#;
        assert!(validate_rule(ExtractorMode::Css, json).is_err());
    }

    #[test]
    fn t_rule_css_illegal_json() {
        assert!(validate_rule(ExtractorMode::Css, "not-json").is_err());
    }

    // ---- validate_rule: regex ----

    #[test]
    fn t_rule_regex_legal() {
        let json = r#"{"pattern":"发布时间：(.+?)$","group":1,"flags":""}"#;
        assert!(validate_rule(ExtractorMode::Regex, json).is_ok());
    }

    #[test]
    fn t_rule_regex_legal_with_flags() {
        let json = r#"{"pattern":"view","group":0,"flags":"i"}"#;
        assert!(validate_rule(ExtractorMode::Regex, json).is_ok());
    }

    #[test]
    fn t_rule_regex_illegal_empty_pattern() {
        let json = r#"{"pattern":""}"#;
        assert!(validate_rule(ExtractorMode::Regex, json).is_err());
    }

    #[test]
    fn t_rule_regex_illegal_syntax() {
        let json = r#"{"pattern":"(unclosed"}"#;
        assert!(validate_rule(ExtractorMode::Regex, json).is_err());
    }

    // ---- validate_rule: prefix_suffix ----

    #[test]
    fn t_rule_prefix_suffix_legal() {
        let json = r#"{"prefix":"<title>","suffix":"</title>","include_boundary":false}"#;
        assert!(validate_rule(ExtractorMode::PrefixSuffix, json).is_ok());
    }

    #[test]
    fn t_rule_prefix_suffix_illegal_empty_prefix() {
        let json = r#"{"prefix":"","suffix":"</title>"}"#;
        assert!(validate_rule(ExtractorMode::PrefixSuffix, json).is_err());
    }

    #[test]
    fn t_rule_prefix_suffix_illegal_empty_suffix() {
        let json = r#"{"prefix":"<title>","suffix":""}"#;
        assert!(validate_rule(ExtractorMode::PrefixSuffix, json).is_err());
    }

    // ---- validate_rule: json_path ----

    #[test]
    fn t_rule_json_path_legal() {
        let json = r#"{"path":"$.name"}"#;
        assert!(validate_rule(ExtractorMode::JsonPath, json).is_ok());
    }

    #[test]
    fn t_rule_json_path_illegal_no_dollar() {
        let json = r#"{"path":"name"}"#;
        assert!(validate_rule(ExtractorMode::JsonPath, json).is_err());
    }

    #[test]
    fn t_rule_json_path_illegal_empty() {
        let json = r#"{"path":""}"#;
        assert!(validate_rule(ExtractorMode::JsonPath, json).is_err());
    }

    // ---- validate_rule: meta_attr ----

    #[test]
    fn t_rule_meta_attr_legal() {
        let json = r#"{"attr_name":"name","attr_value":"description","content_key":"content"}"#;
        assert!(validate_rule(ExtractorMode::MetaAttr, json).is_ok());
    }

    #[test]
    fn t_rule_meta_attr_illegal_empty_attr_name() {
        let json = r#"{"attr_name":"","attr_value":"description"}"#;
        assert!(validate_rule(ExtractorMode::MetaAttr, json).is_err());
    }

    // ---- validate_rule: header_field ----

    #[test]
    fn t_rule_header_field_legal() {
        let json = r#"{"header_name":"X-Total-Count"}"#;
        assert!(validate_rule(ExtractorMode::HeaderField, json).is_ok());
    }

    #[test]
    fn t_rule_header_field_illegal_empty() {
        let json = r#"{"header_name":""}"#;
        assert!(validate_rule(ExtractorMode::HeaderField, json).is_err());
    }

    // ---- deserialize_rule / serialize_rule round-trip ----

    #[test]
    fn t_round_trip_css() {
        let json = r#"{"selector":".title","attr":"text"}"#;
        let rule = deserialize_rule(ExtractorMode::Css, json).unwrap();
        assert!(matches!(rule, Rule::Css(_)));
        let (mode, _) = serialize_rule(&rule);
        assert_eq!(mode, "css");
    }

    #[test]
    fn t_round_trip_regex() {
        let json = r#"{"pattern":"v=(\\d+)","group":1,"flags":"i"}"#;
        let rule = deserialize_rule(ExtractorMode::Regex, json).unwrap();
        let (mode, _) = serialize_rule(&rule);
        assert_eq!(mode, "regex");
    }

    #[test]
    fn t_round_trip_json_path() {
        let json = r#"{"path":"$.data.title"}"#;
        let rule = deserialize_rule(ExtractorMode::JsonPath, json).unwrap();
        let (mode, _) = serialize_rule(&rule);
        assert_eq!(mode, "json_path");
    }

    // ---- Scope / FieldType / SourceLayer enum strings ----

    #[test]
    fn t_scope_round_trip() {
        assert_eq!(Scope::ListPage.as_str(), "list_page");
        assert_eq!(Scope::from_str("list_page"), Some(Scope::ListPage));
        assert_eq!(Scope::from_str("invalid"), None);
    }

    #[test]
    fn t_field_type_round_trip() {
        assert_eq!(FieldType::LinkCard.as_str(), "link_card");
        assert_eq!(FieldType::from_str("link_card"), Some(FieldType::LinkCard));
        assert_eq!(FieldType::Pagination.as_str(), "pagination");
    }

    #[test]
    fn t_source_layer_round_trip() {
        assert_eq!(SourceLayer::Script.as_str(), "script");
        assert_eq!(SourceLayer::from_str("script"), Some(SourceLayer::Script));
    }

    #[test]
    fn t_extractor_mode_all_seven() {
        for s in [
            "css",
            "regex",
            "prefix_suffix",
            "json_path",
            "meta_attr",
            "header_field",
            "follow_url",
        ] {
            assert!(ExtractorMode::from_str(s).is_some(), "missing mode {s}");
            let m = ExtractorMode::from_str(s).unwrap();
            assert_eq!(m.as_str(), s);
        }
    }

    // ---- FollowUrlRule + SubRule ----

    #[test]
    fn t_rule_follow_url_legal_minimal() {
        // transit + extract 都是 css 子规则，transit_layer/target_layer 用默认 Html
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a.dl","attr":"href"}},"extract":{"mode":"css","spec":{"selector":"a.real","attr":"href"}}}"#;
        assert!(validate_rule(ExtractorMode::FollowUrl, json).is_ok());
    }

    #[test]
    fn t_rule_follow_url_legal_with_layers() {
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a.dl","attr":"href"}},"transit_layer":"html","target_layer":"script","target_script_index":2,"extract":{"mode":"json_path","spec":{"path":"$.url"}}}"#;
        assert!(validate_rule(ExtractorMode::FollowUrl, json).is_ok());
    }

    #[test]
    fn t_rule_follow_url_illegal_transit_empty_selector() {
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"","attr":"href"}},"extract":{"mode":"css","spec":{"selector":"a","attr":"href"}}}"#;
        let r = validate_rule(ExtractorMode::FollowUrl, json);
        assert!(r.is_err());
        assert!(
            r.unwrap_err().contains("follow_url.transit"),
            "错误信息应带 follow_url.transit 路径前缀"
        );
    }

    #[test]
    fn t_rule_follow_url_illegal_extract_empty() {
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a","attr":"href"}},"extract":{"mode":"regex","spec":{"pattern":"","group":1}}}"#;
        let r = validate_rule(ExtractorMode::FollowUrl, json);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("follow_url.extract"));
    }

    #[test]
    fn t_rule_follow_url_illegal_target_script_no_index() {
        // target_layer=script 但未给 target_script_index
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a","attr":"href"}},"target_layer":"script","extract":{"mode":"css","spec":{"selector":"a","attr":"href"}}}"#;
        let r = validate_rule(ExtractorMode::FollowUrl, json);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("target_script_index"));
    }

    #[test]
    fn t_rule_follow_url_illegal_json() {
        let r = validate_rule(ExtractorMode::FollowUrl, "{not json");
        assert!(r.is_err());
    }

    #[test]
    fn t_follow_url_rule_serde_round_trip() {
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a.dl","attr":"href"}},"transit_layer":"html","transit_script_index":null,"target_layer":"html","target_script_index":null,"extract":{"mode":"regex","spec":{"pattern":"https://example\\.com/(.+)","group":1,"flags":""}}}"#;
        let rule: FollowUrlRule = serde_json::from_str(json).unwrap();
        // 默认值正确填充
        assert_eq!(rule.transit_layer, SourceLayer::Html);
        assert_eq!(rule.target_layer, SourceLayer::Html);
        assert!(rule.transit_script_index.is_none());
        // 序列化回去仍可解析
        let back = serde_json::to_string(&rule).unwrap();
        let _: FollowUrlRule = serde_json::from_str(&back).unwrap();
    }

    #[test]
    fn t_follow_url_rule_default_layer_is_html() {
        // 省略 transit_layer/target_layer 应默认 Html
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a"}},"extract":{"mode":"css","spec":{"selector":"a"}}}"#;
        let rule: FollowUrlRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.transit_layer, SourceLayer::Html);
        assert_eq!(rule.target_layer, SourceLayer::Html);
    }

    #[test]
    fn t_sub_rule_mode_str_and_extractor_mode() {
        let sub = SubRule::Css(CssRule {
            selector: "a".into(),
            attr: "href".into(),
        });
        assert_eq!(sub.mode_str(), "css");
        assert_eq!(sub.to_extractor_mode(), ExtractorMode::Css);

        let sub = SubRule::HeaderField(HeaderFieldRule {
            header_name: "X-Foo".into(),
        });
        assert_eq!(sub.mode_str(), "header_field");
        assert_eq!(sub.to_extractor_mode(), ExtractorMode::HeaderField);
    }

    #[test]
    fn t_deserialize_rule_follow_url_round_trip() {
        let json = r#"{"transit":{"mode":"css","spec":{"selector":"a.dl","attr":"href"}},"extract":{"mode":"css","spec":{"selector":"a.real","attr":"href"}}}"#;
        let rule = deserialize_rule(ExtractorMode::FollowUrl, json).unwrap();
        match rule {
            Rule::FollowUrl(fu) => {
                assert!(matches!(fu.transit, SubRule::Css(_)));
                assert!(matches!(fu.extract, SubRule::Css(_)));
            }
            other => panic!("期望 FollowUrl，实际 {other:?}"),
        }
    }

    // ---- compile_regex ----

    #[test]
    fn t_compile_regex_no_flags() {
        assert!(compile_regex(r"\d+", "").is_ok());
    }

    #[test]
    fn t_compile_regex_inline_flags() {
        assert!(compile_regex("title", "i").is_ok());
    }

    #[test]
    fn t_compile_regex_unknown_flags_filtered() {
        // 未知 flag 字符应被忽略而非报错
        assert!(compile_regex("title", "iZQ").is_ok());
    }

    // ---- [feature 046] Script variant ----

    #[test]
    fn t_extractor_mode_includes_script_variant() {
        // 序列化为 "script"
        assert_eq!(
            serde_json::to_string(&ExtractorMode::Script).unwrap(),
            "\"script\""
        );
        // 反序列化 round-trip
        let m: ExtractorMode = serde_json::from_str("\"script\"").unwrap();
        assert_eq!(m, ExtractorMode::Script);
        // from_str / as_str 对齐
        assert_eq!(
            ExtractorMode::from_str("script"),
            Some(ExtractorMode::Script)
        );
        assert_eq!(ExtractorMode::Script.as_str(), "script");
    }

    #[test]
    fn t_rule_script_variant_round_trip() {
        let json = r#"{"body":"return ctx.value.toUpperCase()","api_version":"v1"}"#;
        let r: ScriptRule = serde_json::from_str(json).unwrap();
        assert_eq!(r.api_version, "v1");
        let rule = Rule::Script(r);
        // Rule 序列化：{"mode":"script","spec":{...}}
        let s = serde_json::to_string(&rule).unwrap();
        assert!(s.contains("\"mode\":\"script\""), "actual: {s}");
        // 反序列化
        let back: Rule = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Rule::Script(_)));
        assert_eq!(rule.mode_str(), "script");
    }

    #[test]
    fn t_rule_script_default_api_version_is_v1() {
        // 省略 api_version 应默认 "v1"
        let json = r#"{"body":"return ctx.value"}"#;
        let r: ScriptRule = serde_json::from_str(json).unwrap();
        assert_eq!(r.api_version, "v1");
    }

    #[test]
    fn t_validate_script_rule_rejects_empty_body() {
        let r = ScriptRule {
            body: "   \n\t  ".into(),
            api_version: "v1".into(),
        };
        let err = validate_script_rule(&r).unwrap_err();
        assert!(err.contains("script.body"));
    }

    #[test]
    fn t_validate_script_rule_rejects_oversized_body() {
        let r = ScriptRule {
            body: "x".repeat(65_537),
            api_version: "v1".into(),
        };
        let err = validate_script_rule(&r).unwrap_err();
        assert!(err.contains("65537"));
        assert!(err.contains("65536"));
    }

    #[test]
    fn t_validate_script_rule_rejects_unknown_api_version() {
        let r = ScriptRule {
            body: "return ctx.value".into(),
            api_version: "v2".into(),
        };
        let err = validate_script_rule(&r).unwrap_err();
        assert!(err.contains("v1"));
        assert!(err.contains("v2"));
    }

    #[test]
    fn t_validate_script_rule_accepts_legal_body() {
        let r = ScriptRule {
            body: "return ctx.value.toUpperCase()".into(),
            api_version: "v1".into(),
        };
        assert!(validate_script_rule(&r).is_ok());
    }

    #[test]
    fn t_validate_rule_script_path() {
        // 走 validate_rule 公共入口
        let json = r#"{"body":"return ctx.value","api_version":"v1"}"#;
        assert!(validate_rule(ExtractorMode::Script, json).is_ok());
        let bad = r#"{"body":"   ","api_version":"v1"}"#;
        assert!(validate_rule(ExtractorMode::Script, bad).is_err());
    }

    #[test]
    fn t_validate_field_node_rejects_list_scope_with_script() {
        let node = FieldNodeSpec {
            id: None,
            task_id: None,
            parent_id: None,
            scope: Scope::ListPage,
            name: "title".into(),
            display_name: "标题".into(),
            field_type: FieldType::String,
            source_layer: SourceLayer::Html,
            extractor_mode: ExtractorMode::Script,
            rule: Rule::Script(ScriptRule {
                body: "return ctx.value".into(),
                api_version: "v1".into(),
            }),
            post_processors: vec![],
            script_index: None,
            sort_order: 0,
            is_active: true,
            refresh_on_read: false,
        };
        let err = validate_field_node_spec(&node).unwrap_err();
        assert!(err.contains("detail_page"));
        assert!(err.contains("list_page"));
    }

    #[test]
    fn t_validate_field_node_rejects_refresh_on_read_with_non_script() {
        let node = FieldNodeSpec {
            id: None,
            task_id: None,
            parent_id: None,
            scope: Scope::DetailPage,
            name: "title".into(),
            display_name: "标题".into(),
            field_type: FieldType::String,
            source_layer: SourceLayer::Html,
            extractor_mode: ExtractorMode::Css,
            rule: Rule::Css(CssRule {
                selector: ".t".into(),
                attr: "text".into(),
            }),
            post_processors: vec![],
            script_index: None,
            sort_order: 0,
            is_active: true,
            refresh_on_read: true, // ← 仅 script 允许
        };
        let err = validate_field_node_spec(&node).unwrap_err();
        assert!(err.contains("refresh_on_read"));
        assert!(err.contains("css"));
    }

    #[test]
    fn t_validate_field_node_accepts_detail_script_with_refresh() {
        let node = FieldNodeSpec {
            id: None,
            task_id: None,
            parent_id: None,
            scope: Scope::DetailPage,
            name: "title".into(),
            display_name: "标题".into(),
            field_type: FieldType::String,
            source_layer: SourceLayer::Html,
            extractor_mode: ExtractorMode::Script,
            rule: Rule::Script(ScriptRule {
                body: "return ctx.value".into(),
                api_version: "v1".into(),
            }),
            post_processors: vec![],
            script_index: None,
            sort_order: 0,
            is_active: true,
            refresh_on_read: true,
        };
        assert!(validate_field_node_spec(&node).is_ok());
    }

    #[test]
    fn t_serialize_deserialize_rule_script_round_trip() {
        let rule = Rule::Script(ScriptRule {
            body: "return ctx.value + '!'".into(),
            api_version: "v1".into(),
        });
        let (mode, json) = serialize_rule(&rule);
        assert_eq!(mode, "script");
        // 反序列化回来
        let back = deserialize_rule(ExtractorMode::Script, &json).unwrap();
        assert!(matches!(back, Rule::Script(_)));
    }
}
