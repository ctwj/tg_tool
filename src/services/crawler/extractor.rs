//! HTML 字段提取（research.md R1）
//!
//! 基于 `scraper` crate：先 CSS 选择器，后正则后处理。单字段失败不中断整条文章（FR-024）。
//!
//! 配置入口为 [`FieldSelectors`]，对应任务 `selectors` JSON 字段（research.md R9）。
//! 输出为 [`ExtractedFields`]，每个字段为 `Option<String>` 或 `Vec<String>`，
//! 调用方据此判断是否"部分成功"。

use serde::{Deserialize, Serialize};

/// 单字段的"CSS + 可选正则"配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldSelector {
    /// CSS 选择器（必填；为空表示该字段不抓取）
    #[serde(default)]
    pub css: String,
    /// 取节点的哪个属性（默认取文本）：`text` / `html` / 任意属性名如 `href`/`src`
    #[serde(default)]
    pub attr: Option<String>,
    /// 可选正则后处理：从 css 取到的内容中匹配/提取
    /// - 若包含捕获组 `(...)` → 取第一个捕获组
    /// - 否则视为"匹配命中即保留原值"
    #[serde(default)]
    pub regex: Option<String>,
}

/// 完整字段选择器集合（任务 `selectors` JSON）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldSelectors {
    /// 列表页：单个列表项容器
    #[serde(default)]
    pub list_item: String,
    /// 列表页：详情页链接（在 list_item 内查找）
    #[serde(default)]
    pub detail_link: String,
    /// 详情链接取的属性（默认 href）
    #[serde(default)]
    pub detail_link_attr: Option<String>,

    // 详情页字段（每个字段独立 css + 可选 regex）
    #[serde(default)]
    pub title: FieldSelector,
    #[serde(default)]
    pub content: FieldSelector,
    #[serde(default)]
    pub category: FieldSelector,
    #[serde(default)]
    pub tags: FieldSelector,
    /// 图片列表（多匹配，attr=src）
    #[serde(default)]
    pub images: FieldSelector,
    /// 网盘/资源链接列表（多匹配；body 透传给 pan_detector 进一步过滤）
    #[serde(default)]
    pub pan_links: FieldSelector,
    /// 直链列表（多匹配）
    #[serde(default)]
    pub direct_links: FieldSelector,
}

/// 字段提取结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedFields {
    pub title: Option<String>,
    pub content: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    /// 图片 URL 列表（已去重）
    pub images: Vec<String>,
    /// pan_links 区域的所有 <a> 链接（交由 pan_detector 二次识别品牌）
    pub pan_links: Vec<String>,
    /// direct_links 区域的所有 <a> 链接（交由 pan_detector::is_direct_link 二次过滤）
    pub direct_links: Vec<String>,
    /// 哪些字段未命中（用于 `test_run` 预览的 `field_warnings`）
    pub field_warnings: Vec<String>,
}

/// 详情页字段提取主入口
///
/// 单字段失败不 panic — 解析失败的字段写入 `field_warnings` 并继续处理其他字段（FR-024）。
pub fn extract_fields(html: &str, selectors: &FieldSelectors) -> ExtractedFields {
    let document = scraper::Html::parse_document(html);
    let mut out = ExtractedFields::default();

    // title
    out.title = extract_single(&document, &selectors.title, "title", &mut out.field_warnings);
    // content（取 HTML 片段，保留原样供前端渲染）
    out.content = extract_single(&document, &selectors.content, "content", &mut out.field_warnings);
    out.category = extract_single(
        &document,
        &selectors.category,
        "category",
        &mut out.field_warnings,
    );
    out.tags = extract_single(&document, &selectors.tags, "tags", &mut out.field_warnings);
    // images
    out.images = extract_multi(&document, &selectors.images, "images", &mut out.field_warnings);
    // pan_links / direct_links
    out.pan_links = extract_multi(
        &document,
        &selectors.pan_links,
        "pan_links",
        &mut out.field_warnings,
    );
    out.direct_links = extract_multi(
        &document,
        &selectors.direct_links,
        "direct_links",
        &mut out.field_warnings,
    );

    out
}

/// 从列表页 HTML 抽取所有详情链接
///
/// - 对每个 `list_item` 选择器命中的节点，从中查 `detail_link` 选择器的 `<a>`
/// - 取 `detail_link_attr`（默认 `href`），解析为绝对/相对 URL 原样返回
/// - 单条失败跳过
pub fn extract_detail_links(
    html: &str,
    list_item_css: &str,
    detail_link_css: &str,
    detail_link_attr: Option<&str>,
) -> Vec<String> {
    if list_item_css.is_empty() || detail_link_css.is_empty() {
        return Vec::new();
    }
    let document = scraper::Html::parse_document(html);
    let item_sel = match scraper::Selector::parse(list_item_css) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(?e, list_item_css, "list_item selector parse failed");
            return Vec::new();
        }
    };
    let link_sel = match scraper::Selector::parse(detail_link_css) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(?e, detail_link_css, "detail_link selector parse failed");
            return Vec::new();
        }
    };
    let attr_name = detail_link_attr.unwrap_or("href");

    let mut out = Vec::new();
    for item in document.select(&item_sel) {
        for link in item.select(&link_sel) {
            if let Some(v) = link.value().attr(attr_name)
                && !v.is_empty()
            {
                out.push(v.to_string());
            }
        }
    }
    out
}

fn extract_single(
    document: &scraper::Html,
    field: &FieldSelector,
    field_name: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    if field.css.is_empty() {
        return None;
    }
    let sel = match scraper::Selector::parse(&field.css) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!(
                "{field_name}: selector `{}` invalid: {e}",
                field.css
            ));
            return None;
        }
    };
    let mut first: Option<String> = None;
    for el in document.select(&sel) {
        let raw = pick_value(&el, field);
        let processed = apply_regex(&raw, field.regex.as_deref());
        if let Some(v) = processed {
            first = Some(v);
            break;
        }
    }
    if first.is_none() {
        warnings.push(format!("{field_name}: selector `{}` no match", field.css));
    }
    first
}

fn extract_multi(
    document: &scraper::Html,
    field: &FieldSelector,
    field_name: &str,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    if field.css.is_empty() {
        return Vec::new();
    }
    let sel = match scraper::Selector::parse(&field.css) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!(
                "{field_name}: selector `{}` invalid: {e}",
                field.css
            ));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for el in document.select(&sel) {
        let raw = pick_value(&el, field);
        let processed = apply_regex(&raw, field.regex.as_deref());
        if let Some(v) = processed {
            let key = v.clone();
            if seen.insert(key) {
                out.push(v);
            }
        }
    }
    if out.is_empty() {
        warnings.push(format!("{field_name}: selector `{}` no match", field.css));
    }
    out
}

fn pick_value(el: &scraper::ElementRef, field: &FieldSelector) -> String {
    match field.attr.as_deref() {
        Some("text") | None => el.text().collect::<String>(),
        Some("html") => el.html(),
        Some(name) => el.value().attr(name).unwrap_or("").to_string(),
    }
}

/// 应用正则后处理：
/// - None → 返回原值（trim 后非空即保留）
/// - 有捕获组 → 返回第一个捕获组（多次匹配取第一个）
/// - 无捕获组但匹配 → 返回原值
/// - 匹配失败 → None
fn apply_regex(input: &str, pattern: Option<&str>) -> Option<String> {
    let trimmed = input.trim();
    let pattern = match pattern {
        Some(p) if !p.is_empty() => p,
        _ => {
            return if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
    };
    let re = regex::Regex::new(pattern).ok()?;
    let caps = re.captures(trimmed)?;
    // 优先取第一个捕获组；无组则取整个匹配
    if let Some(g) = caps.get(1) {
        let v = g.as_str().trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    } else {
        let m = caps.get(0)?.as_str().trim();
        if m.is_empty() {
            None
        } else {
            Some(m.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DETAIL_HTML: &str = r#"
<!DOCTYPE html>
<html><head><title>Page Title</title></head>
<body>
  <h1 class="title">My Article Title</h1>
  <div class="category">Tech / Rust</div>
  <div class="content">
    <p>First paragraph.</p>
    <p>Second paragraph.</p>
    <img src="https://img.example.com/a.jpg" />
    <img src="https://img.example.com/b.jpg" />
    <img src="/relative/c.jpg" />
  </div>
  <div class="resources">
    <a href="https://pan.quark.cn/s/abc">Quark</a>
    <a href="https://drive.uc.cn/s/xyz">UC</a>
    <a href="https://example.com/file.zip">Direct</a>
  </div>
</body></html>
"#;

    fn fs(css: &str) -> FieldSelector {
        FieldSelector {
            css: css.to_string(),
            attr: None,
            regex: None,
        }
    }

    fn fs_attr(css: &str, attr: &str) -> FieldSelector {
        FieldSelector {
            css: css.to_string(),
            attr: Some(attr.to_string()),
            regex: None,
        }
    }

    #[test]
    fn title_hit() {
        let selectors = FieldSelectors {
            title: fs("h1.title"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.title.as_deref(), Some("My Article Title"));
        assert!(out.field_warnings.is_empty());
    }

    #[test]
    fn content_html_extracted() {
        let selectors = FieldSelectors {
            content: fs_attr("div.content", "html"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        let content = out.content.expect("content");
        assert!(content.contains("First paragraph"));
        assert!(content.contains("Second paragraph"));
    }

    #[test]
    fn images_multi_dedup() {
        let selectors = FieldSelectors {
            images: fs_attr("div.content img", "src"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.images.len(), 3);
        assert!(out.images.contains(&"https://img.example.com/a.jpg".to_string()));
        assert!(out.images.contains(&"/relative/c.jpg".to_string()));
    }

    #[test]
    fn missing_field_recorded_as_warning() {
        let selectors = FieldSelectors {
            title: fs("h1.nonexistent"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert!(out.title.is_none());
        assert!(out.field_warnings.iter().any(|w| w.contains("title")));
    }

    #[test]
    fn invalid_selector_does_not_panic() {
        let selectors = FieldSelectors {
            title: FieldSelector {
                css: ">>>invalid<<<".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert!(out.title.is_none());
        assert!(out.field_warnings.iter().any(|w| w.contains("invalid")));
    }

    #[test]
    fn regex_with_capture_group() {
        let selectors = FieldSelectors {
            category: FieldSelector {
                css: "div.category".to_string(),
                regex: Some(r"(\w+)\s*/".to_string()), // capture first word before /
                ..Default::default()
            },
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.category.as_deref(), Some("Tech"));
    }

    #[test]
    fn regex_no_capture_returns_match() {
        let selectors = FieldSelectors {
            title: FieldSelector {
                css: "h1.title".to_string(),
                regex: Some(r"Article".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.title.as_deref(), Some("Article"));
    }

    #[test]
    fn regex_no_match_returns_none() {
        let selectors = FieldSelectors {
            title: FieldSelector {
                css: "h1.title".to_string(),
                regex: Some(r"NonExistent".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert!(out.title.is_none());
    }

    #[test]
    fn multi_field_partial_failure_does_not_abort() {
        // title 命中 + content 未配置 + category 失败 — title 仍要拿到
        let selectors = FieldSelectors {
            title: fs("h1.title"),
            category: fs(".nonexistent"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.title.as_deref(), Some("My Article Title"));
        assert!(out.field_warnings.iter().any(|w| w.contains("category")));
    }

    #[test]
    fn links_collected_from_pan_links_region() {
        let selectors = FieldSelectors {
            pan_links: fs_attr("div.resources a", "href"),
            ..Default::default()
        };
        let out = extract_fields(SAMPLE_DETAIL_HTML, &selectors);
        assert_eq!(out.pan_links.len(), 3);
    }

    // extract_detail_links 列表页测试
    const LIST_HTML: &str = r#"
<html><body>
  <ul class="list">
    <li class="item"><a href="/p/1">Post 1</a></li>
    <li class="item"><a href="/p/2">Post 2</a></li>
    <li class="other"><a href="/p/3">Skipped</a></li>
  </ul>
</body></html>
"#;

    #[test]
    fn extract_detail_links_basic() {
        let links = extract_detail_links(LIST_HTML, "li.item", "a", None);
        assert_eq!(links, vec!["/p/1", "/p/2"]);
    }

    #[test]
    fn extract_detail_links_custom_attr() {
        // 实际场景：item 容器内部的 <a> 用自定义属性存链接
        let html = r#"<div class="card"><a class="card-link" data-url="/x/1">X</a></div>"#;
        let links = extract_detail_links(html, ".card", "a.card-link", Some("data-url"));
        assert_eq!(links, vec!["/x/1"]);
    }

    #[test]
    fn extract_detail_links_empty_selectors_returns_empty() {
        let links = extract_detail_links(LIST_HTML, "", "a", None);
        assert!(links.is_empty());
    }

    #[test]
    fn extract_detail_links_invalid_selector_returns_empty() {
        let links = extract_detail_links(LIST_HTML, ">>>", "a", None);
        assert!(links.is_empty());
    }
}
