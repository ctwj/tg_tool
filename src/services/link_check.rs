//! 链接检测编排：URL 归一化、缓存读写、资源分类、批量检测。
//!
//! 模块组成：
//! - 纯函数：`normalize_url` / `url_hash`
//! - 缓存 + 并发检测：`check_urls`
//! - 资源有效性分类：`classify_resources` / `classify_resources_with_statuses`（纯函数，可单测）

use crate::errors::AppError;
use crate::models::extracted_resource::ExtractedResource;
use crate::services::link_checker::{LinkChecker, LinkStatus, LinkVerdict};
use crate::state::{DbPool, OptionCache};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// 单批送 PanCheck 的 URL 分块大小（SC-002：一次检测 ≤60 秒）
const URL_CHUNK_SIZE: usize = 20;

/// URL 归一化：trim → 去 fragment → 小写 scheme+host → 去尾部斜杠。
/// 用于生成稳定的缓存键（跨资源/跨推送去重）。无 scheme 的字符串整体小写。
pub fn normalize_url(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return s;
    }
    if let Some(i) = s.find('#') {
        s.truncate(i);
    }
    if let Some(scheme_end) = s.find("://") {
        let split_at = scheme_end + 3;
        let scheme = &s[..split_at]; // 含 "://"
        let rest = &s[split_at..];
        let host_end = rest.find(['/', '?']).unwrap_or(rest.len());
        let host = &rest[..host_end];
        let tail = &rest[host_end..];
        let mut result = format!("{}{}{}", scheme.to_lowercase(), host.to_lowercase(), tail);
        while result.ends_with('/') {
            result.pop();
        }
        result
    } else {
        s = s.to_lowercase();
        while s.ends_with('/') {
            s.pop();
        }
        s
    }
}

/// 归一化 URL 的 SHA-256 hex（64 字符），作为 `link_check_results.url_hash` 缓存键。
pub fn url_hash(normalized: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 拆分资源 url 字段（逗号分隔）为归一化 URL 列表。
pub fn split_resource_urls(raw: Option<&str>) -> Vec<String> {
    raw.map(|u| {
        u.split(',')
            .map(|s| normalize_url(s.trim()))
            .filter(|s| !s.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

// ─── 缓存读写 ─────────────────────────────────────────────────────────────────

/// 构建 IN 占位符（SQLite `?` / Postgres `$N`）。
fn in_placeholders(db: &DbPool, n: usize) -> String {
    match db {
        DbPool::Sqlite(_) => (0..n).map(|_| "?").collect::<Vec<_>>().join(","),
        DbPool::Postgres(_) => (1..=n)
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// 读取缓存命中：status∈{valid,invalid} 且未过期。返回 normalized→status。
async fn read_cache_hits(
    db: &DbPool,
    hashes: &[String],
    hash_to_norm: &HashMap<String, String>,
) -> Result<HashMap<String, LinkStatus>, AppError> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = in_placeholders(db, hashes.len());
    let sql = format!(
        "SELECT url_hash, status FROM link_check_results \
         WHERE url_hash IN ({placeholders}) AND status IN ('valid','invalid') \
         AND expires_at > CURRENT_TIMESTAMP"
    );
    let rows: Vec<(String, String)> = match db {
        DbPool::Sqlite(pool) => {
            let mut q = sqlx::query_as::<_, (String, String)>(&sql);
            for h in hashes {
                q = q.bind(h);
            }
            q.fetch_all(pool).await?
        }
        DbPool::Postgres(pool) => {
            let mut q = sqlx::query_as::<_, (String, String)>(&sql);
            for h in hashes {
                q = q.bind(h);
            }
            q.fetch_all(pool).await?
        }
    };
    let mut map = HashMap::new();
    for (hash, status) in rows {
        if let Some(norm) = hash_to_norm.get(&hash) {
            let st = match status.as_str() {
                "valid" => LinkStatus::Valid,
                "invalid" => LinkStatus::Invalid,
                _ => continue,
            };
            map.insert(norm.clone(), st);
        }
    }
    Ok(map)
}

/// UPSERT 一条检测结果（仅缓存 valid/invalid；pending/unknown 不缓存以便下次重检）。
async fn persist_verdict(
    db: &DbPool,
    verdict: &LinkVerdict,
    expires_at: chrono::NaiveDateTime,
) -> Result<(), AppError> {
    let hash = url_hash(&verdict.url);
    let norm = verdict.url.clone();
    let status = verdict.status.as_str();
    let platform = verdict.platform.as_deref();
    let fail_reason = verdict.fail_reason.as_deref();
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO link_check_results (url_hash, normalized_url, platform, status, fail_reason, checked_at, expires_at) \
                 VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?) \
                 ON CONFLICT(url_hash) DO UPDATE SET normalized_url=excluded.normalized_url, platform=excluded.platform, status=excluded.status, fail_reason=excluded.fail_reason, checked_at=CURRENT_TIMESTAMP, expires_at=excluded.expires_at",
            )
            .bind(hash).bind(norm).bind(platform).bind(status).bind(fail_reason).bind(expires_at)
            .execute(pool).await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO link_check_results (url_hash, normalized_url, platform, status, fail_reason, checked_at, expires_at) \
                 VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, $6) \
                 ON CONFLICT(url_hash) DO UPDATE SET normalized_url=excluded.normalized_url, platform=excluded.platform, status=excluded.status, fail_reason=excluded.fail_reason, checked_at=CURRENT_TIMESTAMP, expires_at=excluded.expires_at",
            )
            .bind(hash).bind(norm).bind(platform).bind(status).bind(fail_reason).bind(expires_at)
            .execute(pool).await?;
        }
    }
    Ok(())
}

/// 检测一批 URL，返回 normalized→status 映射。
/// - `ignore_cache=true`：强制重检并覆盖缓存。
/// - `pancheck_host` 未配置：未命中缓存的 URL 返回 Unknown（不调 PanCheck，FR-004）。
pub async fn check_urls(
    db: &DbPool,
    option_cache: &OptionCache,
    urls: &[String],
    ignore_cache: bool,
) -> Result<HashMap<String, LinkStatus>, AppError> {
    // 归一化 + 去重
    let mut norm_set: HashSet<String> = HashSet::new();
    for u in urls {
        let n = normalize_url(u);
        if !n.is_empty() {
            norm_set.insert(n);
        }
    }
    if norm_set.is_empty() {
        return Ok(HashMap::new());
    }
    let hash_to_norm: HashMap<String, String> =
        norm_set.iter().map(|n| (url_hash(n), n.clone())).collect();
    let all_hashes: Vec<String> = hash_to_norm.keys().cloned().collect();

    let mut result: HashMap<String, LinkStatus> = HashMap::new();

    // 缓存命中
    if !ignore_cache {
        result = read_cache_hits(db, &all_hashes, &hash_to_norm).await?;
    }

    // 未命中
    let uncached: Vec<String> = norm_set
        .iter()
        .filter(|n| !result.contains_key(*n))
        .cloned()
        .collect();

    if !uncached.is_empty() {
        // 通过工厂解析检测器（按 link_checker_type 选择，无缝切换）
        let checker = crate::services::link_checker::resolve_checker(option_cache).await?;

        if let Some(checker) = checker {
            let concurrency = {
                let cache = option_cache.read().await;
                cache
                    .get("link_check_concurrency")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(5)
                    .clamp(1, 20)
            };
            let ttl_hours = {
                let cache = option_cache.read().await;
                cache
                    .get("link_check_cache_ttl_hours")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(24)
                    .max(1)
            };
            let expires_at = chrono::Utc::now().naive_utc() + chrono::Duration::hours(ttl_hours);

            let chunks: Vec<Vec<String>> = uncached
                .chunks(URL_CHUNK_SIZE)
                .map(|c| c.to_vec())
                .collect();
            tracing::info!(
                "链接检测: uncached={}, chunks={}, concurrency={}",
                uncached.len(),
                chunks.len(),
                concurrency
            );

            let verdicts: Vec<LinkVerdict> = futures::stream::iter(chunks)
                .map(|chunk| {
                    let checker_ref: &dyn LinkChecker = checker.as_ref();
                    async move {
                        match checker_ref.check(&chunk).await {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!("链接检测分块失败: {e}");
                                Vec::new()
                            }
                        }
                    }
                })
                .buffer_unordered(concurrency)
                .flat_map(futures::stream::iter)
                .collect()
                .await;

            for v in &verdicts {
                let n = normalize_url(&v.url);
                result.insert(n.clone(), v.status);
                if matches!(v.status, LinkStatus::Valid | LinkStatus::Invalid)
                    && let Err(e) = persist_verdict(db, v, expires_at).await
                {
                    tracing::warn!("链接检测结果写入缓存失败: {e}");
                }
            }
        } else {
            // 未配置检测器：未命中视为 Unknown（FR-004），不缓存
            for n in &uncached {
                result.insert(n.clone(), LinkStatus::Unknown);
            }
        }
    }

    Ok(result)
}

// ─── 资源分类 ─────────────────────────────────────────────────────────────────

/// 跳过原因类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    ImageNotForwarded,
    LinkInvalid,
    EmptyResource,
    Other,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::ImageNotForwarded => "image_not_forwarded",
            SkipReason::LinkInvalid => "link_invalid",
            SkipReason::EmptyResource => "empty_resource",
            SkipReason::Other => "other",
        }
    }
}

/// 一条被跳过的资源及其原因。
#[derive(Debug, Clone)]
pub struct SkipEntry {
    pub resource: ExtractedResource,
    pub reason: SkipReason,
    pub urls_invalid: Vec<String>,
    pub detail: String,
}

/// 分类结果。
#[derive(Debug, Clone, Default)]
pub struct ClassifyResult {
    pub valid: Vec<ExtractedResource>,
    pub skipped: Vec<SkipEntry>,
}

impl ClassifyResult {
    pub fn skipped_image_count(&self) -> usize {
        self.skipped
            .iter()
            .filter(|s| s.reason == SkipReason::ImageNotForwarded)
            .count()
    }
    pub fn skipped_link_count(&self) -> usize {
        self.skipped
            .iter()
            .filter(|s| s.reason == SkipReason::LinkInvalid)
            .count()
    }
    pub fn skipped_empty_count(&self) -> usize {
        self.skipped
            .iter()
            .filter(|s| s.reason == SkipReason::EmptyResource)
            .count()
    }
    pub fn skipped_other_count(&self) -> usize {
        self.skipped
            .iter()
            .filter(|s| s.reason == SkipReason::Other)
            .count()
    }
}

/// 资源有效性分类（纯函数，可单测）。
/// `statuses`：normalized url → LinkStatus（由 `check_urls` 提供）。
/// 规则（FR-003 5 类跳过）：
/// - **EmptyResource（最优先，feature 041 US2）**：img 与 url 同时为空 → EmptyResource
/// - 图片未转存（img 非空且 img_forward_status != "forwarded"）→ ImageNotForwarded
/// - 任一 URL invalid → LinkInvalid
/// - 否则有效（pending/unknown/valid 均不阻塞）
pub fn classify_resources_with_statuses(
    resources: &[ExtractedResource],
    statuses: &HashMap<String, LinkStatus>,
) -> ClassifyResult {
    let mut result = ClassifyResult::default();
    for r in resources {
        let img = r.img.as_deref().unwrap_or("").trim();
        let urls = split_resource_urls(r.url.as_deref());

        // EmptyResource 优先于其他分支：业务上无图无 URL 的资源无推送价值
        // （修复前：此类资源走完图片分支（img 空跳过）+ URL 分支（urls 空不命中 Invalid）后落入 valid，被推送到目标 API）
        if img.is_empty() && urls.is_empty() {
            result.skipped.push(SkipEntry {
                resource: r.clone(),
                reason: SkipReason::EmptyResource,
                urls_invalid: Vec::new(),
                detail: "无图且无 URL，空资源".to_string(),
            });
            continue;
        }

        if !img.is_empty() && r.img_forward_status.as_deref() != Some("forwarded") {
            result.skipped.push(SkipEntry {
                resource: r.clone(),
                reason: SkipReason::ImageNotForwarded,
                urls_invalid: Vec::new(),
                detail: "封面图尚未转存成功".to_string(),
            });
            continue;
        }
        let invalid: Vec<String> = urls
            .iter()
            .filter(|u| statuses.get(*u) == Some(&LinkStatus::Invalid))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            result.skipped.push(SkipEntry {
                resource: r.clone(),
                reason: SkipReason::LinkInvalid,
                urls_invalid: invalid,
                detail: "网盘链接已失效".to_string(),
            });
            continue;
        }
        result.valid.push(r.clone());
    }
    result
}

/// 资源有效性分类（DB 版）：先汇集全部 URL 调 `check_urls`，再分类。
pub async fn classify_resources(
    db: &DbPool,
    option_cache: &OptionCache,
    resources: &[ExtractedResource],
) -> Result<ClassifyResult, AppError> {
    let mut all_urls: Vec<String> = Vec::new();
    for r in resources {
        // 仅对图片已转存（或无图）的资源收集 URL（图片未转存的资源不做链接检测）
        let img = r.img.as_deref().unwrap_or("").trim();
        if !img.is_empty() && r.img_forward_status.as_deref() != Some("forwarded") {
            continue;
        }
        all_urls.extend(split_resource_urls(r.url.as_deref()));
    }
    let statuses = check_urls(db, option_cache, &all_urls, false).await?;
    Ok(classify_resources_with_statuses(resources, &statuses))
}

/// 跳过链接检测的资源分类：仅做图片未转存过滤，URL 全部视为有效。
/// 用于关闭「推送前链接检测」的推送配置 —— 不调用 LinkChecker，避免重复检测。
pub fn classify_without_link_check(resources: &[ExtractedResource]) -> ClassifyResult {
    classify_resources_with_statuses(resources, &HashMap::new())
}

// ─── US4: 资源级聚合 / 单条检测（不触发推送） ──────────────────────────────────

/// 资源级链接状态聚合（纯函数）：
/// 任一 URL invalid → "invalid"；全部 valid → "valid"；任一 pending 且无 invalid → "pending"；否则 → "unknown"。
pub fn aggregate_link_status(
    resource: &ExtractedResource,
    statuses: &HashMap<String, LinkStatus>,
) -> &'static str {
    let urls = split_resource_urls(resource.url.as_deref());
    if urls.is_empty() {
        return "unknown";
    }
    let mut has_pending = false;
    for u in &urls {
        match statuses.get(u) {
            Some(LinkStatus::Invalid) => return "invalid",
            Some(LinkStatus::Pending) => has_pending = true,
            _ => {}
        }
    }
    if urls
        .iter()
        .all(|u| statuses.get(u) == Some(&LinkStatus::Valid))
    {
        "valid"
    } else if has_pending {
        "pending"
    } else {
        "unknown"
    }
}

/// 仅读缓存（不触发 PanCheck）的链接状态映射 —— 供资源列表展示，避免列页触发外部检测。
pub async fn cached_link_status_map(
    db: &DbPool,
    urls: &[String],
) -> Result<HashMap<String, LinkStatus>, AppError> {
    let norm_set: HashSet<String> = urls
        .iter()
        .map(|s| normalize_url(s.as_str()))
        .filter(|s| !s.is_empty())
        .collect();
    if norm_set.is_empty() {
        return Ok(HashMap::new());
    }
    let hash_to_norm: HashMap<String, String> =
        norm_set.iter().map(|n| (url_hash(n), n.clone())).collect();
    let hashes: Vec<String> = hash_to_norm.keys().cloned().collect();
    read_cache_hits(db, &hashes, &hash_to_norm).await
}

/// 单条资源链接检测（资源列表「检测」按钮，Story4）。返回 (资源级状态, 每条 URL 明细)。
pub async fn check_resource_links(
    db: &DbPool,
    option_cache: &OptionCache,
    resource: &ExtractedResource,
    ignore_cache: bool,
) -> Result<(&'static str, Vec<(String, LinkStatus)>), AppError> {
    let urls = split_resource_urls(resource.url.as_deref());
    let statuses = check_urls(db, option_cache, &urls, ignore_cache).await?;
    let details: Vec<(String, LinkStatus)> = urls
        .iter()
        .map(|u| {
            (
                u.clone(),
                statuses.get(u).copied().unwrap_or(LinkStatus::Unknown),
            )
        })
        .collect();
    Ok((aggregate_link_status(resource, &statuses), details))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_resource(
        id: i64,
        url: Option<&str>,
        img: Option<&str>,
        img_status: Option<&str>,
    ) -> ExtractedResource {
        ExtractedResource {
            id,
            collector_history_id: 1,
            title: format!("res-{id}"),
            url: url.map(str::to_string),
            description: None,
            category: None,
            tags: None,
            img: img.map(str::to_string),
            source: "tg".to_string(),
            extra: None,
            extract_mode: "rule".to_string(),
            is_pushed: false,
            is_edited: false,
            created_at: chrono::DateTime::from_timestamp(1700000000, 0)
                .unwrap()
                .naive_utc(),
            updated_at: chrono::DateTime::from_timestamp(1700000000, 0)
                .unwrap()
                .naive_utc(),
            img_forward_status: img_status.map(str::to_string),
            image_message_id: None,
            file_id: None,
        }
    }

    fn st(map: &[(&str, LinkStatus)]) -> HashMap<String, LinkStatus> {
        map.iter().map(|(k, v)| (normalize_url(k), *v)).collect()
    }

    #[test]
    fn test_normalize_trims_whitespace() {
        assert_eq!(
            normalize_url("  https://pan.quark.cn/s/abc  "),
            "https://pan.quark.cn/s/abc"
        );
    }
    #[test]
    fn test_normalize_strips_fragment() {
        assert_eq!(
            normalize_url("https://pan.quark.cn/s/abc#x"),
            "https://pan.quark.cn/s/abc"
        );
    }
    #[test]
    fn test_normalize_lowercases_scheme_and_host() {
        assert_eq!(
            normalize_url("HTTPS://PAN.Quark.CN/s/abc"),
            "https://pan.quark.cn/s/abc"
        );
    }
    #[test]
    fn test_normalize_strips_trailing_slash() {
        assert_eq!(
            normalize_url("https://pan.quark.cn/s/abc/"),
            "https://pan.quark.cn/s/abc"
        );
    }
    #[test]
    fn test_normalize_preserves_path_case_and_query() {
        assert_eq!(
            normalize_url("https://pan.quark.cn/s/AbC?from=share"),
            "https://pan.quark.cn/s/AbC?from=share"
        );
    }
    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_url("   "), "");
    }
    #[test]
    fn test_url_hash_is_64_hex() {
        let h = url_hash("https://pan.quark.cn/s/abc");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
    #[test]
    fn test_url_hash_stable_and_dedup() {
        let a = url_hash(&normalize_url("  https://PAN.quark.cn/s/abc#x  "));
        let b = url_hash(&normalize_url("https://pan.quark.cn/s/abc"));
        assert_eq!(a, b);
        assert_ne!(a, url_hash("https://pan.baidu.com/s/zzz"));
    }

    // --- 分类逻辑 ---

    #[test]
    fn test_classify_image_not_forwarded() {
        let r = make_resource(
            1,
            Some("https://pan.quark.cn/s/a"),
            Some("img1"),
            Some("pending"),
        );
        let res = classify_resources_with_statuses(&[r], &HashMap::new());
        assert_eq!(res.valid.len(), 0);
        assert_eq!(res.skipped_image_count(), 1);
        assert_eq!(res.skipped_link_count(), 0);
    }

    #[test]
    fn test_classify_image_forwarded_valid() {
        let r = make_resource(
            1,
            Some("https://pan.quark.cn/s/a"),
            Some("img1"),
            Some("forwarded"),
        );
        let res = classify_resources_with_statuses(
            &[r],
            &st(&[("https://pan.quark.cn/s/a", LinkStatus::Valid)]),
        );
        assert_eq!(res.valid.len(), 1);
        assert!(res.skipped.is_empty());
    }

    #[test]
    fn test_classify_link_invalid_any_one() {
        let r = make_resource(
            1,
            Some("https://pan.quark.cn/s/a,https://pan.baidu.com/s/b"),
            None,
            None,
        );
        let statuses = st(&[
            ("https://pan.quark.cn/s/a", LinkStatus::Valid),
            ("https://pan.baidu.com/s/b", LinkStatus::Invalid),
        ]);
        let res = classify_resources_with_statuses(&[r], &statuses);
        assert_eq!(res.valid.len(), 0);
        assert_eq!(res.skipped_link_count(), 1);
        assert_eq!(res.skipped[0].urls_invalid.len(), 1);
    }

    #[test]
    fn test_classify_pending_or_unknown_not_blocked() {
        let r1 = make_resource(1, Some("https://pan.quark.cn/s/a"), None, None);
        let r2 = make_resource(2, Some("https://pan.quark.cn/s/b"), None, None);
        let statuses = st(&[
            ("https://pan.quark.cn/s/a", LinkStatus::Pending),
            ("https://pan.quark.cn/s/b", LinkStatus::Unknown),
        ]);
        let res = classify_resources_with_statuses(&[r1, r2], &statuses);
        assert_eq!(res.valid.len(), 2);
        assert!(res.skipped.is_empty());
    }

    #[test]
    fn test_classify_no_url_no_image_is_empty_resource() {
        // FR-003 (feature 041 US2): img 与 url 同时为空 → EmptyResource 跳过
        // 旧测试 test_classify_no_url_image_ok_is_valid 假设此类资源 valid，已废弃
        let r = make_resource(1, None, None, None);
        let res = classify_resources_with_statuses(&[r], &HashMap::new());
        assert_eq!(res.valid.len(), 0);
        assert_eq!(res.skipped_empty_count(), 1);
    }

    #[test]
    fn test_classify_story1_ac1_mixed() {
        // 3 正常 + 1 图片未转存 + 1 链接失效
        let resources = vec![
            make_resource(1, Some("https://pan.quark.cn/s/a"), None, None),
            make_resource(
                2,
                Some("https://pan.quark.cn/s/b"),
                Some("i1"),
                Some("forwarded"),
            ),
            make_resource(3, Some("https://pan.quark.cn/s/c"), None, None),
            make_resource(
                4,
                Some("https://pan.quark.cn/s/d"),
                Some("i2"),
                Some("pending"),
            ),
            make_resource(5, Some("https://pan.baidu.com/s/z"), None, None),
        ];
        let statuses = st(&[
            ("https://pan.quark.cn/s/a", LinkStatus::Valid),
            ("https://pan.quark.cn/s/b", LinkStatus::Valid),
            ("https://pan.quark.cn/s/c", LinkStatus::Valid),
            ("https://pan.baidu.com/s/z", LinkStatus::Invalid),
        ]);
        let res = classify_resources_with_statuses(&resources, &statuses);
        assert_eq!(res.valid.len(), 3); // r1,r2,r3
        assert_eq!(res.skipped_image_count(), 1); // r4
        assert_eq!(res.skipped_link_count(), 1); // r5
    }

    #[test]
    fn test_split_resource_urls() {
        let urls = split_resource_urls(Some("  https://a.com/s/1 , https://b.com/s/2  "));
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().all(|u| !u.contains(',') && !u.starts_with(' ')));
    }

    #[test]
    fn test_classify_without_link_check_skips_only_image() {
        // 关闭链接检测时：图片未转存的资源仍跳过；其余无论 URL 状态全部 valid
        let resources = vec![
            make_resource(1, Some("https://pan.quark.cn/s/a"), None, None),
            make_resource(
                2,
                Some("https://pan.quark.cn/s/b"),
                Some("i1"),
                Some("forwarded"),
            ),
            make_resource(
                3,
                Some("https://pan.baidu.com/s/z"),
                Some("i2"),
                Some("pending"),
            ),
        ];
        let res = classify_without_link_check(&resources);
        assert_eq!(res.valid.len(), 2); // r1 + r2
        assert_eq!(res.skipped_image_count(), 1); // r3 图片未转存
        assert_eq!(res.skipped_link_count(), 0); // 不做链接检测
    }

    // --- FR-003: EmptyResource 分类（feature 041 US2）---

    #[test]
    fn test_classify_empty_resource() {
        // img 空且 url 空时归入 EmptyResource（业务上无意义的空资源不推送）
        let r = make_resource(1, None, None, None);
        let res = classify_resources_with_statuses(&[r], &HashMap::new());
        assert_eq!(res.valid.len(), 0, "空资源不应进入 valid");
        assert_eq!(res.skipped_empty_count(), 1, "应归入 EmptyResource");
        assert_eq!(res.skipped_image_count(), 0);
        assert_eq!(res.skipped_link_count(), 0);
    }

    #[test]
    fn test_classify_empty_resource_with_empty_string_fields() {
        // img="" 空字符串也视为空（DB 字段可能是 NULL 或空串）
        let mut r = make_resource(1, None, Some(""), None);
        r.url = Some("   ".to_string()); // url 全空白也算空
        let res = classify_resources_with_statuses(&[r], &HashMap::new());
        assert_eq!(res.valid.len(), 0);
        assert_eq!(res.skipped_empty_count(), 1);
    }

    #[test]
    fn test_classify_empty_url_with_image_forwarded_is_valid() {
        // 图片已转存 + 无 URL → 非空资源，应 valid（不归入 EmptyResource）
        let r = make_resource(1, None, Some("img_abc"), Some("forwarded"));
        let res = classify_resources_with_statuses(&[r], &HashMap::new());
        assert_eq!(res.valid.len(), 1, "图片已转存无 URL 是有效资源");
        assert_eq!(res.skipped_empty_count(), 0);
        assert_eq!(res.skipped_image_count(), 0);
    }

    #[test]
    fn test_classify_skipped_counts_5_categories() {
        // 混合 4 类资源：2 图片未转存 + 1 链接失效 + 1 空资源 + 1 有效
        // （"Other" 类别当前 classify 不会自然产生，验证 counter 为 0 即可）
        let r_img1 = make_resource(
            10,
            Some("https://pan.quark.cn/s/a"),
            Some("img1"),
            Some("pending"),
        );
        let r_img2 = make_resource(
            11,
            Some("https://pan.quark.cn/s/b"),
            Some("img2"),
            None, // None != "forwarded" → ImageNotForwarded
        );
        let r_link_invalid = make_resource(
            12,
            Some("https://pan.quark.cn/s/expired"),
            None,
            None,
        );
        let r_empty = make_resource(13, None, None, None);
        let r_valid = make_resource(
            14,
            Some("https://pan.quark.cn/s/ok"),
            None,
            None,
        );
        let statuses = st(&[
            ("https://pan.quark.cn/s/expired", LinkStatus::Invalid),
            (
                "https://pan.quark.cn/s/ok",
                LinkStatus::Valid,
            ),
        ]);
        let res = classify_resources_with_statuses(
            &[r_img1, r_img2, r_link_invalid, r_empty, r_valid],
            &statuses,
        );
        assert_eq!(res.skipped_image_count(), 2, "2 条图片未转存");
        assert_eq!(res.skipped_link_count(), 1, "1 条链接失效");
        assert_eq!(res.skipped_empty_count(), 1, "1 条空资源");
        assert_eq!(res.skipped_other_count(), 0, "Other 当前不产生");
        assert_eq!(res.valid.len(), 1, "1 条有效");
        assert_eq!(
            res.skipped.len(),
            4,
            "skipped 总数 = 2 + 1 + 1 + 0 = 4"
        );
    }

    // --- 资源级聚合 ---

    #[test]
    fn test_aggregate_invalid_if_any() {
        let r = make_resource(
            1,
            Some("https://pan.quark.cn/s/a,https://pan.baidu.com/s/b"),
            None,
            None,
        );
        let st_map = st(&[
            ("https://pan.quark.cn/s/a", LinkStatus::Valid),
            ("https://pan.baidu.com/s/b", LinkStatus::Invalid),
        ]);
        assert_eq!(aggregate_link_status(&r, &st_map), "invalid");
    }

    #[test]
    fn test_aggregate_valid_when_all_valid() {
        let r = make_resource(1, Some("https://pan.quark.cn/s/a"), None, None);
        let st_map = st(&[("https://pan.quark.cn/s/a", LinkStatus::Valid)]);
        assert_eq!(aggregate_link_status(&r, &st_map), "valid");
    }

    #[test]
    fn test_aggregate_pending_distinct_from_unknown() {
        // pending 应返回 "pending"，不再混为 "unknown"
        let r1 = make_resource(1, Some("https://pan.quark.cn/s/a"), None, None);
        assert_eq!(
            aggregate_link_status(
                &r1,
                &st(&[("https://pan.quark.cn/s/a", LinkStatus::Pending)])
            ),
            "pending"
        );
        // 无缓存 → unknown
        assert_eq!(aggregate_link_status(&r1, &HashMap::new()), "unknown");
        // 无 URL → unknown
        let r2 = make_resource(2, None, None, None);
        assert_eq!(aggregate_link_status(&r2, &HashMap::new()), "unknown");
    }
}
