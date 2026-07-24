// 链接类型识别（feature 047 US2 — FR-005）
// 区分网盘分享（夸克/UC/百度等）与直链，解析 pwd_id 与提取码

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum SourceType {
    PanShare,
    DirectLink,
    Unknown,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceType::PanShare => "pan_share",
            SourceType::DirectLink => "direct_link",
            SourceType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedLink {
    pub source_type: SourceType,
    pub platform: Option<String>, // quark | uc | baidu | aliyun
    pub pwd_id: Option<String>,
    pub passcode: Option<String>,
}

/// 解析链接：extract_code 优先于 URL 内 ?pwd=
pub fn parse(raw: &str, extract_code: Option<&str>) -> ParsedLink {
    let url = raw.trim();
    if url.is_empty() {
        return ParsedLink {
            source_type: SourceType::Unknown,
            platform: None,
            pwd_id: None,
            passcode: None,
        };
    }

    let passcode = extract_code
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| extract_pwd_from_query(url));

    if let Some((platform, pwd_id)) = parse_pan_share(url) {
        return ParsedLink {
            source_type: SourceType::PanShare,
            platform: Some(platform),
            pwd_id: Some(pwd_id),
            passcode,
        };
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        return ParsedLink {
            source_type: SourceType::DirectLink,
            platform: None,
            pwd_id: None,
            passcode: None,
        };
    }

    ParsedLink {
        source_type: SourceType::Unknown,
        platform: None,
        pwd_id: None,
        passcode: None,
    }
}

fn extract_pwd_from_query(url: &str) -> Option<String> {
    let idx = url.find("pwd=")?;
    let rest = &url[idx + 4..];
    let end = rest
        .find(['&', '#', ' ', '/'])
        .unwrap_or(rest.len());
    let v = &rest[..end];
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_pan_share(url: &str) -> Option<(String, String)> {
    let platform = if url.contains("quark.cn") {
        "quark"
    } else if url.contains("drive.uc.cn") || url.contains("uc.cn") {
        "uc"
    } else if url.contains("pan.baidu.com") || url.contains("yun.baidu.com") {
        "baidu"
    } else if url.contains("aliyundrive.com") || url.contains("alipan.com") {
        "aliyun"
    } else {
        return None;
    };
    let marker = "/s/";
    let start = url.find(marker)? + marker.len();
    let rest = &url[start..];
    let end = rest
        .find(['?', '#', '/', ' ', '&'])
        .unwrap_or(rest.len());
    let pwd_id = &rest[..end];
    if pwd_id.is_empty() {
        return None;
    }
    Some((platform.to_string(), pwd_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quark_share_with_pwd_in_url() {
        let p = parse("https://pan.quark.cn/s/abcdef1234?pwd=xyz9", None);
        assert_eq!(p.source_type, SourceType::PanShare);
        assert_eq!(p.platform.as_deref(), Some("quark"));
        assert_eq!(p.pwd_id.as_deref(), Some("abcdef1234"));
        assert_eq!(p.passcode.as_deref(), Some("xyz9"));
    }

    #[test]
    fn test_quark_share_with_extract_code_param() {
        let p = parse("https://pan.quark.cn/s/abcdef1234", Some("mypwd"));
        assert_eq!(p.pwd_id.as_deref(), Some("abcdef1234"));
        assert_eq!(p.passcode.as_deref(), Some("mypwd"));
    }

    #[test]
    fn test_extract_code_overrides_url_pwd() {
        let p = parse("https://pan.quark.cn/s/abc?pwd=urlcode", Some("paramcode"));
        assert_eq!(p.passcode.as_deref(), Some("paramcode"));
    }

    #[test]
    fn test_uc_share() {
        let p = parse("https://drive.uc.cn/s/ucshareid", None);
        assert_eq!(p.source_type, SourceType::PanShare);
        assert_eq!(p.platform.as_deref(), Some("uc"));
        assert_eq!(p.pwd_id.as_deref(), Some("ucshareid"));
    }

    #[test]
    fn test_baidu_share() {
        let p = parse("https://pan.baidu.com/s/1aBcDeFgH?pwd=a1b2", None);
        assert_eq!(p.platform.as_deref(), Some("baidu"));
        assert_eq!(p.pwd_id.as_deref(), Some("1aBcDeFgH"));
        assert_eq!(p.passcode.as_deref(), Some("a1b2"));
    }

    #[test]
    fn test_direct_link() {
        let p = parse("https://example.com/files/movie.mp4", None);
        assert_eq!(p.source_type, SourceType::DirectLink);
        assert!(p.platform.is_none());
        assert!(p.pwd_id.is_none());
    }

    #[test]
    fn test_unknown_empty() {
        assert_eq!(parse("", None).source_type, SourceType::Unknown);
        assert_eq!(parse("   ", None).source_type, SourceType::Unknown);
    }

    #[test]
    fn test_unknown_non_url() {
        assert_eq!(parse("not-a-link", None).source_type, SourceType::Unknown);
    }

    #[test]
    fn test_pan_share_without_pwd_id_invalid() {
        // /s/ 后紧跟结束符 → 无效分享
        let p = parse("https://pan.quark.cn/s/?pwd=abc", None);
        assert_eq!(p.source_type, SourceType::DirectLink); // 无 pwd_id 退化为直链判定
    }

    #[test]
    fn test_pwd_id_trailing_slash_cut() {
        let p = parse("https://pan.quark.cn/s/abcdef/", None);
        assert_eq!(p.pwd_id.as_deref(), Some("abcdef"));
    }
}
