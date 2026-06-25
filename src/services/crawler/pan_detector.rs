//! 9 平台网盘识别 + 提取码邻近文本关联（research.md R6）
//!
//! 与 PanCheck（`src/services/link_checker.rs:65`）的 9 平台完全对齐：
//! `quark` / `uc` / `baidu` / `tianyi` / `123pan` / `115` / `aliyun` / `xunlei` / `mobile`

use serde::{Deserialize, Serialize};

/// 网盘品牌（字符串值与 PanCheck 配置保持一致，存入 `crawler_article_links.platform`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "quark")]
    Quark,
    #[serde(rename = "uc")]
    Uc,
    #[serde(rename = "baidu")]
    Baidu,
    #[serde(rename = "tianyi")]
    Tianyi,
    #[serde(rename = "123pan")]
    Pan123,
    #[serde(rename = "115")]
    Pan115,
    #[serde(rename = "aliyun")]
    Aliyun,
    #[serde(rename = "xunlei")]
    Xunlei,
    #[serde(rename = "mobile")]
    Mobile,
}

impl Platform {
    /// 序列化为字符串（与 PanCheck 配置 / DB 存储一致）
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Quark => "quark",
            Platform::Uc => "uc",
            Platform::Baidu => "baidu",
            Platform::Tianyi => "tianyi",
            Platform::Pan123 => "123pan",
            Platform::Pan115 => "115",
            Platform::Aliyun => "aliyun",
            Platform::Xunlei => "xunlei",
            Platform::Mobile => "mobile",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 识别结果：品牌 + 可选提取码
pub type Detection = (Platform, Option<String>);

/// 主入口：判断单个 URL 是否命中 9 平台之一。
///
/// 注意：本函数只识别品牌，不提取提取码。
/// 提取码需结合链接节点的邻近文本，使用 [`find_extract_code`]。
pub fn detect_platform(url: &str) -> Option<Platform> {
    let lower = url.to_ascii_lowercase();
    // host 提取（scheme:// 后第一段，到第一个 / 或 ? 或 # 止）
    let host = extract_host(&lower)?;

    // 注意顺序：更具体的子串优先，避免误命中
    if host.contains("pan.quark.cn") {
        return Some(Platform::Quark);
    }
    if host.contains("drive.uc.cn") || host.contains("pan.uc.cn") {
        return Some(Platform::Uc);
    }
    if host.contains("pan.baidu.com") || host.contains("yun.baidu.com") {
        return Some(Platform::Baidu);
    }
    if host.contains("cloud.189.cn") || host.contains("pan.189.cn") {
        return Some(Platform::Tianyi);
    }
    if host.contains("123912.com")
        || host.contains("123pan.com")
        || host.contains("www.123pan.cn")
    {
        return Some(Platform::Pan123);
    }
    if host.contains("115.com") {
        return Some(Platform::Pan115);
    }
    if host.contains("alipan.com")
        || host.contains("aliyundrive.com")
        || host.contains("aliyunpan.com")
    {
        return Some(Platform::Aliyun);
    }
    if host.contains("pan.xunlei.com") || host.contains("xunlei.com") {
        return Some(Platform::Xunlei);
    }
    if host.contains("caiyun.139.com")
        || host.contains("yun.139.com")
        || host.ends_with(".139.com")
    {
        return Some(Platform::Mobile);
    }
    None
}

/// 从给定"邻近文本"中提取提取码。
///
/// 规则（FR-025）：
/// - 正则匹配 `(?:提取码|密码|访问码|password|code|pwd)[:：\s]*([A-Za-z0-9]{4,6})`
/// - 多个匹配时返回第一个（调用方可按距离筛选邻近文本后再传入）
pub fn find_extract_code(surrounding_text: &str) -> Option<String> {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)(?:提取码|密码|访问码|提取碼|密碼|password|passwd|code|pwd)[\s:：]*([A-Za-z0-9]{4,8})",
        )
        .expect("extract_code regex")
    });
    RE.captures(surrounding_text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

/// 一站式：识别品牌 + 从邻近文本提取提取码。
///
/// 返回 `(platform, Some(code))` 或 `(platform, None)`。
/// 若 URL 不是网盘链接，返回 `None`。
pub fn detect(url: &str, surrounding_text: &str) -> Option<Detection> {
    let platform = detect_platform(url)?;
    let code = find_extract_code(surrounding_text);
    Some((platform, code))
}

/// 直链扩展名白名单（FR-024 中 link_type=direct）。小写匹配，不含点。
const DIRECT_LINK_EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "exe", "dmg", "pkg", "iso", "tar", "gz", "bz2", "xz", "mkv", "mp4", "mov",
    "avi", "flv", "wmv", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "apk", "ipa", "mp3",
    "flac", "wav", "epub", "mobi", "azw3", "txt",
];

/// 判断给定 URL 是否为直链（非网盘、扩展名命中白名单）
pub fn is_direct_link(url: &str) -> bool {
    if detect_platform(url).is_some() {
        return false;
    }
    // 提取 path 末尾扩展名（忽略 query/fragment）
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    let ext = match no_query.rsplit('.').next() {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    // 简单防御：rsplit('.') 也会切域名最后一部分；若 ext 长度 > 6 或含 /，视为非扩展名
    if ext.len() > 6 || ext.contains('/') {
        return false;
    }
    DIRECT_LINK_EXTENSIONS.contains(&ext.as_str())
}

/// 简易 host 提取：仅支持 `scheme://host[:port]/...`，失败返回 None
fn extract_host(lower_url: &str) -> Option<&str> {
    let after_scheme = lower_url.split_once("://")?.1;
    let auth_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..auth_end];
    // 去 userinfo
    let host_port = match authority.rsplit_once('@') {
        Some((_, hp)) => hp,
        None => authority,
    };
    if host_port.is_empty() {
        return None;
    }
    Some(host_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quark_detected() {
        assert_eq!(
            detect_platform("https://pan.quark.cn/s/abcdefg"),
            Some(Platform::Quark)
        );
    }

    #[test]
    fn uc_detected() {
        assert_eq!(
            detect_platform("https://drive.uc.cn/s/xyz"),
            Some(Platform::Uc)
        );
    }

    #[test]
    fn baidu_detected() {
        assert_eq!(
            detect_platform("https://pan.baidu.com/s/1a-12345?pwd=abc"),
            Some(Platform::Baidu)
        );
    }

    #[test]
    fn tianyi_detected() {
        assert_eq!(
            detect_platform("https://cloud.189.cn/t/abcdefg"),
            Some(Platform::Tianyi)
        );
    }

    #[test]
    fn pan123_both_domains_detected() {
        assert_eq!(
            detect_platform("https://www.123912.com/s/abc"),
            Some(Platform::Pan123)
        );
        assert_eq!(
            detect_platform("https://123pan.com/s/def"),
            Some(Platform::Pan123)
        );
    }

    #[test]
    fn pan115_detected() {
        assert_eq!(
            detect_platform("https://115.com/s/abcdefg"),
            Some(Platform::Pan115)
        );
    }

    #[test]
    fn aliyun_alipan_detected() {
        assert_eq!(
            detect_platform("https://www.alipan.com/s/abc"),
            Some(Platform::Aliyun)
        );
    }

    #[test]
    fn aliyun_aliyundrive_detected() {
        assert_eq!(
            detect_platform("https://www.aliyundrive.com/s/abc"),
            Some(Platform::Aliyun)
        );
    }

    #[test]
    fn xunlei_detected() {
        assert_eq!(
            detect_platform("https://pan.xunlei.com/s/abcdef"),
            Some(Platform::Xunlei)
        );
    }

    #[test]
    fn mobile_caiyun_detected() {
        assert_eq!(
            detect_platform("https://caiyun.139.com/m/abc"),
            Some(Platform::Mobile)
        );
    }

    #[test]
    fn non_pan_url_returns_none() {
        assert!(detect_platform("https://example.com/something").is_none());
        assert!(detect_platform("https://www.google.com/search?q=test").is_none());
    }

    #[test]
    fn invalid_url_returns_none() {
        assert!(detect_platform("not a url").is_none());
        assert!(detect_platform("").is_none());
    }

    // 提取码相关测试
    #[test]
    fn extract_code_chinese_label() {
        let code = find_extract_code("提取码: abcd").unwrap();
        assert_eq!(code, "abcd");
    }

    #[test]
    fn extract_code_password_label_english() {
        let code = find_extract_code("password： XY12ab").unwrap();
        assert_eq!(code, "XY12ab");
    }

    #[test]
    fn extract_code_with_space_after_label() {
        let code = find_extract_code("pwd 1234").unwrap();
        assert_eq!(code, "1234");
    }

    #[test]
    fn extract_code_not_found() {
        assert!(find_extract_code("just some text").is_none());
    }

    #[test]
    fn detect_combines_platform_and_code() {
        let r = detect("https://pan.quark.cn/s/abc", "提取码：wxyz").unwrap();
        assert_eq!(r.0, Platform::Quark);
        assert_eq!(r.1.as_deref(), Some("wxyz"));
    }

    #[test]
    fn detect_returns_none_for_non_pan() {
        assert!(detect("https://example.com", "").is_none());
    }

    // 直链测试
    #[test]
    fn is_direct_link_zip() {
        assert!(is_direct_link("https://example.com/file.zip"));
    }

    #[test]
    fn is_direct_link_pdf_with_query() {
        assert!(is_direct_link("https://example.com/x.pdf?token=1"));
    }

    #[test]
    fn is_direct_link_false_for_pan() {
        assert!(!is_direct_link("https://pan.quark.cn/s/abc"));
    }

    #[test]
    fn is_direct_link_false_for_html() {
        assert!(!is_direct_link("https://example.com/page.html"));
    }

    #[test]
    fn platform_serializes_to_lowercase_string() {
        let json = serde_json::to_string(&Platform::Pan123).unwrap();
        assert_eq!(json, "\"123pan\"");
        let p: Platform = serde_json::from_str("\"quark\"").unwrap();
        assert_eq!(p, Platform::Quark);
    }
}
