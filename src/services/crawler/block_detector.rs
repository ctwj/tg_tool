//! 反爬拦截识别（research.md R5）
//!
//! 纯函数 `detect_block(status, body, headers) -> Option<BlockType>`，
//! 识别 5 类拦截信号：
//! - HTTP 403 / 429 / 503
//! - Cloudflare "Just a moment" / "Checking your browser" / cf-chl-bypass
//! - 登录墙（关键词命中）
//! - 验证码（关键词命中）
//! - 空响应（body 长度 < 阈值）
//!
//! 多维信号组合：HTTP 状态码优先；body/headers 关键词作为补充判定。

use serde::{Deserialize, Serialize};

/// 拦截类型（存入 `crawler_run_histories.block_type`）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "code", rename_all = "PascalCase")]
pub enum BlockType {
    /// HTTP 状态码拦截（403 / 429 / 503）
    HttpBlocked(u16),
    /// Cloudflare 5 秒盾 / Challenge
    Cloudflare,
    /// 登录墙
    LoginWall,
    /// 验证码墙
    Captcha,
    /// 空响应或异常短响应
    EmptyResponse,
}

impl BlockType {
    /// 用于存入 `crawler_run_histories.block_type` 的字符串形式
    pub fn as_str(&self) -> String {
        match self {
            BlockType::HttpBlocked(code) => format!("HttpBlocked_{code}"),
            BlockType::Cloudflare => "Cloudflare".to_string(),
            BlockType::LoginWall => "LoginWall".to_string(),
            BlockType::Captcha => "Captcha".to_string(),
            BlockType::EmptyResponse => "EmptyResponse".to_string(),
        }
    }
}

impl std::fmt::Display for BlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

/// 登录墙关键词（小写匹配）。可通过任务的 `block_detection_config` 覆盖。
pub const LOGIN_KEYWORDS: &[&str] = &[
    "登录后",
    "请先登录",
    "请登录",
    "登录查看",
    "sign in",
    "log in first",
    "login required",
    "please log in",
    "you must log in",
    "登录后查看",
];

/// 验证码关键词
pub const CAPTCHA_KEYWORDS: &[&str] = &[
    "captcha",
    "recaptcha",
    "hcaptcha",
    "geetest",
    "极验",
    "滑动验证",
    "图形验证",
    "拖动滑块",
    "人机验证",
];

const EMPTY_BODY_THRESHOLD: usize = 200;

/// 主入口：检测拦截
///
/// `headers` 为响应头（key 不区分大小写，调用方任意大小写均可）
pub fn detect_block(status: u16, body: &str, headers: &[(String, String)]) -> Option<BlockType> {
    // 1. HTTP 状态码优先
    if matches!(status, 403 | 429 | 503) {
        // 进一步：检查是否 Cloudflare
        if is_cloudflare(headers) && has_cloudflare_body(body) {
            return Some(BlockType::Cloudflare);
        }
        return Some(BlockType::HttpBlocked(status));
    }

    let body_lower = body.to_ascii_lowercase();
    let body_len = body.len();

    // 2. Cloudflare Challenge 页（即便状态码不是 403/429/503，也可能是 401/503 的变体）
    if is_cloudflare(headers)
        && (body_lower.contains("just a moment")
            || body_lower.contains("checking your browser")
            || body_lower.contains("cf-chl-bypass"))
    {
        return Some(BlockType::Cloudflare);
    }

    // 3. 登录墙：body 命中关键词
    if LOGIN_KEYWORDS
        .iter()
        .any(|kw| body_lower.contains(&kw.to_ascii_lowercase()))
    {
        return Some(BlockType::LoginWall);
    }

    // 4. 验证码
    if CAPTCHA_KEYWORDS
        .iter()
        .any(|kw| body_lower.contains(&kw.to_ascii_lowercase()))
    {
        return Some(BlockType::Captcha);
    }

    // 5. 空响应（body 极短）
    if body_len < EMPTY_BODY_THRESHOLD {
        // 去除空白后还短 — 更确信是空
        let stripped = body.trim();
        if stripped.len() < 50 {
            return Some(BlockType::EmptyResponse);
        }
    }

    None
}

fn is_cloudflare(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("server") && v.to_ascii_lowercase().contains("cloudflare")
    })
}

fn has_cloudflare_body(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("just a moment")
        || lower.contains("checking your browser")
        || lower.contains("cf-chl-bypass")
        || lower.contains("attention required")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[test]
    fn http_403_blocked() {
        let r = detect_block(403, "<html>forbidden</html>", &[]);
        assert_eq!(r, Some(BlockType::HttpBlocked(403)));
    }

    #[test]
    fn http_429_blocked() {
        let r = detect_block(429, "rate limited", &[]);
        assert_eq!(r, Some(BlockType::HttpBlocked(429)));
    }

    #[test]
    fn http_503_blocked() {
        let r = detect_block(503, "service unavailable", &[]);
        assert_eq!(r, Some(BlockType::HttpBlocked(503)));
    }

    #[test]
    fn http_200_with_no_block_returns_none() {
        // 足够长（>200 bytes）且不含拦截关键词
        let body = format!("<html><body>{}</body></html>", "x".repeat(300));
        let r = detect_block(200, &body, &[]);
        assert_eq!(r, None);
    }

    #[test]
    fn cloudflare_403_detected() {
        let headers = vec![h("Server", "cloudflare")];
        let body = "<html><title>Just a moment...</title></html>";
        let r = detect_block(403, body, &headers);
        assert_eq!(r, Some(BlockType::Cloudflare));
    }

    #[test]
    fn cloudflare_body_with_header_even_200() {
        // 某些情况下 CF 返回 200 但 challenge 页
        let headers = vec![h("Server", "cloudflare")];
        let body = "Checking your browser before accessing";
        let r = detect_block(200, body, &headers);
        assert_eq!(r, Some(BlockType::Cloudflare));
    }

    #[test]
    fn login_wall_chinese() {
        let body = "<html><body>请先登录后查看完整内容</body></html>";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::LoginWall));
    }

    #[test]
    fn login_wall_english() {
        let body = "<html><body>Please log in first to continue</body></html>";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::LoginWall));
    }

    #[test]
    fn captcha_detected() {
        let body = "<html><body><div class='recaptcha'></div></body></html>";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::Captcha));
    }

    #[test]
    fn captcha_chinese_slider() {
        let body = "<html><body>请完成滑动验证</body></html>";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::Captcha));
    }

    #[test]
    fn empty_response_detected() {
        let body = "";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::EmptyResponse));
    }

    #[test]
    fn whitespace_only_response_detected() {
        let body = "   \n\t  ";
        let r = detect_block(200, body, &[]);
        assert_eq!(r, Some(BlockType::EmptyResponse));
    }

    #[test]
    fn normal_short_response_not_blocked() {
        // 100 字符但有实质内容 — 不算空
        let body = "x".repeat(100);
        let r = detect_block(200, &body, &[]);
        // 仅 100 字符且无关键词 — 但 trim 后 100 > 50，不触发 EmptyResponse
        assert_eq!(r, None);
    }

    #[test]
    fn block_type_as_str_formats() {
        assert_eq!(BlockType::HttpBlocked(403).as_str(), "HttpBlocked_403");
        assert_eq!(BlockType::Cloudflare.as_str(), "Cloudflare");
        assert_eq!(BlockType::LoginWall.as_str(), "LoginWall");
        assert_eq!(BlockType::Captcha.as_str(), "Captcha");
        assert_eq!(BlockType::EmptyResponse.as_str(), "EmptyResponse");
    }

    #[test]
    fn http_403_without_cf_header_returns_plain_block() {
        // 403 但非 CF — HttpBlocked
        let r = detect_block(403, "forbidden", &[]);
        assert_eq!(r, Some(BlockType::HttpBlocked(403)));
    }
}
