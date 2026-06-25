//! URL 规范化（research.md R2）
//!
//! 用于爬虫文章的去重唯一键（`source_url_canonical`），保证同一文章多次抓取不重复入库（FR-022）。
//!
//! 规则：
//! 1. 解析失败 → 原样返回（不阻塞流程）
//! 2. scheme + host 小写
//! 3. 去掉 fragment
//! 4. 去掉追踪类 query 参数：utm_*、spm、share_*、wechat_*
//! 5. query 参数按 key 字典序排序
//! 6. 去掉末尾冗余斜杠（根路径 `/` 保留）
//! 7. 不做 host → IP 解析（避免 CDN 节点差异）

/// 追踪类 query 参数前缀/全名白名单（小写匹配）
const TRACKING_PARAM_PREFIXES: &[&str] = &["utm_", "share_", "wechat_"];

/// 追踪类 query 参数全名白名单
const TRACKING_PARAM_EXACT: &[&str] = &["spm", "ref", "from", "isappinstalled", "nsukey", "st"];

/// 规范化 URL；解析失败时原样返回
pub fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return input.to_string();
    }

    // 手工解析：避免引入 url crate 依赖
    // 形如 scheme://[userinfo@]host[:port]/path?query#fragment
    let (scheme, rest) = match split_scheme(trimmed) {
        Some(parts) => parts,
        // 无 scheme：协议相对 //host/... 或相对路径 — 原样返回（爬虫场景应总能拿到绝对 URL）
        None => return trimmed.to_string(),
    };
    let scheme = scheme.to_ascii_lowercase();

    // 分离 fragment
    let (before_frag, _frag) = match rest.find('#') {
        Some(idx) => (&rest[..idx], Some(&rest[idx..])),
        None => (rest, None),
    };

    // 分离 authority/path 与 query
    let (auth_path, query) = match before_frag.find('?') {
        Some(idx) => (&before_frag[..idx], Some(&before_frag[idx + 1..])),
        None => (before_frag, None),
    };

    // 分离 authority 与 path（authority 是 // 开头的部分）
    let (authority, path) = if let Some(stripped) = auth_path.strip_prefix("//") {
        match stripped.find('/') {
            Some(idx) => {
                let auth = &stripped[..idx];
                let p = &stripped[idx..]; // 含前导 /
                (Some(auth), Some(p))
            }
            None => (Some(stripped), None),
        }
    } else {
        // 非 //开头（如 /abs/path 或相对路径）— authority 为空，整体当 path
        (None, Some(auth_path))
    };

    // authority 小写（host 部分；userinfo 暂时一并小写，足够去重用途）
    let authority_norm = authority.map(|a| a.to_ascii_lowercase());

    // path 去末尾冗余斜杠（根 / 保留）
    let path_norm = path.map(|p| {
        if p.len() > 1 && p.ends_with('/') {
            p[..p.len() - 1].to_string()
        } else {
            p.to_string()
        }
    });

    // query 清洗 + 排序
    let query_norm = query.map(|q| {
        let pairs: Vec<(String, String)> = q
            .split('&')
            .filter(|kv| !kv.is_empty())
            .filter_map(|kv| {
                let (k, v) = match kv.find('=') {
                    Some(idx) => (&kv[..idx], &kv[idx + 1..]),
                    None => (kv, ""),
                };
                if is_tracking_param(k) {
                    None
                } else {
                    Some((k.to_string(), v.to_string()))
                }
            })
            .collect();
        if pairs.is_empty() {
            return String::new();
        }
        let mut sorted = pairs;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        sorted
            .into_iter()
            .map(|(k, v)| if v.is_empty() { k } else { format!("{k}={v}") })
            .collect::<Vec<_>>()
            .join("&")
    });

    // 拼装
    let mut out = String::new();
    out.push_str(&scheme);
    out.push(':');
    if let Some(auth) = &authority_norm {
        out.push_str("//");
        out.push_str(auth);
    }
    if let Some(p) = &path_norm {
        out.push_str(p);
    }
    if let Some(Some(q)) = Some(query_norm.as_ref())
        && !q.is_empty()
    {
        out.push('?');
        out.push_str(q);
    }
    out
}

/// 从 `scheme:rest` 中分离（要求 scheme 为 [a-zA-Z][a-zA-Z0-9+.-]*）
fn split_scheme(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let first = bytes[0];
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b':' {
            // 至少 1 字符 scheme
            if i == 1 {
                return None;
            }
            // scheme 只允许 alpha / digit / + / - / .
            return Some((&s[..i], &s[i + 1..]));
        }
        if !c.is_ascii_alphanumeric() && c != b'+' && c != b'-' && c != b'.' {
            return None;
        }
        i += 1;
    }
    None
}

fn is_tracking_param(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    if TRACKING_PARAM_EXACT.contains(&lower.as_str()) {
        return true;
    }
    TRACKING_PARAM_PREFIXES.iter().any(|p| lower.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_utm_params() {
        let r = normalize_url("https://example.com/a?utm_source=x&utm_medium=y&id=42");
        assert_eq!(r, "https://example.com/a?id=42");
    }

    #[test]
    fn sorts_query_params() {
        // b=2&a=1 → a=1&b=2
        let r = normalize_url("https://example.com/p?b=2&a=1&c=3");
        assert_eq!(r, "https://example.com/p?a=1&b=2&c=3");
    }

    #[test]
    fn lowercases_scheme_and_host() {
        let r = normalize_url("HTTPS://Example.COM/Path");
        assert_eq!(r, "https://example.com/Path");
    }

    #[test]
    fn removes_fragment() {
        let r = normalize_url("https://example.com/a#section");
        assert_eq!(r, "https://example.com/a");
    }

    #[test]
    fn removes_trailing_slash_non_root() {
        let r = normalize_url("https://example.com/a/b/");
        assert_eq!(r, "https://example.com/a/b");
    }

    #[test]
    fn preserves_root_trailing_slash() {
        let r = normalize_url("https://example.com/");
        assert_eq!(r, "https://example.com/");
    }

    #[test]
    fn invalid_url_returned_as_is() {
        // 无 scheme — 返回原样
        let r = normalize_url("not-a-url");
        assert_eq!(r, "not-a-url");
    }

    #[test]
    fn empty_string_passthrough() {
        assert_eq!(normalize_url(""), "");
        assert_eq!(normalize_url("   "), "   ");
    }

    #[test]
    fn share_and_spm_params_removed() {
        let r = normalize_url("https://example.com/x?share_from=weixin&spm=1001&keep=1");
        assert_eq!(r, "https://example.com/x?keep=1");
    }

    #[test]
    fn all_tracking_removed_yields_no_query() {
        // 全是 utm_* → query 整体消失
        let r = normalize_url("https://example.com/x?utm_a=1&utm_b=2");
        assert_eq!(r, "https://example.com/x");
    }

    #[test]
    fn port_preserved() {
        let r = normalize_url("HTTPS://Example.COM:8080/Path");
        assert_eq!(r, "https://example.com:8080/Path");
    }

    #[test]
    fn mixed_param_order_with_duplicates_keeps_both() {
        // 同 key 出现两次 — 保留两条，按 key 排序后稳定（按出现顺序追加）
        let r = normalize_url("https://example.com/?b=2&a=1&a=3");
        // 根 / 保留；排序：a=1, a=3, b=2
        assert_eq!(r, "https://example.com/?a=1&a=3&b=2");
    }

    #[test]
    fn param_without_value_kept() {
        let r = normalize_url("https://example.com/?flag&a=1");
        assert_eq!(r, "https://example.com/?a=1&flag");
    }
}
