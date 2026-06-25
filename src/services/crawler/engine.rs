//! 单任务抓取引擎（research.md R3+R4+R5+R6）
//!
//! 入口：
//! - [`run_task`]: 立即运行一次，抓列表页 → 详情页 → 落库
//! - [`test_run`]: 测试运行，不落库，返回前 N 条详情结构化预览

use std::time::Duration;

use crate::models::crawler_article_image::NewCrawlerArticleImage;
use crate::models::crawler_article_link::NewCrawlerArticleLink;
use crate::services::crawler::block_detector::detect_block;
use crate::services::crawler::extractor::{
    extract_detail_links, extract_fields, ExtractedFields, FieldSelectors,
};
use crate::services::crawler::pan_detector::{detect_platform, find_extract_code, is_direct_link};
use crate::services::crawler::url_normalize::normalize_url;
use crate::state::{AppState, DbPool};

/// 单次运行结果摘要（用于日志/历史/响应）
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub task_id: i64,
    pub task_name: String,
    pub status: &'static str, // "success" / "partial" / "failed" / "blocked"
    pub block_type: Option<String>,
    pub crawled_count: i64,
    pub new_count: i64,
    pub skipped_count: i64,
    pub failed_count: i64,
    pub error_message: Option<String>,
}

/// 测试运行预览（contracts/crawler-api.md §CrawlerTestPreview）
#[derive(Debug, Clone, serde::Serialize)]
pub struct CrawlerTestPreview {
    pub list_count: i64,
    pub preview_count: i64,
    pub articles: Vec<TestPreviewArticle>,
    pub selector_validation: SelectorValidation,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TestPreviewArticle {
    pub source_url: String,
    pub title: Option<String>,
    pub content_snippet: Option<String>,
    pub pan_links: Vec<TestPanLink>,
    pub direct_links: Vec<String>,
    pub images: Vec<String>,
    pub field_warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TestPanLink {
    pub platform: String,
    pub url: String,
    pub extract_code: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SelectorValidation {
    pub list_item_ok: bool,
    pub detail_link_ok: bool,
    pub missing_fields: Vec<String>,
}

/// 加载任务完整定义（包括解析 selectors JSON）
pub async fn load_task(
    db: &DbPool,
    task_id: i64,
) -> Result<Option<TaskRuntime>, String> {
    let row = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, TaskRow>(
                "SELECT id, name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, \
                 proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, \
                 status, consecutive_failures \
                 FROM crawler_tasks WHERE id = ?",
            )
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, TaskRow>(
                "SELECT id, name, enabled, list_urls, selectors, two_stage, \
                 interval_minutes, task_concurrency, user_agent, request_delay_ms, \
                 proxy, auto_link_check, block_detection_config, max_consecutive_failures, \
                 template_source, pagination_selector, max_pages, \
                 status, consecutive_failures \
                 FROM crawler_tasks WHERE id = $1",
            )
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
        }
    };
    let Some(row) = row else {
        return Ok(None);
    };

    let list_urls: Vec<String> = serde_json::from_str(&row.list_urls).unwrap_or_default();
    let selectors: FieldSelectors =
        serde_json::from_str(&row.selectors).unwrap_or_default();

    Ok(Some(TaskRuntime {
        row,
        list_urls,
        selectors,
    }))
}

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
struct TaskRow {
    id: i64,
    name: String,
    enabled: bool,
    list_urls: String,
    selectors: String,
    two_stage: bool,
    interval_minutes: i64,
    task_concurrency: i64,
    user_agent: Option<String>,
    request_delay_ms: i64,
    proxy: Option<String>,
    auto_link_check: bool,
    block_detection_config: Option<String>,
    max_consecutive_failures: i64,
    template_source: Option<String>,
    pagination_selector: Option<String>,
    max_pages: i64,
    status: String,
    consecutive_failures: i64,
}

pub struct TaskRuntime {
    row: TaskRow,
    pub list_urls: Vec<String>,
    pub selectors: FieldSelectors,
}

impl TaskRuntime {
    pub fn id(&self) -> i64 {
        self.row.id
    }
    pub fn name(&self) -> &str {
        &self.row.name
    }
    pub fn proxy(&self) -> Option<&str> {
        self.row.proxy.as_deref()
    }
    pub fn user_agent(&self) -> Option<&str> {
        self.row.user_agent.as_deref()
    }
    pub fn request_delay_ms(&self) -> u64 {
        self.row.request_delay_ms.max(0) as u64
    }
    pub fn max_consecutive_failures(&self) -> i64 {
        self.row.max_consecutive_failures
    }
    pub fn auto_link_check(&self) -> bool {
        self.row.auto_link_check
    }
    pub fn pagination_selector(&self) -> Option<&str> {
        self.row.pagination_selector.as_deref()
    }
    pub fn max_pages(&self) -> i64 {
        self.row.max_pages
    }
}

/// 立即运行一次任务（落库）
pub async fn run_task(task_id: i64, state: &AppState) -> Result<RunSummary, String> {
    let task = load_task(&state.db, task_id)
        .await?
        .ok_or_else(|| format!("爬虫任务 {task_id} 不存在"))?;

    let task_name = task.name().to_string();
    let started_at = chrono::Utc::now().naive_utc();

    let mut summary = RunSummary {
        task_id,
        task_name: task_name.clone(),
        status: "success",
        block_type: None,
        crawled_count: 0,
        new_count: 0,
        skipped_count: 0,
        failed_count: 0,
        error_message: None,
    };

    // 抓所有列表页（若配置了 next_page_selector，每个 seed URL 会自动翻页）
    let list_result = crawl_list_pages(&task, state, &mut summary).await;
    if let Some(block_msg) = list_result.blocked {
        summary.status = "blocked";
        summary.error_message = Some(block_msg);
        finalize_run(state, &task, started_at, &summary).await;
        return Ok(summary);
    }
    let all_detail_links = list_result.detail_links;

    if all_detail_links.is_empty() {
        summary.status = if summary.failed_count > 0 { "failed" } else { "success" };
        finalize_run(state, &task, started_at, &summary).await;
        return Ok(summary);
    }

    // 抓详情页并落库
    for detail_url in &all_detail_links {
        let abs = resolve_url(detail_url, &task.list_urls.first().cloned().unwrap_or_default());
        let normalized = normalize_url(&abs);
        match fetch_url(&abs, task.user_agent(), task.proxy(), state).await {
            Ok((status_code, body, headers)) => {
                if let Some(block) = detect_block(status_code, &body, &headers) {
                    summary.status = "blocked";
                    summary.block_type = Some(block.as_str());
                    summary.error_message =
                        Some(format!("详情页 {abs} 被拦截: {block}"));
                    break;
                }
                let fields = extract_fields(&body, &task.selectors);
                match upsert_article_and_children(&state.db, task.id(), task.name(), &abs, &normalized, &fields).await {
                    Ok(true) => summary.new_count += 1,
                    Ok(false) => summary.skipped_count += 1,
                    Err(e) => {
                        tracing::warn!("落库失败 {abs}: {e}");
                        summary.failed_count += 1;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("详情页抓取失败 {abs}: {e}");
                summary.failed_count += 1;
            }
        }
        sleep_request_delay(task.request_delay_ms()).await;
    }

    // 状态综合判定
    if summary.block_type.is_some() {
        summary.status = "blocked";
    } else if summary.new_count == 0 && summary.failed_count > 0 && summary.skipped_count == 0 {
        summary.status = "failed";
    } else if summary.failed_count > 0 {
        summary.status = "partial";
    } else {
        summary.status = "success";
    }

    finalize_run(state, &task, started_at, &summary).await;
    Ok(summary)
}

/// 测试运行（不落库）
pub async fn test_run(
    db: &DbPool,
    task_id: i64,
    limit: usize,
) -> Result<CrawlerTestPreview, String> {
    let task = load_task(db, task_id)
        .await?
        .ok_or_else(|| format!("爬虫任务 {task_id} 不存在"))?;

    let mut preview = CrawlerTestPreview {
        list_count: 0,
        preview_count: 0,
        articles: Vec::new(),
        selector_validation: SelectorValidation {
            list_item_ok: !task.selectors.list_item.is_empty(),
            detail_link_ok: !task.selectors.detail_link.is_empty(),
            missing_fields: Vec::new(),
        },
    };

    // 仅读首列表页
    let list_url = match task.list_urls.first() {
        Some(u) => u.clone(),
        None => return Ok(preview),
    };
    // 测试运行：直接 reqwest，不依赖 AppState 配置（保持纯函数语义）
    let body = match fetch_body_simple(&list_url, task.user_agent(), task.proxy()).await {
        Ok(b) => b,
        Err(e) => {
            return Err(format!("列表页抓取失败: {e}"));
        }
    };
    let links = extract_detail_links(
        &body,
        &task.selectors.list_item,
        &task.selectors.detail_link,
        task.selectors.detail_link_attr.as_deref(),
    );
    preview.list_count = links.len() as i64;

    // 取前 N 个详情
    let take = links.into_iter().take(limit);
    for detail_url in take {
        let abs = resolve_url(&detail_url, &list_url);
        let body = match fetch_body_simple(&abs, task.user_agent(), task.proxy()).await {
            Ok(b) => b,
            Err(_) => continue,
        };
        let fields = extract_fields(&body, &task.selectors);
        preview.articles.push(build_preview_article(&abs, &fields));
        preview.preview_count += 1;
    }

    // 选择器命中校验
    let all_missing = preview
        .articles
        .iter()
        .flat_map(|a| a.field_warnings.iter().cloned())
        .collect::<std::collections::HashSet<_>>();
    preview.selector_validation.missing_fields = all_missing.into_iter().collect();
    Ok(preview)
}

fn build_preview_article(
    url: &str,
    fields: &ExtractedFields,
) -> TestPreviewArticle {
    let pan_links: Vec<TestPanLink> = fields
        .pan_links
        .iter()
        .filter_map(|u| {
            detect_platform(u).map(|p| TestPanLink {
                platform: p.to_string(),
                url: u.clone(),
                extract_code: find_extract_code(u),
            })
        })
        .collect();
    let direct_links: Vec<String> = fields
        .direct_links
        .iter()
        .filter(|u| is_direct_link(u))
        .cloned()
        .collect();

    let content_snippet = fields.content.as_ref().map(|c| {
        let plain = strip_html_tags(c);
        if plain.len() > 200 {
            format!("{}…", &plain[..200])
        } else {
            plain
        }
    });

    TestPreviewArticle {
        source_url: url.to_string(),
        title: fields.title.clone(),
        content_snippet,
        pan_links,
        direct_links,
        images: fields.images.clone(),
        field_warnings: fields.field_warnings.clone(),
    }
}

fn strip_html_tags(html: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    let no_tags = TAG_RE.replace_all(html, "");
    no_tags.replace("&nbsp;", " ").trim().to_string()
}

async fn sleep_request_delay(ms: u64) {
    if ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

/// `crawl_list_pages` 的返回
pub struct ListPageResult {
    /// 累计的详情链接（已按 list_item/detail_link 选择器抽取）
    pub detail_links: Vec<String>,
    /// 若中途被拦截，含可读错误信息（调用方应直接返回）
    pub blocked: Option<String>,
}

/// 抓所有列表页 — 若任务配置了 `pagination_selector` 且非单阶段模式，
/// 每个抓到的列表页都会扫描分页容器内的所有 `<a href>`，去重后批量扩散：
/// - 把所有未访问的 URL 加入队列
/// - visited 集合防止死循环
/// - 达到 `max_pages` 上限停止（0 = 不限；含种子 URL）
///
/// 与"只找下一页"相比，分页选择器更通用：能一次匹配 `1 2 3 ... 末页` 所有页码，
/// 也能兼容只有 next/prev 链接的站点。
pub async fn crawl_list_pages(
    task: &TaskRuntime,
    state: &AppState,
    summary: &mut RunSummary,
) -> ListPageResult {
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<String> = task.list_urls.iter().cloned().collect();
    let mut all_detail_links: Vec<String> = Vec::new();
    let pagination_sel = task.pagination_selector().filter(|s| !s.is_empty());
    let max_pages = task.max_pages();
    // 自动翻页：仅当配置了 pagination_selector 时启用
    let pagination_enabled = pagination_sel.is_some();

    while let Some(url) = queue.pop_front() {
        if !visited.insert(url.clone()) {
            continue; // 已访问过，跳过
        }
        // max_pages 含种子页：当 visited 数量已达上限，停止扩散新链接
        if max_pages > 0 && visited.len() as i64 > max_pages {
            // 已抓够，跳过本次（仍消费 queue 中已加入的，但不再扩散）
            // 简化：直接 break 避免继续抓取
            break;
        }
        match fetch_url(&url, task.user_agent(), task.proxy(), state).await {
            Ok((status_code, body, headers)) => {
                if let Some(block) = detect_block(status_code, &body, &headers) {
                    summary.block_type = Some(block.as_str());
                    return ListPageResult {
                        detail_links: all_detail_links,
                        blocked: Some(format!("列表页 {url} 被拦截: {block}")),
                    };
                }
                let links = extract_detail_links(
                    &body,
                    &task.selectors.list_item,
                    &task.selectors.detail_link,
                    task.selectors.detail_link_attr.as_deref(),
                );
                summary.crawled_count += links.len() as i64;
                all_detail_links.extend(links);

                // 分页扩散：扫描页面所有分页链接，加入队列（去重）
                if pagination_enabled
                    && let Some(sel) = pagination_sel
                {
                    let page_links = extract_pagination_urls(&body, sel);
                    let mut new_added = 0;
                    for href in page_links {
                        let abs = resolve_url(&href, &url);
                        if !visited.contains(&abs) {
                            let total = (visited.len() + new_added) as i64;
                            if max_pages == 0 || total < max_pages {
                                queue.push_back(abs);
                                new_added += 1;
                            }
                        }
                    }
                    if new_added > 0 {
                        tracing::info!(
                            task = task.name(),
                            from = %url,
                            new_pages = new_added,
                            "crawler: pagination selector added new pages"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!("列表页抓取失败 {url}: {e}");
                summary.failed_count += 1;
                summary.error_message = Some(format!("列表页抓取失败: {e}"));
            }
        }
        sleep_request_delay(task.request_delay_ms()).await;
    }

    ListPageResult {
        detail_links: all_detail_links,
        blocked: None,
    }
}

/// 从列表页 HTML 中按 CSS 选择器找所有分页链接（去重前）
///
/// - 选择器命中的元素本身若是 `<a>`，直接取 href
/// - 否则向后扫描后代 `<a>`，取 href
/// - 自动跳过空 href
/// - 返回顺序按 DOM 出现顺序，调用方负责去重
pub fn extract_pagination_urls(html: &str, selector: &str) -> Vec<String> {
    use scraper::{Html, Selector};
    let mut out = Vec::new();
    let document = Html::parse_document(html);
    let Ok(sel) = Selector::parse(selector) else {
        return out;
    };
    for element in document.select(&sel) {
        // 元素本身 + 所有后代节点，统一扫描
        for node in element.descendants() {
            if let Some(elem) = node.value().as_element()
                && let Some(href) = elem.attr("href")
                && !href.is_empty()
            {
                out.push(href.to_string());
            }
        }
    }
    out
}

/// 解析相对 URL → 绝对 URL（基于 base）
fn resolve_url(href: &str, base: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    // 纯 query 相对路径：`?page=2` → 替换 base 的 query（保留 path）
    if let Some(rest) = href.strip_prefix('?') {
        let path_end = base.find('?').unwrap_or(base.len());
        return format!("{}?{rest}", &base[..path_end]);
    }
    if let Some(base_scheme_end) = base.find("://") {
        let after_scheme = &base[base_scheme_end + 3..];
        if let Some(slash) = after_scheme.find('/') {
            let origin = &base[..base_scheme_end + 3 + slash];
            if let Some(stripped) = href.strip_prefix('/') {
                return format!("{origin}/{stripped}");
            }
            // 相对路径：去掉最后一段
            let base_path = &after_scheme[slash..];
            if let Some(last_slash) = base_path.rfind('/') {
                let dir = &base_path[..last_slash + 1];
                let origin_full = &base[..base_scheme_end + 3 + slash];
                return format!("{origin_full}{dir}{href}");
            }
            return format!("{origin}/{href}");
        }
        // base 没有路径
        return format!("{base}/{href}");
    }
    href.to_string()
}

/// 简单 GET（测试运行用）— 不走 AppState 的代理选项
async fn fetch_body_simple(
    url: &str,
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<String, String> {
    let client = build_reqwest_client(user_agent, proxy).map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok(text)
}

/// 通过 AppState 抓 URL — 优先任务级代理，回退系统 http_proxy_url
async fn fetch_url(
    url: &str,
    user_agent: Option<&str>,
    task_proxy: Option<&str>,
    state: &AppState,
) -> Result<(u16, String, Vec<(String, String)>), String> {
    let sys_proxy = state.http_proxy_url().await;
    let proxy = task_proxy.or(sys_proxy.as_deref());

    let client = build_reqwest_client(user_agent, proxy).map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status().as_u16();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok((status, text, headers))
}

pub fn build_reqwest_client_pub(
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<reqwest::Client, reqwest::Error> {
    build_reqwest_client(user_agent, proxy)
}

fn build_reqwest_client(
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<reqwest::Client, reqwest::Error> {
    let ua = user_agent
        .unwrap_or(crate::services::crawler::templates::DEFAULT_USER_AGENT);
    let mut builder = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(30));
    if let Some(p) = proxy
        && !p.is_empty()
        && let Ok(proxy) = reqwest::Proxy::all(p)
    {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

/// upsert 文章 + 子表（links/images）
///
/// 返回 `true` 表示新增，`false` 表示已存在（skipped）
async fn upsert_article_and_children(
    db: &DbPool,
    task_id: i64,
    task_name: &str,
    source_url: &str,
    canonical: &str,
    fields: &ExtractedFields,
) -> Result<bool, String> {
    let now = chrono::Utc::now().naive_utc();

    // upsert 文章主表（同 (task_id, source_url_canonical) 幂等）
    let (article_id, is_new): (i64, bool) = match db {
        DbPool::Sqlite(pool) => {
            // 先查
            let exist: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM crawler_articles WHERE task_id = ? AND source_url_canonical = ?",
            )
            .bind(task_id)
            .bind(canonical)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;

            if let Some((id,)) = exist {
                sqlx::query(
                    "UPDATE crawler_articles SET title = ?, content = ?, category = ?, \
                     tags = ?, crawled_at = ?, updated_at = ?, source_url = ?, source_type = ? \
                     WHERE id = ?",
                )
                .bind(&fields.title)
                .bind(&fields.content)
                .bind(&fields.category)
                .bind(&fields.tags)
                .bind(now)
                .bind(now)
                .bind(source_url)
                .bind(task_name)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
                (id, false)
            } else {
                let result = sqlx::query(
                    "INSERT INTO crawler_articles \
                     (task_id, source_type, source_url, source_url_canonical, title, content, \
                     category, tags, is_edited, crawled_at, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
                )
                .bind(task_id)
                .bind(task_name)
                .bind(source_url)
                .bind(canonical)
                .bind(&fields.title)
                .bind(&fields.content)
                .bind(&fields.category)
                .bind(&fields.tags)
                .bind(now)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
                (result.last_insert_rowid(), true)
            }
        }
        DbPool::Postgres(pool) => {
            let exist: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM crawler_articles WHERE task_id = $1 AND source_url_canonical = $2",
            )
            .bind(task_id)
            .bind(canonical)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
            if let Some((id,)) = exist {
                sqlx::query(
                    "UPDATE crawler_articles SET title = $1, content = $2, category = $3, \
                     tags = $4, crawled_at = $5, updated_at = $6, source_url = $7, source_type = $8 \
                     WHERE id = $9",
                )
                .bind(&fields.title)
                .bind(&fields.content)
                .bind(&fields.category)
                .bind(&fields.tags)
                .bind(now)
                .bind(now)
                .bind(source_url)
                .bind(task_name)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
                (id, false)
            } else {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO crawler_articles \
                     (task_id, source_type, source_url, source_url_canonical, title, content, \
                     category, tags, is_edited, crawled_at, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE, $9, $9, $9) RETURNING id",
                )
                .bind(task_id)
                .bind(task_name)
                .bind(source_url)
                .bind(canonical)
                .bind(&fields.title)
                .bind(&fields.content)
                .bind(&fields.category)
                .bind(&fields.tags)
                .bind(now)
                .fetch_one(pool)
                .await
                .map_err(|e| e.to_string())?;
                (id, true)
            }
        }
    };

    // 子表：仅新增文章时写入子表（避免每次更新重复插入）
    // 实际上为了避免老链接漏抓，我们也应在已有文章上增量补 — 但 v1 简化为仅新文章
    if is_new {
        // 链接：从 pan_links 区域 + direct_links 区域联合分析
        let mut links_to_insert: Vec<NewCrawlerArticleLink> = Vec::new();
        for url in &fields.pan_links {
            let canonical_link = normalize_url(url);
            if let Some(platform) = detect_platform(url) {
                let code = find_extract_code(url);
                links_to_insert.push(NewCrawlerArticleLink {
                    article_id,
                    link_type: "pan".into(),
                    platform: Some(platform.to_string()),
                    url: url.clone(),
                    url_canonical: canonical_link,
                    extract_code: code,
                });
            } else if is_direct_link(url) {
                links_to_insert.push(NewCrawlerArticleLink {
                    article_id,
                    link_type: "direct".into(),
                    platform: None,
                    url: url.clone(),
                    url_canonical: canonical_link,
                    extract_code: None,
                });
            }
        }
        for url in &fields.direct_links {
            let canonical_link = normalize_url(url);
            if detect_platform(url).is_some() {
                // direct_links 区域里的网盘链接也归入 pan
                let code = find_extract_code(url);
                links_to_insert.push(NewCrawlerArticleLink {
                    article_id,
                    link_type: "pan".into(),
                    platform: Some(detect_platform(url).unwrap().to_string()),
                    url: url.clone(),
                    url_canonical: canonical_link,
                    extract_code: code,
                });
            } else if is_direct_link(url) {
                links_to_insert.push(NewCrawlerArticleLink {
                    article_id,
                    link_type: "direct".into(),
                    platform: None,
                    url: url.clone(),
                    url_canonical: canonical_link,
                    extract_code: None,
                });
            }
        }
        // 去重（按 url_canonical + link_type）
        let mut seen = std::collections::HashSet::new();
        for l in links_to_insert {
            let key = (l.url_canonical.clone(), l.link_type.clone());
            if !seen.insert(key) {
                continue;
            }
            insert_link(db, &l).await.map_err(|e| e.to_string())?;
        }

        // 图片
        for url in &fields.images {
            let canonical_img = normalize_url(url);
            let new_img = NewCrawlerArticleImage {
                article_id,
                original_url: url.clone(),
                url_canonical: canonical_img,
            };
            insert_image(db, &new_img).await.map_err(|e| e.to_string())?;
        }
    }

    Ok(is_new)
}

async fn insert_link(
    db: &DbPool,
    l: &NewCrawlerArticleLink,
) -> Result<(), sqlx::Error> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO crawler_article_links \
                 (article_id, link_type, platform, url, url_canonical, extract_code, \
                 validity_status, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, 'unknown', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(l.article_id)
            .bind(&l.link_type)
            .bind(&l.platform)
            .bind(&l.url)
            .bind(&l.url_canonical)
            .bind(&l.extract_code)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO crawler_article_links \
                 (article_id, link_type, platform, url, url_canonical, extract_code, \
                 validity_status, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, 'unknown', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(l.article_id)
            .bind(&l.link_type)
            .bind(&l.platform)
            .bind(&l.url)
            .bind(&l.url_canonical)
            .bind(&l.extract_code)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

async fn insert_image(
    db: &DbPool,
    img: &NewCrawlerArticleImage,
) -> Result<(), sqlx::Error> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO crawler_article_images \
                 (article_id, original_url, url_canonical, status, retry_count, \
                 created_at, updated_at) \
                 VALUES (?, ?, ?, 'pending', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(img.article_id)
            .bind(&img.original_url)
            .bind(&img.url_canonical)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO crawler_article_images \
                 (article_id, original_url, url_canonical, status, retry_count, \
                 created_at, updated_at) \
                 VALUES ($1, $2, $3, 'pending', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(img.article_id)
            .bind(&img.original_url)
            .bind(&img.url_canonical)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 写历史记录 + 更新任务的 last_run_at / next_run_at / consecutive_failures / status
///
/// 在抓取主循环之后调用一次。
async fn finalize_run(state: &AppState, task: &TaskRuntime, started_at: chrono::NaiveDateTime, summary: &RunSummary) {
    let now = chrono::Utc::now().naive_utc();
    let duration_ms = (now - started_at).num_milliseconds().max(0);
    let task_name = task.name().to_string();
    let interval_mins = task.row.interval_minutes.max(1);
    let next_run = now + chrono::Duration::minutes(interval_mins);

    let is_blocked_or_failed = matches!(summary.status, "blocked" | "failed");

    // 写历史
    if let Err(e) = insert_history(
        &state.db,
        task.id(),
        &task_name,
        started_at,
        now,
        duration_ms,
        summary,
    )
    .await
    {
        tracing::warn!("写 crawler_run_history 失败: {e}");
    }

    // 更新任务计数 / 状态
    let new_status = if is_blocked_or_failed
        && task.row.consecutive_failures + 1 >= task.max_consecutive_failures()
    {
        "auto_blocked"
    } else {
        &task.row.status
    };

    match &state.db {
        DbPool::Sqlite(pool) => {
            if let Err(e) = sqlx::query(
                "UPDATE crawler_tasks SET \
                 last_run_at = ?, next_run_at = ?, \
                 consecutive_failures = CASE WHEN ? THEN consecutive_failures + 1 ELSE 0 END, \
                 status = ?, updated_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(next_run)
            .bind(is_blocked_or_failed)
            .bind(new_status)
            .bind(now)
            .bind(task.id())
            .execute(pool)
            .await
            {
                tracing::warn!("更新 crawler_tasks 失败: {e}");
            }
        }
        DbPool::Postgres(pool) => {
            if let Err(e) = sqlx::query(
                "UPDATE crawler_tasks SET \
                 last_run_at = $1, next_run_at = $2, \
                 consecutive_failures = CASE WHEN $3 THEN consecutive_failures + 1 ELSE 0 END, \
                 status = $4, updated_at = $5 WHERE id = $6",
            )
            .bind(now)
            .bind(next_run)
            .bind(is_blocked_or_failed)
            .bind(new_status)
            .bind(now)
            .bind(task.id())
            .execute(pool)
            .await
            {
                tracing::warn!("更新 crawler_tasks 失败: {e}");
            }
        }
    }
    if new_status == "auto_blocked" {
        tracing::warn!(
            "任务 {} ({}) 连续失败达阈值 {}，自动停用",
            task.id(),
            task.name(),
            task.max_consecutive_failures()
        );
    }
}

async fn insert_history(
    db: &DbPool,
    task_id: i64,
    task_name: &str,
    started_at: chrono::NaiveDateTime,
    finished_at: chrono::NaiveDateTime,
    duration_ms: i64,
    summary: &RunSummary,
) -> Result<(), sqlx::Error> {
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "INSERT INTO crawler_run_histories \
                 (task_id, task_name, started_at, finished_at, duration_ms, status, \
                 block_type, crawled_count, new_count, skipped_count, failed_count, \
                 error_message, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
            )
            .bind(task_id)
            .bind(task_name)
            .bind(started_at)
            .bind(finished_at)
            .bind(duration_ms)
            .bind(summary.status)
            .bind(&summary.block_type)
            .bind(summary.crawled_count)
            .bind(summary.new_count)
            .bind(summary.skipped_count)
            .bind(summary.failed_count)
            .bind(&summary.error_message)
            .execute(pool)
            .await?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "INSERT INTO crawler_run_histories \
                 (task_id, task_name, started_at, finished_at, duration_ms, status, \
                 block_type, crawled_count, new_count, skipped_count, failed_count, \
                 error_message, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, CURRENT_TIMESTAMP)",
            )
            .bind(task_id)
            .bind(task_name)
            .bind(started_at)
            .bind(finished_at)
            .bind(duration_ms)
            .bind(summary.status)
            .bind(&summary.block_type)
            .bind(summary.crawled_count)
            .bind(summary.new_count)
            .bind(summary.skipped_count)
            .bind(summary.failed_count)
            .bind(&summary.error_message)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_url_absolute_passthrough() {
        assert_eq!(
            resolve_url("https://x.com/a", "https://y.com"),
            "https://x.com/a"
        );
    }

    #[test]
    fn resolve_url_root_relative() {
        assert_eq!(
            resolve_url("/p/1", "https://example.com/list/page"),
            "https://example.com/p/1"
        );
    }

    #[test]
    fn resolve_url_relative() {
        assert_eq!(
            resolve_url("p/2", "https://example.com/list/page"),
            "https://example.com/list/p/2"
        );
    }

    #[test]
    fn strip_html_tags_basic() {
        assert_eq!(strip_html_tags("<p>hello <b>world</b></p>"), "hello world");
        assert_eq!(strip_html_tags("<p>a&nbsp;b</p>"), "a b");
    }

    // T059 SC-005 幂等性测试：同 (task_id, source_url_canonical) 重复调用 100 次，DB 中仅 1 行
    #[tokio::test]
    async fn upsert_article_idempotent_on_repeated_calls() {
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect test db");
        sqlx::raw_sql(include_str!("../../../migrations/020_crawler_tasks_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 020 migration");
        sqlx::raw_sql(include_str!("../../../migrations/021_crawler_articles_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 021 migration");

        // 插入一个任务
        sqlx::query(
            "INSERT INTO crawler_tasks (name, list_urls, selectors) VALUES ('t-idem', 'https://x/l', '{}')",
        )
        .execute(&pool)
        .await
        .expect("insert task");
        let (task_id,): (i64,) =
            sqlx::query_as("SELECT id FROM crawler_tasks WHERE name = 't-idem'")
                .fetch_one(&pool)
                .await
                .expect("select task");

        let db = DbPool::Sqlite(pool.clone());
        let fields = ExtractedFields {
            title: Some("T".into()),
            content: Some("C".into()),
            category: None,
            tags: None,
            images: vec![],
            pan_links: vec![],
            direct_links: vec![],
            field_warnings: vec![],
        };

        let url = "https://example.com/a";
        let canonical = url; // 简化：直接用 URL 作 canonical
        let mut new_count = 0;
        for _ in 0..100 {
            let is_new = upsert_article_and_children(&db, task_id, "t-idem", url, canonical, &fields)
                .await
                .expect("upsert");
            if is_new {
                new_count += 1;
            }
        }

        // 仅首次为新增
        assert_eq!(new_count, 1, "first call should create, others should update");

        // DB 中仅 1 行
        let (total,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM crawler_articles WHERE task_id = ?")
                .bind(task_id)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(total, 1, "DB should have exactly 1 row after 100 calls");
    }

    // T057 SC-008 性能验证：1000 条种子数据，文章列表查询 + 单文章 20 图查询响应时延
    // 用 #[ignore] 标注，手动跑：cargo test perf_seed_1000_articles --release -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn perf_seed_1000_articles_list_query_under_1s() {
        use std::time::Instant;
        use sqlx::sqlite::SqlitePoolOptions;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect test db");
        sqlx::raw_sql(include_str!("../../../migrations/020_crawler_tasks_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 020 migration");
        sqlx::raw_sql(include_str!("../../../migrations/021_crawler_articles_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 021 migration");
        sqlx::raw_sql(include_str!("../../../migrations/022_crawler_article_links_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 022 migration");
        sqlx::raw_sql(include_str!("../../../migrations/023_crawler_article_images_sqlite.sql"))
            .execute(&pool)
            .await
            .expect("run 023 migration");

        // 插入任务
        sqlx::query(
            "INSERT INTO crawler_tasks (name, list_urls, selectors) VALUES ('perf-task', 'https://x/l', '{}')",
        )
        .execute(&pool)
        .await
        .expect("insert task");
        let (task_id,): (i64,) =
            sqlx::query_as("SELECT id FROM crawler_tasks WHERE name = 'perf-task'")
                .fetch_one(&pool)
                .await
                .expect("select task");

        // 批量插入 1000 篇文章
        let now = chrono::Utc::now().naive_utc();
        for i in 0..1000 {
            let url = format!("https://example.com/p/{i}");
            sqlx::query(
                "INSERT INTO crawler_articles \
                 (task_id, source_type, source_url, source_url_canonical, title, content, crawled_at, created_at, updated_at) \
                 VALUES (?, 'perf-task', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(task_id)
            .bind(&url)
            .bind(&url)
            .bind(format!("Title {i}"))
            .bind(format!("Content body {i}"))
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&pool)
            .await
            .expect("insert article");
        }

        // 性能测量：模拟 list_articles 的分页查询（带 join 子查询统计）
        let start = Instant::now();
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT a.id, a.title, a.source_url \
             FROM crawler_articles a \
             WHERE a.task_id = ? \
             ORDER BY a.crawled_at DESC \
             LIMIT 20 OFFSET 0",
        )
        .bind(task_id)
        .fetch_all(&pool)
        .await
        .expect("list query");
        let elapsed = start.elapsed();
        assert_eq!(rows.len(), 20);
        println!("list_articles(20 of 1000): {elapsed:?} (SC-008 budget: ≤1s)");
        assert!(elapsed.as_millis() < 1000, "SC-008 violated: list > 1s");

        // 单文章 20 图查询（SC-004 详情页 ≤ 2s 预算，本测试仅测 DB 部分）
        let (first_id,): (i64,) =
            sqlx::query_as("SELECT id FROM crawler_articles WHERE task_id = ? ORDER BY id LIMIT 1")
                .bind(task_id)
                .fetch_one(&pool)
                .await
                .expect("first id");
        for j in 0..20 {
            let img_url = format!("https://img.example.com/{j}.jpg");
            sqlx::query(
                "INSERT INTO crawler_article_images (article_id, original_url, url_canonical, status, retry_count) \
                 VALUES (?, ?, ?, 'pending', 0)",
            )
            .bind(first_id)
            .bind(&img_url)
            .bind(&img_url)
            .execute(&pool)
            .await
            .expect("insert image");
        }
        let start = Instant::now();
        let _imgs: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, original_url FROM crawler_article_images WHERE article_id = ?",
        )
        .bind(first_id)
        .fetch_all(&pool)
        .await
        .expect("images query");
        let elapsed = start.elapsed();
        println!("get_article 20 images: {elapsed:?} (SC-004 DB portion)");
        assert!(elapsed.as_millis() < 2000, "SC-004 violated: images > 2s");
    }

    // ===== 自动翻页：extract_pagination_urls =====

    const PAGE_HTML_WITH_NEXT: &str = r#"<!DOCTYPE html>
<html><body>
  <ul class="list">
    <li><a href="/p/1">1</a></li>
    <li><a href="/p/2">2</a></li>
  </ul>
  <a class="next" href="/list?page=3">下一页 ›</a>
</body></html>"#;

    const PAGE_HTML_PAGINATION_FULL: &str = r#"<!DOCTYPE html>
<html><body>
  <ul class="list"><li>...</li></ul>
  <div class="pagination">
    <a href="/list?page=1" class="active">1</a>
    <a href="/list?page=2">2</a>
    <a href="/list?page=3">3</a>
    <a href="/list?page=4">4</a>
    <a href="/list?page=2" class="next">下一页</a>
  </div>
</body></html>"#;

    const PAGE_HTML_LAST: &str = r#"<!DOCTYPE html>
<html><body>
  <ul class="list">
    <li><a href="/p/99">99</a></li>
  </ul>
  <span class="no-more">已到末页</span>
</body></html>"#;

    #[test]
    fn extract_pagination_urls_finds_single_next() {
        // 单一 next 链接场景
        let urls = extract_pagination_urls(PAGE_HTML_WITH_NEXT, "a.next");
        assert_eq!(urls, vec!["/list?page=3"]);
    }

    #[test]
    fn extract_pagination_urls_collects_all_page_links() {
        // 分页选择器：一次性抓 1/2/3/4 + 下一页（共 5 个链接，按 DOM 顺序）
        let urls = extract_pagination_urls(PAGE_HTML_PAGINATION_FULL, ".pagination a");
        assert_eq!(
            urls,
            vec![
                "/list?page=1",
                "/list?page=2",
                "/list?page=3",
                "/list?page=4",
                "/list?page=2", // 下一页链接（同 URL，由调用方去重）
            ]
        );
    }

    #[test]
    fn extract_pagination_urls_supports_multiple_selectors() {
        // 第一个选择器 a[rel=next] 未命中，第二个 a.next 命中
        let urls = extract_pagination_urls(PAGE_HTML_WITH_NEXT, "a[rel=next], a.next");
        assert_eq!(urls, vec!["/list?page=3"]);
    }

    #[test]
    fn extract_pagination_urls_empty_on_last_page() {
        // 末页：没有匹配元素，返回空 Vec → 不再扩散
        let urls = extract_pagination_urls(PAGE_HTML_LAST, "a.next");
        assert!(urls.is_empty());
    }

    #[test]
    fn extract_pagination_urls_invalid_selector_returns_empty() {
        let urls = extract_pagination_urls(PAGE_HTML_WITH_NEXT, ">>>");
        assert!(urls.is_empty());
    }

    #[test]
    fn extract_pagination_urls_descendant_links_in_container() {
        // 选择器命中的是容器（.pagination），后代扫描找到所有 <a>
        const HTML: &str = r#"<div class="pagination">
            <a href="/p/1">1</a><a href="/p/2">2</a>
        </div>"#;
        let urls = extract_pagination_urls(HTML, ".pagination");
        assert_eq!(urls, vec!["/p/1", "/p/2"]);
    }

    #[test]
    fn extract_pagination_urls_ignores_empty_href() {
        // href="" 不应当返回
        const HTML: &str = r#"<div class="pagination">
            <a href="">empty</a><a href="/p/2">2</a>
        </div>"#;
        let urls = extract_pagination_urls(HTML, ".pagination");
        assert_eq!(urls, vec!["/p/2"]);
    }

    #[test]
    fn resolve_url_query_only_relative() {
        // 自动翻页典型场景：相对的 ?page=2
        assert_eq!(
            resolve_url("?page=2", "https://example.com/list?page=1"),
            "https://example.com/list?page=2"
        );
    }
}
