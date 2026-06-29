//! 源码素材抓取层（feature 043-crawler-configurator，US1 T019）
//!
//! 设计目的：把一个 URL 的源码切成 4 个可消费的 tab 视图（header/html/script/meta），
//! 供前端 SourceViewer 渲染 + Probe 验证字段规则。
//!
//! 与 042 `engine.rs::fetch_url` 的关系：
//! - 复用 `build_reqwest_client`（不依赖 AppState，签名轻量）
//! - 不走 `fetch_url(state)` 因为 Probe 场景下 proxy 由调用方显式传入（无系统兜底）
//!
//! 反爬/超链接：相对→绝对 URL 解析复用 `engine::resolve_url`；HTTP 重定向由 reqwest 默认策略自动跟随。

use std::collections::HashMap;
use std::time::Instant;

use chrono::NaiveDateTime;

use crate::services::crawler::engine::{build_reqwest_client, resolve_url};

// ============================================================================
// 数据结构
// ============================================================================

/// 抓取的素材：单 URL 的 4 tab 视图聚合
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceMaterial {
    /// 最终落地 URL（跟随重定向后的最终地址，作为后续相对→绝对 URL 解析的 base）
    pub final_url: String,
    /// HTTP 状态码（最终响应）
    pub status: u16,
    /// 响应头（key 不区分大小写 → 全部小写存储，便于查询）
    pub headers: HashMap<String, String>,
    /// HTML 原文（不解码、不清洗，原样给前端显示与 Probe CSS 命中）
    pub html: String,
    /// 解析出的 `<script>` 块
    pub scripts: Vec<ScriptBlock>,
    /// 解析出的 `<meta>` 标签
    pub metas: Vec<MetaTag>,
    /// 抓取完成时间（UTC）
    pub fetched_at: NaiveDateTime,
    /// 抓取耗时（毫秒）
    pub duration_ms: u64,
}

/// `<script>` 块视图
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptBlock {
    /// 第几个 `<script>`（0-based，前端用 script_index 引用）
    pub index: usize,
    /// 外链 src（若有），绝对 URL
    pub src: Option<String>,
    /// 内联脚本内容（若是外链则为 None）
    pub content: Option<String>,
    /// 内联脚本解析出的 JSON 值（US4 T052）：尝试把 `content` 当作纯 JSON 解析；
    /// 解析失败（如 `window.__DATA__={...}` 不是合法 JSON）则为 None，
    /// 由 `extractor::extract_by_json_path` 进一步用启发式从 content 中提取 JSON 段
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_value: Option<serde_json::Value>,
}

impl PartialEq for ScriptBlock {
    fn eq(&self, other: &Self) -> bool {
        // json_value 不参与相等性判定（Value 比较开销小但语义上以脚本文本为准）
        self.index == other.index
            && self.src == other.src
            && self.content == other.content
    }
}

impl Eq for ScriptBlock {}

/// `<meta>` 标签视图
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MetaTag {
    /// 用 name 还是 property 属性标识（HTML5 规范两者皆可）
    pub key_kind: MetaKeyKind,
    /// name/property/http-equiv 的值（如 "description" / "og:title"）
    pub key: String,
    /// content 属性值
    pub content: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetaKeyKind {
    Name,
    Property,
    HttpEquiv,
    Other,
}

impl MetaKeyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaKeyKind::Name => "name",
            MetaKeyKind::Property => "property",
            MetaKeyKind::HttpEquiv => "http-equiv",
            MetaKeyKind::Other => "other",
        }
    }
}

// ============================================================================
// 结构化错误（与 probe.rs 共享）
// ============================================================================

/// 抓取/Probe 链路的结构化错误（US1 T019/T021 共用）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeError {
    /// 出错的阶段
    pub stage: ProbeStage,
    /// 错误分类（前端按此选择图标/文案）
    pub category: ProbeCategory,
    /// 面向用户的简明错误信息
    pub message: String,
    /// 修复建议（可选）
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStage {
    /// 抓 URL 阶段失败
    Fetch,
    /// 解析 HTML / JSON 阶段失败
    Parse,
    /// 应用字段规则失败
    Match,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCategory {
    /// URL 不可达（DNS/连接/超时/TLS）
    UrlUnreachable,
    /// HTTP 4xx/5xx
    #[serde(rename = "http_4xx_5xx")]
    Http4xx5xx,
    /// 命中反爬拦截（登录墙/验证码/403/429/503）
    Blocked,
    /// 字段规则非法
    InvalidRule,
    /// 字段未命中（0 条）
    ZeroHits,
    /// 父字段未命中导致子字段无法运行
    ParentEmpty,
}

impl ProbeError {
    pub fn new(stage: ProbeStage, category: ProbeCategory, message: impl Into<String>) -> Self {
        Self {
            stage,
            category,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}/{:?}] {}", self.stage, self.category, self.message)
    }
}

impl std::error::Error for ProbeError {}

// ============================================================================
// 主入口
// ============================================================================

/// 抓取单个 URL 的素材
///
/// - `url`：目标 URL（应含 scheme；不带 scheme 时返回 `UrlUnreachable`）
/// - `user_agent`：自定义 UA；None 用 `DEFAULT_USER_AGENT`
/// - `proxy`：自定义代理；None 不走代理
///
/// 失败场景（映射到 `ProbeError`）：
/// - URL 解析失败 → stage=Fetch / UrlUnreachable
/// - 网络层失败（DNS、连接拒绝、超时、TLS） → stage=Fetch / UrlUnreachable
/// - HTTP 4xx/5xx → stage=Fetch / Http4xx5xx
/// - 反爬拦截特征（Cloudflare challenge / 登录墙关键词） → stage=Fetch / Blocked
pub async fn fetch_source_material(
    url: &str,
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<SourceMaterial, ProbeError> {
    let started = Instant::now();
    let fetched_at = chrono::Utc::now().naive_utc();

    if !looks_like_url(url) {
        return Err(ProbeError::new(
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            format!("URL 不合法：{url}"),
        )
        .with_hint("URL 应包含 scheme（如 https://example.com/list）"));
    }

    let client = build_reqwest_client(user_agent, proxy).map_err(|e| {
        ProbeError::new(
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            format!("构建 HTTP 客户端失败：{e}"),
        )
    })?;

    let resp = client.get(url).send().await.map_err(|e| {
        ProbeError::new(
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            format!("GET {url} 失败：{e}"),
        )
    })?;

    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();

    // headers：lowercase key 便于查询
    let mut headers: HashMap<String, String> = HashMap::new();
    for (k, v) in resp.headers().iter() {
        let key = k.as_str().to_lowercase();
        let val = v.to_str().unwrap_or("").to_string();
        // 多值合并（逗号分隔）
        headers
            .entry(key)
            .and_modify(|existing: &mut String| {
                existing.push_str(", ");
                existing.push_str(&val);
            })
            .or_insert(val);
    }

    let body = resp.text().await.map_err(|e| {
        ProbeError::new(
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            format!("读取响应体失败：{e}"),
        )
    })?;

    // 4xx/5xx：构造错误返回，但仍把素材信息塞进 hint 供调试（headers 已收集）
    if (400..500).contains(&status) || (500..600).contains(&status) {
        let blocked = is_blocked_status(status, &body);
        let category = if blocked { ProbeCategory::Blocked } else { ProbeCategory::Http4xx5xx };
        return Err(ProbeError::new(
            ProbeStage::Fetch,
            category,
            format!("HTTP {status}（{url}）"),
        ));
    }

    let scripts = parse_scripts(&body, &final_url);
    let metas = parse_metas(&body);

    Ok(SourceMaterial {
        final_url,
        status,
        headers,
        html: body,
        scripts,
        metas,
        fetched_at,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// US3 T046：取详情页样本素材
///
/// 流程：
/// 1. 抓取列表 URL 一次
/// 2. 在 list_page 字段树中找到首个能产出 URL 的字段（field_type='url' 或
///    'link_card' 含 url 子字段，或 'pagination'）
/// 3. 用该字段规则在列表素材上求命中 → 取首条 URL（自动绝对化）
/// 4. 抓取该详情 URL，返回 `(detail_url, detail_material)`
///
/// 错误：
/// - 列表抓取失败 → 透传 fetch 阶段错误
/// - 找不到能产出 URL 的字段 → `Match / ParentEmpty`（FR-014）
/// - 字段命中但 URL 为空 → `Match / ZeroHits`
/// - 详情抓取失败 → 透传 fetch 阶段错误
pub async fn fetch_detail_sample(
    list_nodes: &[crate::models::crawler_field_node::FieldTreeNode],
    list_url: &str,
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<(String, SourceMaterial), ProbeError> {
    // 1. 抓列表 URL
    let list_material = fetch_source_material(list_url, user_agent, proxy).await?;

    // 2. 找首个 URL 类字段并求命中
    let detail_url = find_first_detail_url(list_nodes, &list_material).ok_or_else(|| {
        ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ParentEmpty,
            "未找到能产出 URL 的 list_page 字段（需要 field_type=url/link_card/pagination）",
        )
        .with_hint("在字段树中添加一个 url 类字段（如 link_card 下的 url 子字段）以让详情页 tab 取到样本")
    })?;

    if detail_url.trim().is_empty() {
        return Err(ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ZeroHits,
            "URL 字段命中但值为空",
        ));
    }

    // 3. 防 self-loop / 列表分页 loop：详情 URL 等于列表 URL，或明显是列表的分页
    //    变体（/page/N、?page=N、list_path + "/N"），说明 url 子字段选错了元素——
    //    选到了导航/分页/分类链接（如"下一页"），而不是文章详情链接。放行会抓回
    //    列表页（或下一页列表），前端"详情页素材"tab 看到的还是列表源码。
    let list_final = &list_material.final_url;
    if is_list_self_or_pagination_loop(&detail_url, list_final) {
        return Err(ProbeError::new(
            ProbeStage::Match,
            ProbeCategory::ZeroHits,
            format!(
                "提取出的详情 URL 看起来还是列表页：{}（与列表 URL 相同或是其分页变体）",
                detail_url
            ),
        )
        .with_hint(
            "url 子字段的 selector 选错了元素（选到了导航/分页/分类链接）：\n  \
             1. 确认 attr = href（取 <a> 的链接）\n  \
             2. 在左侧 HTML tab 找到真实文章详情的 <a> 标签，复制其 class 作为 selector\n  \
             3. 避免选到分页栏（含下一页/页码的容器）和导航栏的链接\n  \
             4. 若用 link_card：父规则应只匹配文章卡片容器，子 url 规则在容器内取 href",
        ));
    }

    // 4. 抓详情 URL
    let detail_material = fetch_source_material(&detail_url, user_agent, proxy).await?;
    Ok((detail_url, detail_material))
}

/// 去掉 URL 的 #fragment 部分（用于 self-loop 比对）
fn strip_fragment(url: &str) -> &str {
    url.split_once('#').map(|(before, _)| before).unwrap_or(url)
}

/// 判断 detail_url 是否是 list_url 的 self-loop 或列表分页变体
///
/// 命中条件（任一）：
/// 1. 两者（去 fragment 后）完全相同 → self-loop（href="#" / href=list_url 自身）
/// 2. detail_path = list_path + "/N"（末段纯数字，如 list=/cat/x/ detail=/cat/x/2）
/// 3. detail_path = list_path + "/page/N"（WordPress 风格分页）
/// 4. path 相同但 detail query 含 page/p/paged/pg 参数（?page=2 / ?p=2）
///
/// 覆盖 Discuz / WordPress / 通用 CMS 的列表分页 URL 模式。详情页 URL 通常
/// 与列表 URL 路径不共享长前缀（如 /post/123、/article/xxx），不会被误判。
fn is_list_self_or_pagination_loop(detail: &str, base: &str) -> bool {
    let detail = strip_fragment(detail);
    let base = strip_fragment(base);
    if detail == base {
        return true;
    }
    let (d_path, d_q) = detail.split_once('?').unwrap_or((detail, ""));
    let (b_path, _b_q) = base.split_once('?').unwrap_or((base, ""));
    let b_trim = b_path.trim_end_matches('/');
    // 情况 1 & 2：detail_path = base_path_trim + "/N" 或 + "/page/N"
    if d_path.len() > b_trim.len() && d_path.starts_with(b_trim) {
        let rest = d_path[b_trim.len()..].trim_end_matches('/');
        if let Some(seg) = rest.strip_prefix('/') {
            // 末段纯数字（/2 → Discuz 风格 /category/x/N）
            if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
            // WordPress 风格 /page/N
            if let Some(n) = seg.strip_prefix("page/")
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
            {
                return true;
            }
        }
    }
    // 情况 3：path 相同，detail query 含分页参数
    if d_path == b_path && !d_q.is_empty() {
        let has_page = d_q.split('&').any(|kv| {
            let k = kv.split('=').next().unwrap_or("");
            matches!(k, "page" | "p" | "paged" | "pg")
        });
        if has_page {
            return true;
        }
    }
    false
}

/// 在 list_page 字段树中找到首条详情 URL（递归扫描）
///
/// 优先级：
/// 1. field_type=url 的根字段：直接提取（pagination 字段语义为"列表下一页"，
///    不是详情入口，**不**在此处理，否则会把分页链接当详情 URL）
/// 2. field_type=link_card：在其子节点中找 name=url/link 或 field_type=url 的子字段
/// 3. 任意根字段（排除 pagination/link_card）提取出的值看起来像 URL（启发式）
fn find_first_detail_url(
    nodes: &[crate::models::crawler_field_node::FieldTreeNode],
    material: &SourceMaterial,
) -> Option<String> {
    use crate::services::crawler::field_schema::FieldType;

    // Pass 1: 直接 URL 类字段（注意：不含 Pagination —— pagination 是"列表下一页"，
    // 不是详情入口；若纳入会把分页链接（如 /category/window/2）当详情 URL 抓回列表页）
    for node in nodes {
        let spec = match node.row.to_spec() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if spec.field_type != FieldType::Url {
            continue;
        }
        if let Some(url) = extract_first_url_from_node(&spec.rule, spec.source_layer, spec.script_index, &spec.post_processors, material) {
            return Some(url);
        }
    }

    // Pass 2: link_card 下找 url 子字段
    for node in nodes {
        let spec = match node.row.to_spec() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !matches!(spec.field_type, FieldType::LinkCard) {
            continue;
        }
        // 先抓父命中得到 context_html（应用父字段 post_processors，与 probe run_nested_probe 保持一致）
        let p_raw = extract_to_hits(&spec.rule, spec.source_layer, spec.script_index, material);
        let parent_hits = crate::services::crawler::extractor::apply_post_processors(
            p_raw,
            &spec.post_processors,
            &material.final_url,
        );
        if parent_hits.is_empty() {
            continue;
        }
        // 在父命中下找 url 子字段
        for child in &node.children {
            let cs = match child.row.to_spec() {
                Ok(s) => s,
                Err(_) => continue,
            };
            let wants = matches!(cs.field_type, FieldType::Url)
                || child.row.name == "url"
                || child.row.name == "link";
            if !wants {
                continue;
            }
            // 在每条父命中下尝试子规则
            for ph in &parent_hits {
                let sub = make_sub_from_hit(ph, material);
                if let Some(url) = extract_first_url_from_node(
                    &cs.rule,
                    cs.source_layer,
                    cs.script_index,
                    &cs.post_processors,
                    &sub,
                ) {
                    return Some(url);
                }
            }
        }
    }

    // Pass 3: 任意字段值看起来像 URL（启发式兜底）
    // 排除 Pagination（会把"列表下一页"当成详情入口）和 LinkCard（根规则产出容器 HTML，
    // 不是 URL 本身；其 url 子字段已在 Pass 2 处理）
    for node in nodes {
        let spec = match node.row.to_spec() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if matches!(spec.field_type, FieldType::Pagination | FieldType::LinkCard) {
            continue;
        }
        // 用原始命中值判断（不绝对化后再判），避免误把文本当作 URL
        let hits = extract_to_hits(&spec.rule, spec.source_layer, spec.script_index, material);
        let processed = crate::services::crawler::extractor::apply_post_processors(hits, &spec.post_processors, &material.final_url);
        if let Some(first) = processed.into_iter().next() {
            let v = first.value.trim();
            if v.starts_with("http://") || v.starts_with("https://") {
                return Some(crate::services::crawler::engine::resolve_url(v, &material.final_url));
            }
        }
    }

    None
}

/// 用 rule+source_layer+script_index 在 material 上提取首条命中并绝对化为 URL
fn extract_first_url_from_node(
    rule: &crate::services::crawler::field_schema::Rule,
    source_layer: crate::services::crawler::field_schema::SourceLayer,
    script_index: Option<i32>,
    post_processors: &[crate::services::crawler::field_schema::PostProcessor],
    material: &SourceMaterial,
) -> Option<String> {
    let hits = extract_to_hits(rule, source_layer, script_index, material);
    let processed = crate::services::crawler::extractor::apply_post_processors(hits, post_processors, &material.final_url);
    processed
        .into_iter()
        .next()
        .map(|h| {
            let v = h.value.trim();
            if v.is_empty() {
                String::new()
            } else {
                crate::services::crawler::engine::resolve_url(v, &material.final_url)
            }
        })
        .filter(|s| !s.is_empty())
}

/// 提取为 Hit 列表（不应用 post_processors）
fn extract_to_hits(
    rule: &crate::services::crawler::field_schema::Rule,
    source_layer: crate::services::crawler::field_schema::SourceLayer,
    script_index: Option<i32>,
    material: &SourceMaterial,
) -> Vec<crate::services::crawler::extractor::Hit> {
    use crate::services::crawler::extractor::{ExtractInput, extract};
    let input = ExtractInput::from_material(material, script_index).with_layer(source_layer);
    extract(rule, &input).unwrap_or_default()
}

/// 用父命中的 context_html 构造子作用域素材
fn make_sub_from_hit(
    ph: &crate::services::crawler::extractor::Hit,
    parent: &SourceMaterial,
) -> SourceMaterial {
    let html = ph
        .context_html
        .clone()
        .unwrap_or_else(|| ph.value.clone());
    SourceMaterial {
        final_url: parent.final_url.clone(),
        status: parent.status,
        headers: parent.headers.clone(),
        html,
        scripts: Vec::<crate::services::crawler::source_layer::ScriptBlock>::new(),
        metas: Vec::<crate::services::crawler::source_layer::MetaTag>::new(),
        fetched_at: parent.fetched_at,
        duration_ms: 0,
    }
}

// ============================================================================
// 内部工具
// ============================================================================

fn looks_like_url(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

/// 4xx/5xx 是否属于反爬拦截（与 block_detector 对齐：403/429/503 或登录墙关键词）
fn is_blocked_status(status: u16, body: &str) -> bool {
    matches!(status, 403 | 429 | 503) || body_has_block_keywords(body)
}

fn body_has_block_keywords(body: &str) -> bool {
    // 取前 8KB 足够（拦截页通常在头部）
    let head: &str = if body.len() > 8192 { &body[..8192] } else { body };
    const KEYWORDS: &[&str] = &[
        "Just a moment",            // Cloudflare challenge
        "Checking your browser",    // Cloudflare
        "cf-browser-verification",
        "Attention Required",       // Cloudflare 403
        "请输入验证码",
        "请登录后",
        "请先登录",
        "系统检测到您的访问行为",
    ];
    let lower = head.to_lowercase();
    KEYWORDS
        .iter()
        .any(|kw| head.contains(kw) || lower.contains(&kw.to_lowercase()))
}

/// 从 HTML 解析所有 `<script>` 块
///
/// 规则：
/// - 内联脚本：取 text 内容；忽略 type 非法的（保留 type=text/javascript / 空 / 缺省）
/// - 外链脚本：取 src 属性，相对→绝对
/// - 不递归 `<noscript>` 内的脚本
fn parse_scripts(html: &str, base_url: &str) -> Vec<ScriptBlock> {
    let document = scraper::Html::parse_document(html);
    let Ok(script_sel) = scraper::Selector::parse("script") else {
        return Vec::new();
    };
    document
        .select(&script_sel)
        .enumerate()
        .map(|(i, el)| {
            let src = el.value().attr("src").map(|s| resolve_url(s, base_url));
            // text() 返回 RefCell<String>，借用拷贝出来
            let raw_text = el.text().collect::<String>();
            let content = if raw_text.trim().is_empty() {
                None
            } else {
                Some(raw_text)
            };
            ScriptBlock {
                index: i,
                src,
                json_value: content
                    .as_deref()
                    .and_then(|c| serde_json::from_str::<serde_json::Value>(c.trim()).ok()),
                content,
            }
        })
        .collect()
}

/// 从 HTML 解析所有 `<meta>` 标签
///
/// 规则：保留 name/property/http-equiv 三种标识 + content；charset 单独处理
fn parse_metas(html: &str) -> Vec<MetaTag> {
    let document = scraper::Html::parse_document(html);
    let Ok(meta_sel) = scraper::Selector::parse("meta") else {
        return Vec::new();
    };
    document
        .select(&meta_sel)
        .filter_map(|el| {
            let v = el.value();
            // charset 特例：<meta charset="utf-8"> → key_kind=Other, key=charset
            if let Some(charset) = v.attr("charset") {
                return Some(MetaTag {
                    key_kind: MetaKeyKind::Other,
                    key: "charset".to_string(),
                    content: charset.to_string(),
                });
            }
            // 优先 name → property → http-equiv
            for (kind, attr_name) in [
                (MetaKeyKind::Name, "name"),
                (MetaKeyKind::Property, "property"),
                (MetaKeyKind::HttpEquiv, "http-equiv"),
            ] {
                if let Some(k) = v.attr(attr_name)
                    && let Some(c) = v.attr("content") {
                        return Some(MetaTag {
                            key_kind: kind,
                            key: k.to_string(),
                            content: c.to_string(),
                        });
                    }
            }
            None
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metas_basic_description() {
        let html = r#"<html><head>
            <meta name="description" content="测试站点描述">
            <meta charset="utf-8">
        </head><body></body></html>"#;
        let metas = parse_metas(html);
        // 注意：scraper 会自动补全 <meta charset>，所以可能多出一条
        let desc = metas.iter().find(|m| m.key == "description").expect("找到 description");
        assert_eq!(desc.content, "测试站点描述");
        assert_eq!(desc.key_kind, MetaKeyKind::Name);
    }

    #[test]
    fn parse_metas_og_property() {
        let html = r#"<meta property="og:title" content="分享标题">"#;
        let metas = parse_metas(html);
        let og = metas.iter().find(|m| m.key == "og:title").expect("og:title");
        assert_eq!(og.content, "分享标题");
        assert_eq!(og.key_kind, MetaKeyKind::Property);
    }

    #[test]
    fn parse_metas_http_equiv() {
        let html = r#"<meta http-equiv="refresh" content="3;url=/next">"#;
        let metas = parse_metas(html);
        let r = metas.iter().find(|m| m.key == "refresh").expect("refresh");
        assert_eq!(r.content, "3;url=/next");
        assert_eq!(r.key_kind, MetaKeyKind::HttpEquiv);
    }

    #[test]
    fn parse_metas_charset_special_case() {
        let html = r#"<meta charset="utf-8">"#;
        let metas = parse_metas(html);
        let cs = metas.iter().find(|m| m.key == "charset").expect("charset");
        assert_eq!(cs.content, "utf-8");
        assert_eq!(cs.key_kind, MetaKeyKind::Other);
    }

    #[test]
    fn parse_metas_skips_empty_content() {
        let html = r#"<meta name="empty" content="">"#;
        let metas = parse_metas(html);
        // content="" 也会被保留（不丢弃，便于显示"该字段存在但值为空"）
        assert!(metas.iter().any(|m| m.key == "empty"));
    }

    #[test]
    fn parse_metas_empty_html_returns_empty() {
        let metas = parse_metas("");
        assert!(metas.is_empty());
    }

    #[test]
    fn parse_scripts_inline_only() {
        let html = r#"<html><body>
            <script>console.log("a")</script>
            <script>var x = 1;</script>
        </body></html>"#;
        let scripts = parse_scripts(html, "https://example.com/");
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].index, 0);
        assert!(scripts[0].src.is_none());
        assert!(scripts[0].content.as_deref().unwrap_or("").contains("console.log"));
        assert!(scripts[1].content.as_deref().unwrap_or("").contains("var x"));
    }

    #[test]
    fn parse_scripts_external_only() {
        let html = r#"<script src="/static/a.js"></script>
                      <script src="https://cdn.com/b.js"></script>"#;
        let scripts = parse_scripts(html, "https://example.com/page");
        assert_eq!(scripts.len(), 2);
        assert_eq!(scripts[0].src.as_deref(), Some("https://example.com/static/a.js"));
        assert_eq!(scripts[1].src.as_deref(), Some("https://cdn.com/b.js"));
        assert!(scripts[0].content.is_none());
    }

    #[test]
    fn parse_scripts_mixed() {
        let html = r#"<script src="a.js"></script><script>inline();</script>"#;
        let scripts = parse_scripts(html, "https://example.com/");
        assert_eq!(scripts.len(), 2);
        assert!(scripts[0].src.is_some());
        assert!(scripts[0].content.is_none());
        assert!(scripts[1].src.is_none());
        assert!(scripts[1].content.is_some());
    }

    #[test]
    fn parse_scripts_empty_html_returns_empty() {
        assert!(parse_scripts("", "https://example.com/").is_empty());
    }

    #[test]
    fn parse_scripts_relative_to_absolute() {
        let html = r#"<script src="../lib/c.js"></script>"#;
        let scripts = parse_scripts(html, "https://example.com/a/b/page.html");
        // resolve_url 对 ../ 的处理：scraper 解析后 src="../lib/c.js"，base 含 /a/b/page.html
        // resolve_url 会把 ../lib/c.js 拼到 /a/b/ → /a/lib/c.js（resolve_url 实现简化，可能不处理 ..）
        // 这里只验证返回绝对 URL（scheme://...）形式
        let src = scripts[0].src.as_deref().unwrap_or("");
        assert!(src.starts_with("https://example.com/"));
    }

    #[test]
    fn looks_like_url_valid() {
        assert!(looks_like_url("http://example.com"));
        assert!(looks_like_url("https://example.com/list?page=1"));
    }

    #[test]
    fn looks_like_url_invalid() {
        assert!(!looks_like_url(""));
        assert!(!looks_like_url("   "));
        assert!(!looks_like_url("example.com")); // 缺 scheme
        assert!(!looks_like_url("ftp://example.com")); // 非 http/https
    }

    #[test]
    fn is_blocked_status_403_429_503() {
        assert!(is_blocked_status(403, ""));
        assert!(is_blocked_status(429, ""));
        assert!(is_blocked_status(503, ""));
        assert!(!is_blocked_status(404, ""));
        assert!(!is_blocked_status(500, ""));
    }

    #[test]
    fn is_blocked_status_login_wall_keywords() {
        assert!(is_blocked_status(200, "<html><body>请先登录后查看内容</body></html>"));
        assert!(is_blocked_status(200, "Just a moment... Cloudflare"));
        assert!(!is_blocked_status(200, "<html>正常文章内容</html>"));
    }

    #[test]
    fn probe_error_with_hint_chain() {
        let e = ProbeError::new(
            ProbeStage::Fetch,
            ProbeCategory::UrlUnreachable,
            "GET 失败",
        )
        .with_hint("检查 URL 是否带 scheme");
        assert_eq!(e.hint.as_deref(), Some("检查 URL 是否带 scheme"));
        assert_eq!(e.stage, ProbeStage::Fetch);
        assert_eq!(e.category, ProbeCategory::UrlUnreachable);
    }

    #[test]
    fn meta_key_kind_serde_snake_case() {
        let s = serde_json::to_string(&MetaKeyKind::HttpEquiv).unwrap();
        assert_eq!(s, "\"http_equiv\"");
    }

    #[test]
    fn probe_stage_serde_snake_case() {
        let s = serde_json::to_string(&ProbeStage::Parse).unwrap();
        assert_eq!(s, "\"parse\"");
    }

    #[test]
    fn probe_category_serde_snake_case() {
        let s = serde_json::to_string(&ProbeCategory::Http4xx5xx).unwrap();
        assert_eq!(s, "\"http_4xx_5xx\"");
    }

    // ===== US3 T046：find_first_detail_url 单元测试 =====

    /// 构造 FieldTreeNode 用于测试（不依赖 DB）
    fn make_url_node(name: &str, selector: &str) -> crate::models::crawler_field_node::FieldTreeNode {
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        FieldTreeNode {
            row: FieldNodeRow {
                id: 1,
                task_id: 1,
                parent_id: None,
                scope: "list_page".into(),
                name: name.into(),
                display_name: name.into(),
                field_type: "url".into(),
                source_layer: "html".into(),
                extractor_mode: "css".into(),
                rule_json: format!(r#"{{"selector":"{selector}","attr":"href"}}"#),
                post_processors_json: Some(r#"[{"op":"absolutize_url"}]"#.into()),
                script_index: None,
                sort_order: 0,
                is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: Vec::new(),
        }
    }

    #[test]
    fn find_first_detail_url_picks_url_field() {
        // 构造素材：一个 <a class="link" href>
        let material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: r#"<a class="link" href="/post/1">p1</a>"#.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        let nodes = vec![make_url_node("post_url", "a.link")];
        let url = find_first_detail_url(&nodes, &material).expect("应找到 URL");
        assert_eq!(url, "https://example.com/post/1");
    }

    #[test]
    fn find_first_detail_url_returns_none_when_no_url_field() {
        let material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: r#"<a class="link" href="/post/1">p1</a>"#.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        // 没有 url 类字段，应返回 None
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        let nodes = vec![FieldTreeNode {
            row: FieldNodeRow {
                id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "title".into(), display_name: "title".into(),
                field_type: "string".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":"a.link","attr":"text"}"#.into(),
                post_processors_json: None, script_index: None,
                sort_order: 0, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![],
        }];
        assert!(find_first_detail_url(&nodes, &material).is_none());
    }

    #[test]
    fn find_first_detail_url_pass3_heuristic_picks_url_like_value() {
        // field_type=string 但值是 http URL → 启发式命中
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        let material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: r#"<a class="link" href="https://example.com/full/post/1">p1</a>"#.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        let nodes = vec![FieldTreeNode {
            row: FieldNodeRow {
                id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "any".into(), display_name: "any".into(),
                field_type: "string".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":"a.link","attr":"href"}"#.into(),
                post_processors_json: None, script_index: None,
                sort_order: 0, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![],
        }];
        let url = find_first_detail_url(&nodes, &material).expect("启发式应命中");
        assert_eq!(url, "https://example.com/full/post/1");
    }

    // ===== fetch_detail_sample 的 self-loop 防护（strip_fragment 比对）=====

    #[test]
    fn strip_fragment_removes_trailing_fragment() {
        assert_eq!(strip_fragment("https://x.com/p/1"), "https://x.com/p/1");
        assert_eq!(strip_fragment("https://x.com/p/1#top"), "https://x.com/p/1");
        assert_eq!(strip_fragment("https://x.com/p/1?a=1#top"), "https://x.com/p/1?a=1");
        assert_eq!(strip_fragment(""), "");
    }

    // ===== is_list_self_or_pagination_loop：覆盖用户实际场景 =====

    #[test]
    fn pagination_loop_detects_discuz_style_numeric_suffix() {
        // 用户报告：list=/category/window/，fetch_detail_sample 取到 /category/window/2
        assert!(is_list_self_or_pagination_loop(
            "https://www.08rj.com/category/window/2",
            "https://www.08rj.com/category/window/",
        ));
        // list 无尾斜杠也要命中
        assert!(is_list_self_or_pagination_loop(
            "https://www.08rj.com/category/window/2",
            "https://www.08rj.com/category/window",
        ));
    }

    #[test]
    fn pagination_loop_detects_wordpress_page_prefix() {
        assert!(is_list_self_or_pagination_loop(
            "https://example.com/blog/page/3",
            "https://example.com/blog/",
        ));
    }

    #[test]
    fn pagination_loop_detects_query_page_param() {
        assert!(is_list_self_or_pagination_loop(
            "https://example.com/list?page=2&order=desc",
            "https://example.com/list?order=desc",
        ));
        assert!(is_list_self_or_pagination_loop(
            "https://example.com/list?p=5",
            "https://example.com/list",
        ));
    }

    #[test]
    fn pagination_loop_self_loop_exact_match() {
        assert!(is_list_self_or_pagination_loop(
            "https://example.com/list",
            "https://example.com/list",
        ));
        assert!(is_list_self_or_pagination_loop(
            "https://example.com/list#frag",
            "https://example.com/list",
        ));
    }

    #[test]
    fn pagination_loop_does_not_false_positive_on_real_detail() {
        // 真实详情：路径与 list 不共享前缀
        assert!(!is_list_self_or_pagination_loop(
            "https://example.com/post/123",
            "https://example.com/category/news/",
        ));
        // 真实详情：分类下文章（非纯数字末段）
        assert!(!is_list_self_or_pagination_loop(
            "https://example.com/category/news/hello-world",
            "https://example.com/category/news/",
        ));
        // 真实详情：不同 host
        assert!(!is_list_self_or_pagination_loop(
            "https://detail.example.com/123",
            "https://list.example.com/list",
        ));
        // 真实详情：query 非 page 参数（如 ?id=123）
        assert!(!is_list_self_or_pagination_loop(
            "https://example.com/list?id=123",
            "https://example.com/list",
        ));
    }

    /// link_card 下 url 子字段的 selector 误选了指向列表页本身的链接（href = 列表
    /// URL），resolve 后等于列表 URL → fetch_detail_sample 应识别这种 "self-loop"
    /// 并报错（否则会抓回列表页假装成详情页）。这里验证 find_first_detail_url 返回
    /// 的 url 经 strip_fragment 后等于列表 URL（fetch_detail_sample 会据此报错）。
    #[test]
    fn find_first_detail_url_self_loop_when_href_equals_list_url() {
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        let material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: r##"<div class="card"><a class="bg-white card-hover" href="https://example.com/list">所有</a></div>"##.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        // link_card 父：选 .card 容器；url 子：a.bg-white.card-hover 取 href
        let nodes = vec![FieldTreeNode {
            row: FieldNodeRow {
                id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "link_card".into(), display_name: "链接卡片".into(),
                field_type: "link_card".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":".card","attr":"text"}"#.into(),
                post_processors_json: None, script_index: None,
                sort_order: 0, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![FieldTreeNode {
                row: FieldNodeRow {
                    id: 2, task_id: 1, parent_id: Some(1), scope: "list_page".into(),
                    name: "url".into(), display_name: "链接".into(),
                    field_type: "url".into(),
                    source_layer: "html".into(), extractor_mode: "css".into(),
                    rule_json: r#"{"selector":"a.bg-white.card-hover","attr":"href"}"#.into(),
                    post_processors_json: None, script_index: None,
                    sort_order: 0, is_active: true,
                    created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                    updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                },
                children: vec![],
            }],
        }];
        let url = find_first_detail_url(&nodes, &material).expect("应解析出 URL");
        // resolve 后等于列表 URL 自身（self-loop），fetch_detail_sample 会据此报错
        assert_eq!(
            strip_fragment(&url),
            strip_fragment(&material.final_url),
            "href=list_url resolve 后应等于列表 URL，触发 self-loop 防护"
        );
    }

    /// 用户报告的核心场景（feature 043 实际 bug）：
    /// list_page 字段树中同时配置了：
    ///   - 根级 pagination 字段（"下一页"链接，提取出 /category/window/2）
    ///   - link_card 父字段 + url 子字段（提取出真实详情 /post/123）
    ///
    /// 修复前：Pass 1 包含 Pagination，先命中根级分页字段 → 返回 /category/window/2
    ///         → fetch_detail_sample 报"提取出的详情 URL 看起来还是列表页"
    /// 修复后：Pass 1 排除 Pagination，Pass 2 命中 link_card 的 url 子字段 → /post/123
    #[test]
    fn find_first_detail_url_skips_pagination_uses_link_card_url() {
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        let material = SourceMaterial {
            final_url: "https://www.08rj.com/category/window/".into(),
            status: 200,
            headers: HashMap::new(),
            html: r##"<div class="pg"><a href="/category/window/2">下一页</a></div>
                      <div class="card"><a class="title" href="https://www.08rj.com/post/123">文章标题</a></div>"##.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        // 根级 pagination 字段：会提取出 /category/window/2（分页 URL）
        let pagination = FieldTreeNode {
            row: FieldNodeRow {
                id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "next_page".into(), display_name: "下一页".into(),
                field_type: "pagination".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":".pg a","attr":"href"}"#.into(),
                post_processors_json: Some(r#"[{"op":"absolutize_url"}]"#.into()),
                script_index: None,
                sort_order: 0, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![],
        };
        // link_card 父字段 + url 子字段：提取出真实详情 URL
        let link_card = FieldTreeNode {
            row: FieldNodeRow {
                id: 2, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "link_card".into(), display_name: "链接卡片".into(),
                field_type: "link_card".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":".card","attr":"html"}"#.into(),
                post_processors_json: None, script_index: None,
                sort_order: 1, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![FieldTreeNode {
                row: FieldNodeRow {
                    id: 3, task_id: 1, parent_id: Some(2), scope: "list_page".into(),
                    name: "url".into(), display_name: "链接".into(),
                    field_type: "url".into(),
                    source_layer: "html".into(), extractor_mode: "css".into(),
                    rule_json: r#"{"selector":"a.title","attr":"href"}"#.into(),
                    post_processors_json: Some(r#"[{"op":"absolutize_url"}]"#.into()),
                    script_index: None,
                    sort_order: 0, is_active: true,
                    created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                    updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                },
                children: vec![],
            }],
        };
        // 注意：pagination 在前（sort_order 更小），验证它不会被先选中
        let nodes = vec![pagination, link_card];
        let url = find_first_detail_url(&nodes, &material).expect("应找到详情 URL");
        assert_eq!(
            url, "https://www.08rj.com/post/123",
            "应跳过 pagination 分页字段，使用 link_card 的 url 子字段提取的详情 URL"
        );
    }

    /// Pass 3 启发式兜底也不应把 pagination 字段提取的分页 URL 当成详情 URL
    #[test]
    fn find_first_detail_url_pass3_skips_pagination_field() {
        use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode};
        let material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: r#"<a class="next" href="https://example.com/list?page=2">下一页</a>"#.into(),
            scripts: vec![],
            metas: vec![],
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 10,
        };
        // 只有一个 pagination 字段（无 url / link_card）→ Pass 1 跳过、Pass 2 跳过、
        // Pass 3 也跳过 → 返回 None（让上层报 ParentEmpty 而非拿分页 URL 当详情）
        let nodes = vec![FieldTreeNode {
            row: FieldNodeRow {
                id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
                name: "next".into(), display_name: "下一页".into(),
                field_type: "pagination".into(),
                source_layer: "html".into(), extractor_mode: "css".into(),
                rule_json: r#"{"selector":"a.next","attr":"href"}"#.into(),
                post_processors_json: None, script_index: None,
                sort_order: 0, is_active: true,
                created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
                updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
            },
            children: vec![],
        }];
        assert!(
            find_first_detail_url(&nodes, &material).is_none(),
            "pagination 字段不应被任何 Pass 当成详情 URL 来源"
        );
    }
}
