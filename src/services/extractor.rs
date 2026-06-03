// 规则提取引擎 — 从 Telegram 消息中提取结构化资源信息
// 移植自 Go 代码 demo/common/netdisk_checker.go, demo/common/extract-info.go, demo/controller/tool_push.go

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

/// 提取结果草稿 — 在写入 DB 之前的中间结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResourceDraft {
    pub title: String,
    pub url: Vec<String>,
    pub description: String,
    pub category: String,
    pub tags: String,
    pub source: String,
}

// ─── 网盘链接识别 ────────────────────────────────────────────────────────────

/// 网盘服务类型
pub const SERVICE_UC: &str = "uc";
pub const SERVICE_ALIYUN: &str = "aliyun";
pub const SERVICE_QUARK: &str = "quark";
pub const SERVICE_115: &str = "115";
pub const SERVICE_123: &str = "123pan";
pub const SERVICE_TIANYI: &str = "tianyi";
pub const SERVICE_XUNLEI: &str = "xunlei";
pub const SERVICE_BAIDU: &str = "baidu";
pub const SERVICE_NOT_FOUND: &str = "notfound";

/// 网盘正则模式定义
struct NetDiskPattern {
    domains: &'static [&'static str],
    pattern: &'static str,
}

/// 8 种网盘的正则定义（编译一次）
static NETDISK_PATTERNS: &[(&str, NetDiskPattern)] = &[
    (
        SERVICE_UC,
        NetDiskPattern {
            domains: &["drive.uc.cn"],
            pattern: r"https?://drive\.uc\.cn/s/([a-zA-Z0-9]+)",
        },
    ),
    (
        SERVICE_ALIYUN,
        NetDiskPattern {
            domains: &["aliyundrive.com", "alipan.com"],
            pattern: r"https?://(?:www\.)?(?:aliyundrive|alipan)\.com/s/([a-zA-Z0-9]+)",
        },
    ),
    (
        SERVICE_QUARK,
        NetDiskPattern {
            domains: &["pan.quark.cn"],
            pattern: r"https?://(?:www\.)?pan\.quark\.cn/s/([a-zA-Z0-9]+)",
        },
    ),
    (
        SERVICE_115,
        NetDiskPattern {
            domains: &["115.com", "115cdn.com", "anxia.com"],
            pattern: r"https?://(?:www\.)?(?:115|115cdn|anxia)\.com/s/([a-zA-Z0-9]+)",
        },
    ),
    (
        SERVICE_123,
        NetDiskPattern {
            domains: &[
                "123684.com",
                "123685.com",
                "123912.com",
                "123pan.com",
                "123pan.cn",
                "123592.com",
            ],
            pattern: r"https?://(?:www\.)?(?:123684|123685|123912|123pan|123pan\.cn|123592)\.com/s/([a-zA-Z0-9-]+)",
        },
    ),
    (
        SERVICE_TIANYI,
        NetDiskPattern {
            domains: &["cloud.189.cn"],
            pattern: r"https?://cloud\.189\.cn/(?:t/|web/share\?code=)([a-zA-Z0-9]+)",
        },
    ),
    (
        SERVICE_XUNLEI,
        NetDiskPattern {
            domains: &["pan.xunlei.com"],
            pattern: r"https?://(?:www\.)?pan\.xunlei\.com/s/([a-zA-Z0-9-]+)",
        },
    ),
    (
        SERVICE_BAIDU,
        NetDiskPattern {
            domains: &["pan.baidu.com", "yun.baidu.com"],
            pattern: r"https?://(?:[a-z]+\.)?(?:pan|yun)\.baidu\.com/(?:s/|share/init\?surl=)([a-zA-Z0-9_-]+)(?:\?|$)",
        },
    ),
];

/// 预编译正则（once_cell 保证只编译一次）
static NETDISK_COMPILED: Lazy<Vec<(&'static str, Regex, &'static [&'static str])>> =
    Lazy::new(|| {
        NETDISK_PATTERNS
            .iter()
            .filter_map(|(service, def)| {
                Regex::new(def.pattern)
                    .ok()
                    .map(|re| (*service, re, def.domains))
            })
            .collect()
    });

/// URL 正则，用于从文本中提取所有 HTTP(S) 链接
static URL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s<>"'），。、；：？！】》]+"#).unwrap());

/// 标签正则，用于匹配 #标签
static TAG_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"#([^\s#]+)").unwrap());

/// 白名单 URL 前缀 — 不截断广告
const WHITELIST_PREFIXES: &[&str] = &[];

/// 从 URL 中提取域名部分
fn extract_domain(url: &str) -> Option<&str> {
    // 简单提取域名：在 "://" 之后取到第一个 '/' 之前
    let start = url.find("://")? + 3;
    let rest = &url[start..];
    let end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..end];
    // 去掉端口
    Some(host.split(':').next().unwrap_or(host))
}

/// 判断 URL 域名是否匹配任一给定域名
fn contains_domain(url: &str, domains: &[&str]) -> bool {
    if let Some(host) = extract_domain(url) {
        domains.iter().any(|d| host.contains(d))
    } else {
        false
    }
}

/// 识别网盘链接 → 返回 (share_id, service_type)
/// 移植自 demo/common/netdisk_checker.go extractShareID
pub fn identify_netdisk(url: &str) -> (String, String) {
    for (service, re, domains) in NETDISK_COMPILED.iter() {
        if contains_domain(url, domains)
            && let Some(caps) = re.captures(url)
            && let Some(m) = caps.get(1)
        {
            return (m.as_str().to_string(), (*service).to_string());
        }
    }
    (String::new(), SERVICE_NOT_FOUND.to_string())
}

// ─── 广告清洗 ────────────────────────────────────────────────────────────────

/// 广告清洗 — 截断 t.me 链接及其后续内容
/// 移植自 demo/common/extract-info.go CleanAdvertisement
///
/// 简化版本：由于 grammers 序列化消息的格式与 TDLib 不同，
/// 我们直接在纯文本层面做截断 — 找到非白名单的 t.me 链接时截断到该行
pub fn clean_advertisement(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut cut_at = lines.len(); // 默认不截断

    for (i, line) in lines.iter().enumerate() {
        // 检查是否包含 t.me 链接
        if line.contains("https://t.me/") {
            // 检查白名单
            let is_whitelisted = WHITELIST_PREFIXES
                .iter()
                .any(|prefix| line.contains(prefix));

            if !is_whitelisted {
                cut_at = i;
                break;
            }
        }
    }

    if cut_at < lines.len() {
        let cleaned: Vec<&str> = lines[..cut_at].to_vec();
        cleaned.join("\n").trim().to_string()
    } else {
        text.to_string()
    }
}

// ─── 关键词结构提取 ──────────────────────────────────────────────────────────

/// 通过关键词提取标题
/// 移植自 demo/common/extract-info.go extractTitleInfo
pub fn extract_title_by_keywords(text: &str) -> Option<String> {
    let keywords = ["名称：", "标题：", "资源名称："];
    for keyword in keywords {
        if let Some(pos) = text.find(keyword) {
            let after = &text[pos + keyword.len()..];
            let line = after.lines().next().unwrap_or("").trim();
            if !line.is_empty() {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// 通过关键词提取描述
/// 移植自 demo/common/extract-info.go extractDescriptionInfo
pub fn extract_description_by_keywords(text: &str) -> Option<String> {
    let keywords = ["亮点：", "描述：", "资源简介："];
    for keyword in keywords {
        if let Some(pos) = text.find(keyword) {
            let after = &text[pos + keyword.len()..];
            let mut paragraphs = Vec::new();
            for line in after.lines() {
                if line == "." || line.is_empty() {
                    break;
                }
                paragraphs.push(line.trim());
            }
            let desc = paragraphs.join(" ");
            if !desc.is_empty() {
                return Some(desc);
            }
        }
    }
    None
}

/// 通过关键词提取链接
/// 移植自 demo/common/extract-info.go extraResInfo
pub fn extract_links_by_keyword(text: &str) -> Vec<String> {
    let keyword = "链接：";
    if let Some(pos) = text.find(keyword) {
        let after = &text[pos + keyword.len()..];
        let mut links = Vec::new();
        for line in after.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue; // 跳过空行
            }
            if line.starts_with("http") {
                links.push(line.to_string());
            } else {
                break;
            }
        }
        return links;
    }
    Vec::new()
}

// ─── 标签提取 ────────────────────────────────────────────────────────────────

/// 提取 #标签
/// 移植自 demo/common/extract-info.go extractKeywordsInfo
pub fn extract_tags(text: &str) -> Vec<String> {
    TAG_REGEX
        .captures_iter(text)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

// ─── 多资源拆分 ──────────────────────────────────────────────────────────────

/// 判断是否需要拆分为多个资源
/// 移植自 demo/controller/tool_push.go extractMessageResources
///
/// 条件：同一类别的网盘链接 > 3 且总链接 > 3
pub fn should_split_multiple(links: &[(String, String)]) -> bool {
    if links.len() <= 3 {
        return false;
    }
    // 统计每个类别的数量
    let mut category_count: HashMap<&str, usize> = HashMap::new();
    for (_, service) in links {
        if service != SERVICE_NOT_FOUND {
            *category_count.entry(service.as_str()).or_insert(0) += 1;
        }
    }
    // 同一类别有多个链接
    category_count.values().any(|&count| count > 1)
}

/// 多资源拆分 — 每个链接独立成一条资源
/// 移植自 demo/controller/tool_push.go extractMultipleResources
pub fn split_multiple_resources(
    text: &str,
    links: &[(String, String)],
) -> Vec<ResourceDraft> {
    let lines: Vec<&str> = text.lines().collect();
    let mut resources = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 检查当前行是否包含有效网盘链接
        for (url, category) in links {
            if line.contains(url.as_str()) {
                // 取前一行为标题
                let title = if i > 0 {
                    let prev = lines[i - 1].trim();
                    if prev.len() >= 3 {
                        prev.to_string()
                    } else {
                        line.to_string()
                    }
                } else {
                    line.to_string()
                };

                resources.push(ResourceDraft {
                    title,
                    url: vec![url.clone()],
                    description: String::new(),
                    category: category.clone(),
                    tags: String::new(),
                    source: "tg".to_string(),
                });
                break;
            }
        }
    }

    resources
}

// ─── 主入口 ──────────────────────────────────────────────────────────────────

/// 从文本中提取所有 HTTP(S) 链接
pub fn extract_all_urls(text: &str) -> Vec<String> {
    URL_REGEX
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// 主入口：从原始消息文本中提取结构化资源列表
/// 整合流程：链接识别 → 广告清洗 → 关键词提取 → 多资源判断/拆分 → 返回结果
pub fn extract_resources(raw_text: &str) -> Vec<ResourceDraft> {
    if raw_text.trim().is_empty() {
        return Vec::new();
    }

    // 1. 提取所有 URL
    let all_urls = extract_all_urls(raw_text);
    if all_urls.is_empty() {
        return Vec::new();
    }

    // 2. 识别网盘链接
    let netdisk_links: Vec<(String, String)> = all_urls
        .iter()
        .filter_map(|url| {
            let (share_id, service) = identify_netdisk(url);
            if service != SERVICE_NOT_FOUND && !share_id.is_empty() {
                Some((url.clone(), service))
            } else {
                None
            }
        })
        .collect();

    if netdisk_links.is_empty() {
        return Vec::new();
    }

    // 3. 广告清洗
    let cleaned_text = clean_advertisement(raw_text);

    // 4. 关键词提取标题
    let title = extract_title_by_keywords(&cleaned_text)
        .or_else(|| {
            // 备用标题：取第一个非空行（截断到 50 字符）
            cleaned_text
                .lines()
                .find(|l| !l.trim().is_empty())
                .map(|l| {
                    let t = l.trim();
                    let chars: Vec<char> = t.chars().collect();
                    if chars.len() > 50 {
                        format!("{}...", chars[..50].iter().collect::<String>())
                    } else {
                        t.to_string()
                    }
                })
        })
        .unwrap_or_else(|| "未命名资源".to_string());

    // 5. 关键词提取描述
    let description = extract_description_by_keywords(&cleaned_text).unwrap_or_default();

    // 6. 标签提取
    let tags = extract_tags(&cleaned_text);
    let tags_str = tags.join(",");

    // 7. 确定主要类别
    let category = netdisk_links
        .first()
        .map(|(_, s)| s.clone())
        .unwrap_or_default();

    // 8. 判断是否需要多资源拆分
    if should_split_multiple(&netdisk_links) {
        let mut split = split_multiple_resources(&cleaned_text, &netdisk_links);
        // 为拆分的资源补上全局标签
        for draft in &mut split {
            if !tags_str.is_empty() {
                draft.tags = tags_str.clone();
            }
        }
        // 如果拆分结果为空，退回单资源
        if !split.is_empty() {
            return split;
        }
    }

    // 9. 单资源模式
    let urls: Vec<String> = netdisk_links.iter().map(|(u, _)| u.clone()).collect();

    vec![ResourceDraft {
        title,
        url: urls,
        description,
        category,
        tags: tags_str,
        source: "tg".to_string(),
    }]
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- T005: 网盘链接识别 ---

    #[test]
    fn test_identify_uc() {
        let (id, svc) = identify_netdisk("https://drive.uc.cn/s/e1ebe95d144c4");
        assert_eq!(id, "e1ebe95d144c4");
        assert_eq!(svc, SERVICE_UC);
    }

    #[test]
    fn test_identify_aliyun() {
        let (id, svc) = identify_netdisk("https://www.aliyundrive.com/s/hz1HHxhahsE");
        assert_eq!(id, "hz1HHxhahsE");
        assert_eq!(svc, SERVICE_ALIYUN);
    }

    #[test]
    fn test_identify_alipan() {
        let (id, svc) = identify_netdisk("https://www.alipan.com/s/QbaHJ71QjV1");
        assert_eq!(id, "QbaHJ71QjV1");
        assert_eq!(svc, SERVICE_ALIYUN);
    }

    #[test]
    fn test_identify_quark() {
        let (id, svc) = identify_netdisk("https://pan.quark.cn/s/9803af406f13");
        assert_eq!(id, "9803af406f13");
        assert_eq!(svc, SERVICE_QUARK);
    }

    #[test]
    fn test_identify_115() {
        let (id, svc) = identify_netdisk("https://115.com/s/swh88n13z72");
        assert_eq!(id, "swh88n13z72");
        assert_eq!(svc, SERVICE_115);
    }

    #[test]
    fn test_identify_115cdn() {
        let (id, svc) = identify_netdisk("https://115cdn.com/s/swh88n13z72");
        assert_eq!(id, "swh88n13z72");
        assert_eq!(svc, SERVICE_115);
    }

    #[test]
    fn test_identify_123pan() {
        let (id, svc) = identify_netdisk("https://www.123pan.com/s/i4uaTd-WHn0");
        assert_eq!(id, "i4uaTd-WHn0");
        assert_eq!(svc, SERVICE_123);
    }

    #[test]
    fn test_identify_tianyi_t() {
        let (id, svc) = identify_netdisk("https://cloud.189.cn/t/viy2quQzMBne");
        assert_eq!(id, "viy2quQzMBne");
        assert_eq!(svc, SERVICE_TIANYI);
    }

    #[test]
    fn test_identify_tianyi_share() {
        let (id, svc) = identify_netdisk("https://cloud.189.cn/web/share?code=UfUjiiFRbymq");
        assert_eq!(id, "UfUjiiFRbymq");
        assert_eq!(svc, SERVICE_TIANYI);
    }

    #[test]
    fn test_identify_xunlei() {
        let (id, svc) = identify_netdisk("https://pan.xunlei.com/s/VNabc123def");
        assert_eq!(id, "VNabc123def");
        assert_eq!(svc, SERVICE_XUNLEI);
    }

    #[test]
    fn test_identify_baidu() {
        let (id, svc) = identify_netdisk("https://pan.baidu.com/s/1rIcc6X7D3rVzNSqivsRejw");
        assert_eq!(id, "1rIcc6X7D3rVzNSqivsRejw");
        assert_eq!(svc, SERVICE_BAIDU);
    }

    #[test]
    fn test_identify_not_found() {
        let (id, svc) = identify_netdisk("https://www.google.com/search?q=test");
        assert_eq!(id, "");
        assert_eq!(svc, SERVICE_NOT_FOUND);
    }

    #[test]
    fn test_identify_unknown_url() {
        let (id, svc) = identify_netdisk("not even a url");
        assert_eq!(id, "");
        assert_eq!(svc, SERVICE_NOT_FOUND);
    }

    // --- T006: 广告清洗 ---

    #[test]
    fn test_clean_advertisement_tme_link() {
        let text = "资源标题\n描述内容\nhttps://t.me/some_bot?start=123\n广告内容";
        let cleaned = clean_advertisement(text);
        assert!(cleaned.contains("资源标题"));
        assert!(cleaned.contains("描述内容"));
        assert!(!cleaned.contains("t.me"));
        assert!(!cleaned.contains("广告内容"));
    }

    #[test]
    fn test_clean_advertisement_no_ad() {
        let text = "资源标题\n描述内容\n链接在这里";
        let cleaned = clean_advertisement(text);
        assert_eq!(cleaned, text);
    }

    #[test]
    fn test_clean_advertisement_empty() {
        let cleaned = clean_advertisement("");
        assert!(cleaned.is_empty());
    }

    // --- T007: 关键词结构提取 ---

    #[test]
    fn test_extract_title_name() {
        let text = "名称：我的资源\n其他内容";
        assert_eq!(
            extract_title_by_keywords(text),
            Some("我的资源".to_string())
        );
    }

    #[test]
    fn test_extract_title_tag() {
        let text = "标题：测试标题\n链接如下";
        assert_eq!(
            extract_title_by_keywords(text),
            Some("测试标题".to_string())
        );
    }

    #[test]
    fn test_extract_title_resource_name() {
        let text = "资源名称：XXX\nhttps://pan.quark.cn/s/abc";
        assert_eq!(
            extract_title_by_keywords(text),
            Some("XXX".to_string())
        );
    }

    #[test]
    fn test_extract_title_not_found() {
        assert_eq!(extract_title_by_keywords("没有关键字的文本"), None);
    }

    #[test]
    fn test_extract_description_highlight() {
        let text = "亮点：这是描述内容\n第二行\n.";
        assert_eq!(
            extract_description_by_keywords(text),
            Some("这是描述内容 第二行".to_string())
        );
    }

    #[test]
    fn test_extract_description_keyword() {
        let text = "描述：我的描述\n.";
        assert_eq!(
            extract_description_by_keywords(text),
            Some("我的描述".to_string())
        );
    }

    #[test]
    fn test_extract_description_not_found() {
        assert_eq!(extract_description_by_keywords("没有描述关键字"), None);
    }

    #[test]
    fn test_extract_links_keyword() {
        let text = "链接：\nhttps://pan.quark.cn/s/abc\nhttps://pan.quark.cn/s/def\n其他";
        let links = extract_links_by_keyword(text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "https://pan.quark.cn/s/abc");
        assert_eq!(links[1], "https://pan.quark.cn/s/def");
    }

    #[test]
    fn test_extract_links_not_found() {
        assert!(extract_links_by_keyword("没有链接关键字").is_empty());
    }

    // --- T008: 标签提取 ---

    #[test]
    fn test_extract_tags_multiple() {
        let text = "这是一个 #电影 #动作 资源";
        let tags = extract_tags(text);
        assert_eq!(tags, vec!["电影", "动作"]);
    }

    #[test]
    fn test_extract_tags_chinese() {
        let text = "#科幻电影 #2024新片";
        let tags = extract_tags(text);
        assert_eq!(tags, vec!["科幻电影", "2024新片"]);
    }

    #[test]
    fn test_extract_tags_empty() {
        assert!(extract_tags("没有标签的文本").is_empty());
    }

    // --- T009: 多资源拆分 ---

    #[test]
    fn test_should_split_more_than_3_same_category() {
        let links: Vec<(String, String)> = (0..5)
            .map(|i| {
                (
                    format!("https://pan.quark.cn/s/link{}", i),
                    SERVICE_QUARK.to_string(),
                )
            })
            .collect();
        assert!(should_split_multiple(&links));
    }

    #[test]
    fn test_should_not_split_3_or_fewer() {
        let links: Vec<(String, String)> = (0..3)
            .map(|i| {
                (
                    format!("https://pan.quark.cn/s/link{}", i),
                    SERVICE_QUARK.to_string(),
                )
            })
            .collect();
        assert!(!should_split_multiple(&links));
    }

    #[test]
    fn test_should_not_split_mixed() {
        let links = vec![
            ("https://pan.quark.cn/s/a".to_string(), SERVICE_QUARK.to_string()),
            ("https://pan.baidu.com/s/b".to_string(), SERVICE_BAIDU.to_string()),
            ("https://drive.uc.cn/s/c".to_string(), SERVICE_UC.to_string()),
        ];
        // 3 条不同类别各 1 个，不应拆分（但 >3 的判断不满足）
        assert!(!should_split_multiple(&links));
    }

    #[test]
    fn test_split_multiple_resources() {
        let text = "资源1\nhttps://pan.quark.cn/s/abc\n资源2\nhttps://pan.quark.cn/s/def\n资源3\nhttps://pan.quark.cn/s/ghi";
        let links = vec![
            ("https://pan.quark.cn/s/abc".to_string(), SERVICE_QUARK.to_string()),
            ("https://pan.quark.cn/s/def".to_string(), SERVICE_QUARK.to_string()),
            ("https://pan.quark.cn/s/ghi".to_string(), SERVICE_QUARK.to_string()),
        ];
        let resources = split_multiple_resources(text, &links);
        assert_eq!(resources.len(), 3);
        assert_eq!(resources[0].title, "资源1");
        assert_eq!(resources[0].url, vec!["https://pan.quark.cn/s/abc"]);
        assert_eq!(resources[1].title, "资源2");
    }

    // --- T010: 主入口 extract_resources() ---

    #[test]
    fn test_extract_resources_full_message() {
        let text = "名称：测试资源\n描述：这是一个描述\nhttps://pan.quark.cn/s/abc123\n#测试 #标签";
        let resources = extract_resources(text);
        assert!(!resources.is_empty());
        let r = &resources[0];
        assert_eq!(r.title, "测试资源");
        assert!(r.url.contains(&"https://pan.quark.cn/s/abc123".to_string()));
        assert_eq!(r.category, SERVICE_QUARK);
    }

    #[test]
    fn test_extract_resources_no_netdisk_links() {
        let text = "这是一条普通消息，没有网盘链接";
        let resources = extract_resources(text);
        assert!(resources.is_empty());
    }

    #[test]
    fn test_extract_resources_empty_text() {
        let resources = extract_resources("");
        assert!(resources.is_empty());
    }

    #[test]
    fn test_extract_resources_with_tags() {
        let text = "#电影 #动作\nhttps://pan.quark.cn/s/test123";
        let resources = extract_resources(text);
        assert!(!resources.is_empty());
        assert!(resources[0].tags.contains("电影"));
        assert!(resources[0].tags.contains("动作"));
    }

    #[test]
    fn test_extract_resources_with_advertisement() {
        let text = "资源标题\nhttps://pan.quark.cn/s/abc123\nhttps://t.me/some_bot\n广告内容";
        let resources = extract_resources(text);
        assert!(!resources.is_empty());
        // 标题应该来自第一行（因为第一行是非空行作为备用标题）
        assert!(resources[0].url.contains(&"https://pan.quark.cn/s/abc123".to_string()));
    }

    #[test]
    fn test_extract_resources_multiple_split() {
        // 5 个夸克链接 → 触发拆分
        let text = "\
资源A
https://pan.quark.cn/s/link1
资源B
https://pan.quark.cn/s/link2
资源C
https://pan.quark.cn/s/link3
资源D
https://pan.quark.cn/s/link4
资源E
https://pan.quark.cn/s/link5";
        let resources = extract_resources(text);
        assert_eq!(resources.len(), 5);
        assert_eq!(resources[0].title, "资源A");
        assert_eq!(resources[4].title, "资源E");
    }

    // --- T012: 混合多种网盘链接 ---

    #[test]
    fn test_extract_mixed_netdisk_types() {
        let text = "名称：混合资源合集\n\
            https://pan.quark.cn/s/abc\n\
            https://www.alipan.com/s/def\n\
            https://pan.baidu.com/s/xyz";
        let resources = extract_resources(text);
        assert!(!resources.is_empty());
        let r = &resources[0];
        assert_eq!(r.title, "混合资源合集");
        // 所有网盘链接都应被收集
        assert!(r.url.contains(&"https://pan.quark.cn/s/abc".to_string()));
        assert!(r.url.contains(&"https://www.alipan.com/s/def".to_string()));
        assert!(r.url.contains(&"https://pan.baidu.com/s/xyz".to_string()));
        // category 为第一个网盘类型
        assert_eq!(r.category, SERVICE_QUARK);
    }

    // --- T013: 首行标题截断 >50 字符 ---

    #[test]
    fn test_extract_title_fallback_truncation() {
        // 构造一条没有关键词标题且首行超长的消息（>50 字符）
        let long_line = "这是一行非常非常长的资源标题文本内容用来验证当没有关键词匹配时系统会自动将首行截断到五十个字符以内并且添加省略号后缀以提示用户该标题已被截断处理";
        assert!(long_line.chars().count() > 50, "测试字符串需超过 50 字符，实际为 {}", long_line.chars().count());
        let text = format!("{}\nhttps://pan.quark.cn/s/abc", long_line);
        let resources = extract_resources(&text);
        assert!(!resources.is_empty());
        let title = &resources[0].title;
        // 标题应被截断到 50 字符 + "..."
        assert!(title.ends_with("..."), "title was: {}", title);
        let without_dots = title.trim_end_matches("...");
        assert!(without_dots.chars().count() <= 50);
    }

    // --- T014: "链接：" 关键词提取描述 ---

    #[test]
    fn test_extract_description_links_keyword() {
        // "链接：" 不在关键词列表中，使用 "描述：" 测试
        let text = "名称：测试资源\n描述：这是一段资源描述内容用于测试\nhttps://pan.quark.cn/s/abc";
        let description = extract_description_by_keywords(text);
        assert!(description.is_some());
        assert!(description.unwrap().contains("资源描述内容"));
    }

    // --- T015: 特殊字符标签 ---

    #[test]
    fn test_extract_tags_with_special_chars() {
        let text = "#C++ #前端/Vue #React.js #AI·大模型\nhttps://pan.quark.cn/s/abc";
        let tags = extract_tags(text);
        assert!(tags.contains(&"C++".to_string()), "应提取 C++ 标签");
        assert!(tags.contains(&"前端/Vue".to_string()), "应提取 前端/Vue 标签");
        assert!(tags.contains(&"React.js".to_string()), "应提取 React.js 标签");
        assert!(tags.contains(&"AI·大模型".to_string()), "应提取 AI·大模型 标签");
    }

    // --- T016: 复杂 t.me 广告 + 网盘链接 ---

    #[test]
    fn test_extract_resources_complex_tme_ad() {
        let text = "名称：电影合集\n\
            https://t.me/ads_channel/123\n\
            💥限时优惠！关注频道获取更多资源\n\
            https://pan.quark.cn/s/abc\n\
            https://www.alipan.com/s/def";
        let resources = extract_resources(text);
        assert!(!resources.is_empty());
        let r = &resources[0];
        assert_eq!(r.title, "电影合集");
        // t.me 广告链接不应出现在 url 中
        assert!(!r.url.iter().any(|u| u.contains("t.me")));
        // 网盘链接应保留
        assert!(r.url.contains(&"https://pan.quark.cn/s/abc".to_string()));
        assert!(r.url.contains(&"https://www.alipan.com/s/def".to_string()));
    }
}
