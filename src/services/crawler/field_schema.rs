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

/// 匹配模式：6 种
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractorMode {
    Css,
    Regex,
    PrefixSuffix,
    JsonPath,
    MetaAttr,
    HeaderField,
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

/// 6 模式 Rule 联合（discriminated by `mode`，但实际持久化按 mode_json 分别解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", content = "spec", rename_all = "snake_case")]
pub enum Rule {
    Css(CssRule),
    Regex(RegexRule),
    PrefixSuffix(PrefixSuffixRule),
    JsonPath(JsonPathRule),
    MetaAttr(MetaAttrRule),
    HeaderField(HeaderFieldRule),
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
        }
    }
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
static NAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z][a-z0-9_]{0,31}$").expect("invalid name regex")
});

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
///   MetaAttr attr_name+attr_value / HeaderField header_name）
pub fn validate_rule(mode: ExtractorMode, rule_json: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(rule_json)
        .map_err(|e| format!("rule_json 不是合法 JSON: {e}"))?;
    match mode {
        ExtractorMode::Css => {
            let r: CssRule = serde_json::from_value(value)
                .map_err(|e| format!("css 规则反序列化失败: {e}"))?;
            if r.selector.trim().is_empty() {
                return Err("css.selector 不能为空".into());
            }
            Ok(())
        }
        ExtractorMode::Regex => {
            let r: RegexRule = serde_json::from_value(value)
                .map_err(|e| format!("regex 规则反序列化失败: {e}"))?;
            if r.pattern.is_empty() {
                return Err("regex.pattern 不能为空".into());
            }
            // 试编译以提前发现语法错误
            compile_regex(&r.pattern, &r.flags)
                .map_err(|e| format!("regex.pattern 编译失败: {e}"))?;
            Ok(())
        }
        ExtractorMode::PrefixSuffix => {
            let r: PrefixSuffixRule = serde_json::from_value(value)
                .map_err(|e| format!("prefix_suffix 规则反序列化失败: {e}"))?;
            if r.prefix.is_empty() {
                return Err("prefix_suffix.prefix 不能为空".into());
            }
            if r.suffix.is_empty() {
                return Err("prefix_suffix.suffix 不能为空".into());
            }
            Ok(())
        }
        ExtractorMode::JsonPath => {
            let r: JsonPathRule = serde_json::from_value(value)
                .map_err(|e| format!("json_path 规则反序列化失败: {e}"))?;
            if r.path.trim().is_empty() {
                return Err("json_path.path 不能为空".into());
            }
            if !r.path.starts_with('$') {
                return Err("json_path.path 须以 $ 开头（RFC 9535）".into());
            }
            Ok(())
        }
        ExtractorMode::MetaAttr => {
            let r: MetaAttrRule = serde_json::from_value(value)
                .map_err(|e| format!("meta_attr 规则反序列化失败: {e}"))?;
            if r.attr_name.trim().is_empty() {
                return Err("meta_attr.attr_name 不能为空".into());
            }
            if r.attr_value.trim().is_empty() {
                return Err("meta_attr.attr_value 不能为空".into());
            }
            Ok(())
        }
        ExtractorMode::HeaderField => {
            let r: HeaderFieldRule = serde_json::from_value(value)
                .map_err(|e| format!("header_field 规则反序列化失败: {e}"))?;
            if r.header_name.trim().is_empty() {
                return Err("header_field.header_name 不能为空".into());
            }
            Ok(())
        }
    }
}

/// 把 DB 中的 (mode, rule_json) 反序列化为 [`Rule::Css`] 等（用于应用层 dispatch）
pub fn deserialize_rule(mode: ExtractorMode, rule_json: &str) -> Result<Rule, String> {
    let value: Value = serde_json::from_str(rule_json)
        .map_err(|e| format!("rule_json 不是合法 JSON: {e}"))?;
    Ok(match mode {
        ExtractorMode::Css => Rule::Css(
            serde_json::from_value(value)
                .map_err(|e| format!("css 反序列化失败: {e}"))?,
        ),
        ExtractorMode::Regex => Rule::Regex(
            serde_json::from_value(value)
                .map_err(|e| format!("regex 反序列化失败: {e}"))?,
        ),
        ExtractorMode::PrefixSuffix => Rule::PrefixSuffix(
            serde_json::from_value(value)
                .map_err(|e| format!("prefix_suffix 反序列化失败: {e}"))?,
        ),
        ExtractorMode::JsonPath => Rule::JsonPath(
            serde_json::from_value(value)
                .map_err(|e| format!("json_path 反序列化失败: {e}"))?,
        ),
        ExtractorMode::MetaAttr => Rule::MetaAttr(
            serde_json::from_value(value)
                .map_err(|e| format!("meta_attr 反序列化失败: {e}"))?,
        ),
        ExtractorMode::HeaderField => Rule::HeaderField(
            serde_json::from_value(value)
                .map_err(|e| format!("header_field 反序列化失败: {e}"))?,
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
    (mode, serde_json::to_string(&inner).unwrap_or_else(|_| "{}".to_string()))
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
    fn t_extractor_mode_all_six() {
        for s in [
            "css",
            "regex",
            "prefix_suffix",
            "json_path",
            "meta_attr",
            "header_field",
        ] {
            assert!(ExtractorMode::from_str(s).is_some(), "missing mode {s}");
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
}
