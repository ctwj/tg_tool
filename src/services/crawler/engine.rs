//! 单任务抓取引擎（feature 043-crawler-configurator）
//!
//! **043 重写**：取代 042 旧 `FieldSelectors` + `extract_fields` 路径。
//! 现在通过 `crawler_task_field_nodes` 加载 `FieldTree`，两阶段执行：
//! - **列表页阶段**：fetch 每个 list_url → 应用所有 `scope='list_page'` 字段
//!   （递归处理父子嵌套，父字段命中N条则子字段作用域为父的 N 次单条片段）
//! - **详情页阶段**：从列表页阶段产出的"链接卡片"父字段（field_type=link_card）
//!   抽取 detail URL → fetch 每条详情 → 应用 `scope='detail_page'` 字段
//!
//! 落库（T036）：
//! - 每条字段命中（含嵌套子字段）写入 `crawler_article_field_values`（field_path 物化路径）
//! - 简单单值字段聚合写入 `crawler_articles.extra_fields_json`（列表页快速渲染用）
//! - 未命中字段写入 `is_hit=0` 行用于 FR-027 统计
//! - 单字段失败不中断其他字段（FR-019）

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use serde_json::Value;

use crate::models::crawler_field_node::{FieldNodeRow, FieldTreeNode, FieldTree};
use crate::services::crawler::block_detector::{detect_block, BlockType};
use crate::services::crawler::extractor::{
    apply_post_processors, extract, ExtractError, ExtractInput, Hit,
};
use crate::services::crawler::field_schema::{FieldType, Rule, Scope, SourceLayer};
use crate::services::crawler::source_layer::fetch_source_material;
use crate::services::crawler::url_normalize::normalize_url;
use crate::state::{AppState, DbPool};

/// 默认 User-Agent（feature 042 templates.rs 的常量迁移至此，避免循环引用）
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/130.0.0.0 Safari/537.36";

// ============================================================================
// 公共结构体（保留签名兼容性，US1 重写时可能调整）
// ============================================================================

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

/// 任务运行时视图：加载任务 + 字段树 + 列表 URL 解析
#[derive(Debug, Clone)]
pub struct TaskRuntime {
    pub task_id: i64,
    pub task_name: String,
    pub source_type: String,
    pub list_urls: Vec<String>,
    pub user_agent: Option<String>,
    pub proxy: Option<String>,
    pub task_concurrency: i64,
    pub request_delay_ms: i64,
    pub pagination_selector: Option<String>,
    pub max_pages: i64,
    /// 043 US5：字段树 pagination 字段驱动的最大翻页深度（0=不限）
    pub max_pagination_depth: i64,
    /// 044：全量采集开关（true=每次全量；false=连续 3 页零新增早停）
    pub force_full_collect: bool,
    /// 045：URL 模板分页模板（含 {page} 占位符）；空串=未启用（走字段树 pagination 分页）
    pub page_url_template: String,
    /// 045：模板生成页码起始值
    pub page_start: i64,
    /// 045：模板生成页码上限（0=不限）
    pub page_end: i64,
    pub field_tree: FieldTree,
}

// ============================================================================
// HTTP 工具（与 selectors 无关，永久保留）
// ============================================================================

pub fn build_reqwest_client_pub(
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<reqwest::Client, reqwest::Error> {
    build_reqwest_client(user_agent, proxy)
}

pub fn build_reqwest_client(
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<reqwest::Client, reqwest::Error> {
    let ua = user_agent.unwrap_or(DEFAULT_USER_AGENT);
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

/// 跳转跟踪并发上限
const REDIRECT_CONCURRENCY: usize = 4;

/// 构建「不跟随重定向」的 reqwest 客户端（用于跳转跟踪 HEAD/GET 解析真实 URL）
pub fn build_no_redirect_client(
    user_agent: Option<&str>,
    proxy: Option<&str>,
) -> Result<reqwest::Client, reqwest::Error> {
    let ua = user_agent.unwrap_or(DEFAULT_USER_AGENT);
    let mut builder = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(p) = proxy
        && !p.is_empty()
        && let Ok(proxy) = reqwest::Proxy::all(p)
    {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

/// 通过 AppState 抓 URL — 优先任务级代理，回退系统 http_proxy_url
///
/// 返回 (status, body, headers) 三元组。US1 T019 source_layer.rs 复用此实现。
pub async fn fetch_url(
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

/// 从 3xx 响应提取 Location 并补全为绝对 URL
fn location_from_response(resp: &reqwest::Response, base_url: &str) -> Option<String> {
    let status = resp.status().as_u16();
    if !(300..400).contains(&status) {
        return None;
    }
    let loc = resp.headers().get(reqwest::header::LOCATION)?;
    let loc_str = loc.to_str().ok()?;
    if loc_str.is_empty() {
        return None;
    }
    Some(resolve_url(loc_str, base_url))
}

async fn resolve_one(client: &reqwest::Client, url: &str, base_url: &str) -> String {
    let absolute = resolve_url(url, base_url);
    if absolute != url {
        tracing::debug!(from = url, to = %absolute, "resolve_one: relative url expanded to absolute");
    }
    match client.head(&absolute).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if let Some(resolved) = location_from_response(&resp, base_url) {
                return resolved;
            }
            if status == 405 || status == 501 {
                return resolve_one_get(client, &absolute, base_url).await;
            }
            absolute
        }
        Err(e) => {
            tracing::debug!(?e, url = %absolute, "redirect HEAD failed, keeping original");
            absolute
        }
    }
}

async fn resolve_one_get(client: &reqwest::Client, url: &str, base_url: &str) -> String {
    match client.get(url).send().await {
        Ok(resp) => {
            if let Some(resolved) = location_from_response(&resp, base_url) {
                return resolved;
            }
            url.to_string()
        }
        Err(e) => {
            tracing::debug!(?e, url, "redirect GET fallback failed, keeping original");
            url.to_string()
        }
    }
}

/// 对一组链接并发跟踪一次 HTTP 重定向，返回真实 URL 列表（失败兜底原 URL）
pub async fn resolve_redirects(
    client: &reqwest::Client,
    links: &[String],
    base_url: &str,
    follow: bool,
) -> Vec<String> {
    if !follow || links.is_empty() {
        return links.to_vec();
    }
    stream::iter(links.iter().cloned())
        .map(|u| async move { resolve_one(client, &u, base_url).await })
        .buffer_unordered(REDIRECT_CONCURRENCY)
        .collect()
        .await
}

/// 相对 URL → 绝对 URL（基于 base_url）；已是绝对路径则原样返回
pub fn resolve_url(href: &str, base: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Some(idx) = base.find("://") {
        let base_scheme_end = idx + 3;
        let after_scheme = &base[base_scheme_end..];
        if let Some(slash) = after_scheme.find('/') {
            let origin = &base[..base_scheme_end + slash];
            if let Some(stripped) = href.strip_prefix('/') {
                return format!("{origin}/{stripped}");
            }
            let base_path = &after_scheme[slash..];
            if let Some(last_slash) = base_path.rfind('/') {
                let dir = &base_path[..last_slash + 1];
                let origin_full = &base[..base_scheme_end + slash];
                return format!("{origin_full}{dir}{href}");
            }
            return format!("{origin}/{href}");
        }
        return format!("{base}/{href}");
    }
    href.to_string()
}

/// 从 HTML 中按 CSS 选择器提取分页链接
pub fn extract_pagination_urls(html: &str, selector: &str) -> Vec<String> {
    if selector.is_empty() {
        return Vec::new();
    }
    let document = scraper::Html::parse_document(html);
    let Ok(sels) = scraper::Selector::parse(selector) else {
        return Vec::new();
    };
    document
        .select(&sels)
        .filter_map(|el| el.value().attr("href").map(str::to_string))
        .collect()
}

// ============================================================================
// 任务加载（US1 T035）
// ============================================================================

/// 加载任务运行时视图：基本信息 + 字段树
pub async fn load_task(db: &DbPool, task_id: i64) -> Result<TaskRuntime, String> {
    let task: crate::models::crawler_task::CrawlerTask = match db {
        DbPool::Sqlite(pool) => sqlx::query_as("SELECT * FROM crawler_tasks WHERE id = ?")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("任务 {task_id} 加载失败: {e}"))?,
        DbPool::Postgres(pool) => sqlx::query_as("SELECT * FROM crawler_tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("任务 {task_id} 加载失败: {e}"))?,
    };

    let list_urls: Vec<String> = serde_json::from_str::<Vec<String>>(&task.list_urls)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    let field_tree = load_field_tree(db, task_id).await?;

    Ok(TaskRuntime {
        task_id,
        task_name: task.name.clone(),
        source_type: task.name,
        list_urls,
        user_agent: task.user_agent,
        proxy: task.proxy,
        task_concurrency: task.task_concurrency.max(1),
        request_delay_ms: task.request_delay_ms.max(0),
        pagination_selector: task.pagination_selector,
        max_pages: task.max_pages,
        max_pagination_depth: task.max_pagination_depth,
        force_full_collect: task.force_full_collect,
        page_url_template: task.page_url_template,
        page_start: task.page_start.max(1),
        page_end: task.page_end.max(0),
        field_tree,
    })
}

/// 从 DB 加载字段树（按 task_id 查所有节点 → from_rows 组装）
pub async fn load_field_tree(db: &DbPool, task_id: i64) -> Result<FieldTree, String> {
    let rows: Vec<FieldNodeRow> = match db {
        DbPool::Sqlite(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = ? ORDER BY sort_order, id",
            )
            .bind(task_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("加载字段节点失败: {e}"))?
        }
        DbPool::Postgres(pool) => {
            sqlx::query_as::<_, FieldNodeRow>(
                "SELECT * FROM crawler_task_field_nodes WHERE task_id = $1 ORDER BY sort_order, id",
            )
            .bind(task_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("加载字段节点失败: {e}"))?
        }
    };
    Ok(crate::models::crawler_field_node::from_rows(rows))
}

// ============================================================================
// 字段提取 — 递归父子嵌套（US1 T035 + US2 T042-T045 完整版）
// ============================================================================

/// 单条字段在某个上下文（HTML/源码素材）下的提取结果
#[derive(Debug, Clone, Default)]
pub struct FieldExtraction {
    /// 物化路径：`/list_page/link_card/title`
    pub field_path: String,
    /// field_node id（用于回写 field_node_id）
    pub field_node_id: Option<i64>,
    /// scope 字符串
    pub scope: String,
    /// 命中列表（post_processor 之后）；空=未命中
    pub hits: Vec<Hit>,
    /// 提取失败的错误信息（不中断其他字段，仅记 warning）
    pub error: Option<String>,
}

/// 提取一层字段（同 parent 下的兄弟节点），递归处理子节点
///
/// `parent_hits`：父字段命中的片段集合（用于 US2 嵌套作用域）
///  - 当 `parent_hits` 非空，每个 hit 的 `source_fragment` 被作为局部 HTML 上下文重新解析
///  - 当 `parent_hits` 为空（顶层字段），使用完整 material 作为上下文
///
/// 递归：每个 node 提取后，用其命中作为子节点的 parent_hits，
/// 子节点的结果追加到 out（field_path 自带父子层级）
fn extract_layer(
    nodes: &[FieldTreeNode],
    material: &crate::services::crawler::source_layer::SourceMaterial,
    parent_hits: &[Hit],
    scope_str: &str,
    parent_path: &str,
) -> Vec<FieldExtraction> {
    let mut out = Vec::new();
    for node in nodes {
        let row = &node.row;
        if !row.is_active {
            continue;
        }
        let spec = match row.to_spec() {
            Ok(s) => s,
            Err(e) => {
                out.push(FieldExtraction {
                    field_path: format!("{parent_path}/{}", row.name),
                    field_node_id: Some(row.id),
                    scope: scope_str.to_string(),
                    hits: Vec::new(),
                    error: Some(format!("字段 spec 解析失败: {e}")),
                });
                continue;
            }
        };

        let field_path = format!("{parent_path}/{}", row.name);
        let layer = spec.source_layer;

        // 决定提取上下文：父命中片段（每条独立提取）或完整素材
        let (mut hits, err) = if parent_hits.is_empty() {
            // 顶层字段：直接在完整素材上提取
            extract_one(&spec.rule, layer, material, spec.script_index, &spec.post_processors, &material.final_url)
        } else {
            // 子字段：在父命中片段上逐条提取，结果合并
            let mut combined = Vec::new();
            let mut last_err = None;
            for (i, ph) in parent_hits.iter().enumerate() {
                // 父命中若是 HTML 片段，构造虚拟子素材
                let sub_material = make_sub_material_from_hit(ph, material);
                let (mut hs, e) = extract_one(
                    &spec.rule,
                    layer,
                    &sub_material,
                    spec.script_index,
                    &spec.post_processors,
                    &material.final_url,
                );
                if let Some(e) = e {
                    last_err = Some(e);
                }
                // 给每条命中打上 parent_index 标记，便于追溯
                for h in hs.iter_mut() {
                    h.location = Some(format!(
                        "parent[{}]{}",
                        i,
                        h.location.as_deref().map(|l| format!("::{l}")).unwrap_or_default()
                    ));
                }
                combined.append(&mut hs);
            }
            (combined, last_err)
        };

        // URL 类字段自动绝对化（避免相对 URL 进入下游）
        // 适用：url / link_card / pagination / image
        if matches!(
            spec.field_type,
            FieldType::Url | FieldType::LinkCard | FieldType::Pagination | FieldType::Image
        ) {
            for h in hits.iter_mut() {
                h.value = resolve_url(&h.value, &material.final_url);
            }
        }

        out.push(FieldExtraction {
            field_path: field_path.clone(),
            field_node_id: Some(row.id),
            scope: scope_str.to_string(),
            hits: hits.clone(),
            error: err,
        });

        // 递归处理子节点（以当前节点的 hits 作为 parent_hits）
        if !node.children.is_empty() {
            let mut child_extractions = extract_layer(
                &node.children,
                material,
                &hits,
                scope_str,
                &field_path,
            );
            out.append(&mut child_extractions);
        }
    }
    out
}

/// 对单条规则执行完整提取链：layer → extract → post_processors
///
/// 返回 (hits, optional_error)
fn extract_one(
    rule: &Rule,
    layer: SourceLayer,
    material: &crate::services::crawler::source_layer::SourceMaterial,
    script_index: Option<i32>,
    post_processors: &[crate::services::crawler::field_schema::PostProcessor],
    base_url: &str,
) -> (Vec<Hit>, Option<String>) {
    // follow_url 字段需 async 两阶段提取，extract_layer 同步路径跳过；
    // 真正的两阶段提取由 collect_follow_url_extractions 在 run_task 中并发执行
    if let Rule::FollowUrl(_) = rule {
        return (
            Vec::new(),
            Some("follow_url 由异步两阶段提取处理，同步路径跳过".into()),
        );
    }
    let input = ExtractInput::from_material(material, script_index).with_layer(layer);
    match extract(rule, &input) {
        Ok(hits) => {
            let processed = apply_post_processors(hits, post_processors, base_url);
            (processed, None)
        }
        Err(ExtractError { kind: _, message }) => (Vec::new(), Some(message)),
    }
}

/// 对 detail_page 顶层 FollowUrl 字段并发执行两阶段提取（extract_layer 同步路径已跳过它们）
///
/// 单字段失败 graceful degrade：返回带 error 的 FieldExtraction，不 panic、不影响其他字段。
/// 单文章内 FollowUrl 字段数通常 <5，直接 `join_all` 并发，不引入新 semaphore。
async fn collect_follow_url_extractions(
    nodes: &[FieldTreeNode],
    material: &crate::services::crawler::source_layer::SourceMaterial,
    ua: Option<&str>,
    proxy: Option<&str>,
) -> Vec<FieldExtraction> {
    use crate::services::crawler::field_schema::FieldType;
    use crate::services::crawler::follow_url::{extract_follow_url_async, FollowUrlError};

    // 仅处理顶层（rule=FollowUrl）节点。子节点中的 FollowUrl 不支持（父子嵌套作用域
    // 与 follow_url 二次请求作用域冲突），由 extract_one 跳过、此处也不接管。
    let fu_nodes: Vec<&FieldTreeNode> = nodes
        .iter()
        .filter(|n| {
            n.row
                .to_spec()
                .map(|s| matches!(s.rule, Rule::FollowUrl(_)))
                .unwrap_or(false)
        })
        .collect();
    if fu_nodes.is_empty() {
        return Vec::new();
    }

    let futures_iter = fu_nodes.into_iter().map(|n| async move {
        let spec = match n.row.to_spec() {
            Ok(s) => s,
            Err(e) => {
                return FieldExtraction {
                    field_path: format!("/{}/{}", Scope::DetailPage.as_str(), n.row.name),
                    field_node_id: Some(n.row.id),
                    scope: Scope::DetailPage.as_str().to_string(),
                    hits: Vec::new(),
                    error: Some(format!("字段 spec 解析失败: {e}")),
                };
            }
        };
        let fu = match &spec.rule {
            Rule::FollowUrl(fu) => fu,
            _ => unreachable!("filter 已保证只处理 FollowUrl"),
        };
        let result = extract_follow_url_async(fu, material, ua, proxy).await;
        let (mut hits, err) = match result {
            Ok(h) => (h, None),
            Err(FollowUrlError::TransitEmpty) => (
                Vec::new(),
                Some("transit 子规则未提取到中转 URL".into()),
            ),
            Err(FollowUrlError::TransitExtract(e)) => {
                (Vec::new(), Some(format!("transit 提取失败: {e}")))
            }
            Err(FollowUrlError::Fetch(e)) => {
                (Vec::new(), Some(format!("二次请求失败: {e}")))
            }
            Err(FollowUrlError::ExtractExtract(e)) => {
                (Vec::new(), Some(format!("extract 提取失败: {e}")))
            }
            Err(FollowUrlError::ZeroHits) => {
                (Vec::new(), Some("extract 子规则在二次响应 0 命中".into()))
            }
        };
        // 应用 post_processors
        if !spec.post_processors.is_empty() {
            hits = apply_post_processors(hits, &spec.post_processors, &material.final_url);
        }
        // URL 类字段自动绝对化（与 extract_layer 行为一致）
        if matches!(
            spec.field_type,
            FieldType::Url | FieldType::LinkCard | FieldType::Pagination | FieldType::Image
        ) {
            for h in hits.iter_mut() {
                h.value = resolve_url(&h.value, &material.final_url);
            }
        }
        tracing::debug!(
            target: "crawler",
            "follow_url field '{}' extracted: {} hits, error={:?}",
            n.row.name,
            hits.len(),
            err
        );
        FieldExtraction {
            field_path: format!("/{}/{}", Scope::DetailPage.as_str(), n.row.name),
            field_node_id: Some(n.row.id),
            scope: Scope::DetailPage.as_str().to_string(),
            hits,
            error: err,
        }
    });
    futures::future::join_all(futures_iter).await
}

/// 把父字段命中片段（HTML 文本）包装为可被提取器消费的子素材
///
/// 优先使用 `hit.context_html`（CSS 模式下捕获的命中元素外部 HTML），
/// 这样子字段可以用相对选择器（如 `img.cover`）在父元素范围内提取。
/// 对非 CSS 模式（regex / prefix_suffix），fallback 到 `hit.value`。
fn make_sub_material_from_hit(
    ph: &Hit,
    parent: &crate::services::crawler::source_layer::SourceMaterial,
) -> crate::services::crawler::source_layer::SourceMaterial {
    use crate::services::crawler::source_layer::{MetaTag, ScriptBlock};
    let html = ph.context_html.clone().unwrap_or_else(|| ph.value.clone());
    // 复用父素材的 headers / final_url；scripts/metas 留空（子字段通常无需）
    crate::services::crawler::source_layer::SourceMaterial {
        final_url: parent.final_url.clone(),
        status: parent.status,
        headers: parent.headers.clone(),
        html,
        scripts: Vec::<ScriptBlock>::new(),
        metas: Vec::<MetaTag>::new(),
        fetched_at: parent.fetched_at,
        duration_ms: 0,
    }
}

// ============================================================================
// 抓取主循环（US1 T035）
// ============================================================================

/// 045：判断任务是否正在运行（crawler_run_histories 存在 status='running' 行）
///
/// 用于手动 /run 与调度器 tick 触发前防重——同一任务同时只允许一个运行
pub async fn is_task_running(db: &DbPool, task_id: i64) -> bool {
    let row: Option<(i64,)> = match db {
        DbPool::Sqlite(pool) => sqlx::query_as(
            "SELECT id FROM crawler_run_histories WHERE task_id = ? AND status = 'running' LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten(),
        DbPool::Postgres(pool) => sqlx::query_as(
            "SELECT id FROM crawler_run_histories WHERE task_id = $1 AND status = 'running' LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten(),
    };
    row.is_some()
}

/// 立即运行一次任务
pub async fn run_task(task_id: i64, state: &AppState) -> Result<RunSummary, String> {
    let started = Instant::now();
    let rt = load_task(&state.db, task_id).await?;

    // 045：先插一条 running 历史行，让前端「爬虫历史」立即看到任务运行中；结尾 update 成最终状态
    let run_id = insert_run_history(
        &state.db, task_id, &rt.task_name, 0, "running", None, 0, 0, 0, 0, None, None,
    )
    .await
    .unwrap_or(0);

    tracing::info!(
        target: "crawler",
        "Task {task_id} ({}) run: list_urls={} list_fields={} detail_fields={}",
        rt.task_name,
        rt.list_urls.len(),
        rt.field_tree.list_page.len(),
        rt.field_tree.detail_page.len(),
    );

    let mut crawled = 0i64;
    let mut new_count = 0i64;
    let mut skipped = 0i64;
    let mut failed = 0i64;
    let mut first_block: Option<(BlockType, String)> = None;
    let mut errors: Vec<String> = Vec::new();

    // 系统代理兜底
    let sys_proxy = state.http_proxy_url().await;
    let proxy = rt.proxy.clone().or(sys_proxy);

    // 045 US2：跨 seed 全局列表页去重集合（种子页/DOM 提取页/模板生成页三类来源统一判定）
    let mut visited_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    // 045 US2：跨 seed/跨页 全局详情去重集合（避免重复发起 detail 网络请求）
    let mut detail_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 045：翻页起点。纯模板模式（配了模板且无入口 list_urls）用模板第 page_start 页作起点；
    // 入口/混合模式用 list_urls 作种子。
    let template_mode = !rt.page_url_template.is_empty();
    let pure_template = template_mode && rt.list_urls.is_empty();
    let seeds: Vec<String> = if pure_template {
        build_template_url(&rt.page_url_template, rt.page_start, "")
            .map(|u| vec![u])
            .unwrap_or_default()
    } else {
        rt.list_urls.clone()
    };

    for seed_url in &seeds {
        // US5 T055：链式翻页，直到无下一页 / 达到 max_pagination_depth / 循环检测
        let mut current_url: Option<String> = Some(seed_url.clone());
        let mut depth = 0i64;
        let mut empty_pages = 0i64;          // 044：每 seed 独立的连续零新增页数
        const EMPTY_PAGE_LIMIT: i64 = 3;     // 044：连续 3 整页零新增即停
        // 045 US1：template_mode 在循环外声明；纯模板模式第 page_start 页已作起点，游标从 +1 开始
        let mut template_page = if pure_template { rt.page_start + 1 } else { rt.page_start };
        while let Some(list_url) = current_url.take() {
            // 翻页深度限制（仅入口/DOM 模式生效；模板模式由 page_end 独占边界，045）
            if !template_mode && rt.max_pagination_depth > 0 && depth >= rt.max_pagination_depth {
                break;
            }
            depth += 1;

            // 循环检测：跳过已抓过的 URL（045：跨 seed 全局）
            let canonical_list = normalize_url(&list_url);
            if !visited_urls.insert(canonical_list) {
                // 已处理过该列表页（跨 seed 重复 / 模板生成页与种子页重复）
                if template_mode {
                    // 模板模式：推进到下一页码继续（不中断整条翻页链）
                    match next_template_page(&rt, template_page, seed_url) {
                        Some(u) => {
                            template_page += 1;
                            current_url = Some(u);
                            continue;
                        }
                        None => break,
                    }
                } else {
                    break;
                }
            }

            // 抓列表页素材
            let material = match fetch_source_material(&list_url, rt.user_agent.as_deref(), proxy.as_deref()).await {
                Ok(m) => m,
                Err(e) => {
                    // 拦截感知：ProbeError.category=Blocked/Http4xx5xx 时视为拦截
                    if matches!(
                        e.category,
                        crate::services::crawler::source_layer::ProbeCategory::Blocked
                            | crate::services::crawler::source_layer::ProbeCategory::Http4xx5xx
                    ) {
                        let bt = BlockType::HttpBlocked(0);
                        if first_block.is_none() {
                            first_block = Some((bt.clone(), format!("list_url {list_url}: {}", e.message)));
                        }
                    }
                    failed += 1;
                    errors.push(format!("list {list_url}: {}", e.message));
                    // 045：模板模式单页失败（如 404）跳过该页继续下一页码；DOM 模式 break（现状）
                    if template_mode {
                        match next_template_page(&rt, template_page, seed_url) {
                            Some(u) => {
                                template_page += 1;
                                current_url = Some(u);
                                continue;
                            }
                            None => break,
                        }
                    } else {
                        break;
                    }
                }
            };

            // 反爬检测
            let header_pairs: Vec<(String, String)> = material
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if let Some(bt) = detect_block(material.status, &material.html, &header_pairs) {
                if first_block.is_none() {
                    first_block = Some((bt.clone(), format!("list_url {list_url} blocked: {bt}")));
                }
                failed += 1;
                break;
            }

            // 应用 list_page 字段
            let list_extractions = extract_layer(
                &rt.field_tree.list_page,
                &material,
                &[],
                Scope::ListPage.as_str(),
                &format!("/{}", Scope::ListPage.as_str()),
            );

            // 从 list_page 字段中找 link_card 类型的父字段 → 提取 detail 链接
            let detail_links = collect_detail_links(
                &list_extractions,
                &rt.field_tree.list_page,
                &format!("/{}", Scope::ListPage.as_str()),
                &material.final_url,
                &mut detail_seen,
            );

            crawled += detail_links.len() as i64;
            let new_before = new_count; // 044：本页 detail 循环前的 new_count 快照

            // 落库每条详情
            for detail_url in &detail_links {
                let canonical_d = normalize_url(detail_url);
                // 详情页素材
                let dmaterial = match fetch_source_material(
                    detail_url,
                    rt.user_agent.as_deref(),
                    proxy.as_deref(),
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        failed += 1;
                        errors.push(format!("detail {detail_url}: {}", e.message));
                        continue;
                    }
                };

                let d_header_pairs: Vec<(String, String)> = dmaterial
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if let Some(bt) = detect_block(dmaterial.status, &dmaterial.html, &d_header_pairs) {
                    if first_block.is_none() {
                        first_block = Some((bt.clone(), format!("detail {detail_url} blocked: {bt}")));
                    }
                    failed += 1;
                    continue;
                }

                // 应用 detail_page 字段
                let mut detail_extractions = extract_layer(
                    &rt.field_tree.detail_page,
                    &dmaterial,
                    &[],
                    Scope::DetailPage.as_str(),
                    &format!("/{}", Scope::DetailPage.as_str()),
                );

                // follow_url 字段并发两阶段提取（extract_layer 同步路径已跳过）
                // 单字段失败 graceful degrade，不影响其他字段、不算 failed（遵循 042 设计原则）
                let fu_extractions = collect_follow_url_extractions(
                    &rt.field_tree.detail_page,
                    &dmaterial,
                    rt.user_agent.as_deref(),
                    proxy.as_deref(),
                )
                .await;
                if !fu_extractions.is_empty() {
                    tracing::debug!(
                        target: "crawler",
                        "detail {detail_url}: follow_url 字段提取 {} 条",
                        fu_extractions.len()
                    );
                    detail_extractions.extend(fu_extractions);
                }

                // upsert 文章 + 写 field_values + extra_fields_json
                match upsert_article_with_fields(
                    &state.db,
                    task_id,
                    &rt.source_type,
                    detail_url,
                    &canonical_d,
                    &detail_extractions,
                )
                .await
                {
                    Ok(true) => new_count += 1,
                    Ok(false) => skipped += 1,
                    Err(e) => {
                        failed += 1;
                        errors.push(format!("upsert {detail_url}: {e}"));
                    }
                }

                // 请求间隔
                if rt.request_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(rt.request_delay_ms as u64)).await;
                }
            }

            // 044：早停判定（仅 force_full_collect=false 时启用）
            if should_stop_after_page(
                rt.force_full_collect,
                new_before,
                new_count,
                &mut empty_pages,
                EMPTY_PAGE_LIMIT,
            ) {
                tracing::info!(
                    target: "crawler",
                    "任务 {} 种子 {} 早停：连续 {EMPTY_PAGE_LIMIT} 页零新增，停止翻页（已采到历史边界）",
                    rt.task_id, seed_url
                );
                break;
            }

            // 045：定位下一页 —— 模板模式优先独占（build_template_url），否则 DOM pagination 字段
            if template_mode {
                let next = next_template_page(&rt, template_page, &material.final_url);
                template_page += 1;
                current_url = next;
            } else {
                // US5 T055：DOM 模式 —— pagination 字段命中
                current_url = find_next_page_url(
                    &list_extractions,
                    &rt.field_tree.list_page,
                    &format!("/{}", Scope::ListPage.as_str()),
                    &material.final_url,
                );
            }

            // 翻页间隔（避免过快）
            if current_url.is_some() && rt.request_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(rt.request_delay_ms as u64)).await;
            }
        }
    }

    let status: &'static str = match (first_block.as_ref(), new_count, failed) {
        (Some(_), _, _) => "blocked",
        (_, 0, f) if f > 0 => "failed",
        (_, n, f) if n > 0 && f > 0 => "partial",
        _ => "success",
    };
    let duration_ms = started.elapsed().as_millis() as i64;

    // 写 run_history（045：把开头插的 running 行 update 成最终状态）
    let err_msg = if errors.is_empty() {
        None
    } else {
        Some(errors.join("; ").chars().take(500).collect())
    };
    let _ = update_run_history(
        &state.db,
        run_id,
        duration_ms,
        status,
        first_block.as_ref().map(|(b, _)| b.as_str()),
        crawled,
        new_count,
        skipped,
        failed,
        err_msg,
    )
    .await;

    // 调度时间更新（不覆盖 finalize_run 的并发安全语义）
    let _ = update_task_schedule(&state.db, task_id, status).await;

    Ok(RunSummary {
        task_id,
        task_name: rt.task_name,
        status,
        block_type: first_block.map(|(b, _)| b.as_str()),
        crawled_count: crawled,
        new_count,
        skipped_count: skipped,
        failed_count: failed,
        error_message: None,
    })
}

/// 从列表页提取结果中收集所有详情链接（来自 link_card 类型字段的 url 子字段，
/// 或回退到 css 命中的 href）
///
/// 实现简化：扫描 list_page 字段树找 field_type=link_card 节点，
/// 该节点的命中值（href 或 text）即视为详情链接
///
/// `scope_path` 为 extract_layer 调用时的 parent_path（如 "/list_page"），
/// 必须与 extractions 中的 field_path 前缀保持一致
fn collect_detail_links(
    extractions: &[FieldExtraction],
    tree_nodes: &[FieldTreeNode],
    scope_path: &str,
    base_url: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<String> {
    let mut links: Vec<String> = Vec::new();

    // 建立 field_path → field_type 的索引（与 extract_layer 的 parent_path 对齐）
    let type_by_path: HashMap<String, FieldType> = build_type_index(tree_nodes, scope_path);

    for ext in extractions {
        // 判断该字段是否 link_card 或 url 类型
        // 注意：Pagination 字段是"下一页列表页"语义，不应作为详情链接入库
        // （Pagination 由 find_next_page_url 单独处理，驱动 while 循环翻页）
        let is_link = type_by_path
            .get(&ext.field_path)
            .map(|t| matches!(t, FieldType::LinkCard | FieldType::Url))
            .unwrap_or(false);
        if !is_link {
            continue;
        }
        for h in &ext.hits {
            let v = h.value.trim();
            if v.is_empty() {
                continue;
            }
            // URL 合法性兜底：跳过明显不是 URL 的命中值
            // 典型场景：LinkCard 字段配 attr=html，命中值是元素的 outerHTML 字符串
            // （如 `<a class="..." href="/x">...</a>`），若直接 resolve_url 会拼成脏 URL
            // 入库并 fetch 失败。合法 URL 不会含 `<` `>`，也不会含未转义的内部空白。
            if v.contains('<') || v.contains('>') {
                continue;
            }
            if v.split_whitespace().count() > 1 {
                continue;
            }
            let abs = resolve_url(v, base_url);
            // 045：key 统一规范化（normalize_url），修复 ?a=1&b=2 与 ?b=2&a=1 误判；seen 跨页/跨 seed 共享
            let key = normalize_url(&abs);
            if seen.insert(key) {
                links.push(abs);
            }
        }
    }
    links
}

/// 递归构建 field_path → FieldType 映射
fn build_type_index(
    nodes: &[FieldTreeNode],
    parent_path: &str,
) -> HashMap<String, FieldType> {
    let mut map = HashMap::new();
    for node in nodes {
        let p = format!("{parent_path}/{}", node.row.name);
        if let Ok(spec) = node.row.to_spec() {
            map.insert(p.clone(), spec.field_type);
        }
        map.extend(build_type_index(&node.children, &p));
    }
    map
}

/// US5 T055：从 list_page 字段树中找 `field_type='pagination'` 节点的命中 URL
///
/// 语义：
/// - 扫描 list_page 字段树，定位第一个 `field_type=Pagination` 的节点
/// - 在提取结果里找到对应 field_path，取第一个非空命中值
/// - 用 `resolve_url(base_url)` 绝对化后返回
/// - 无 pagination 字段 / 字段未命中 / 命中为空 → 返回 None（停止翻页）
///
/// 判定翻页是否应早停。返回 true 表示应停止翻页（由调用方 break）。
///
/// - `force_full`：true 时永远返回 false（强制全量，旁路早停），且不维护 `empty_pages`
/// - `new_before` / `new_after`：本页 detail 循环前后的 `new_count` 快照
/// - `empty_pages`：跨页累计的"连续零新增页数"，由本函数维护
/// - `limit`：连续多少页零新增触发早停（调用方常量 3）
pub fn should_stop_after_page(
    force_full: bool,
    new_before: i64,
    new_after: i64,
    empty_pages: &mut i64,
    limit: i64,
) -> bool {
    if force_full {
        return false;
    }
    if new_after - new_before == 0 {
        *empty_pages += 1;
        *empty_pages >= limit
    } else {
        *empty_pages = 0;
        false
    }
}

/// 045：URL 模板分页 —— 把含 `{page}` 占位符的模板替换为指定页码，并基于 base_url 绝对化
///
/// - 模板须含且仅含一个 `{page}`（否则返回 None，调用方据此判定模板无效/停止翻页）
/// - 替换后用 `resolve_url(base_url)` 绝对化（支持相对模板，如 `page-{page}.html`）
pub fn build_template_url(template: &str, page: i64, base_url: &str) -> Option<String> {
    if template.matches("{page}").count() != 1 {
        return None;
    }
    let filled = template.replace("{page}", &page.to_string());
    Some(resolve_url(&filled, base_url))
}

/// 045：模板模式下生成指定页码的列表页 URL；页码超过 page_end 上限或模板无效则返回 None
///
/// - `page` 为"将要生成"的页码（调用方负责递增）
/// - `page_end > 0` 时为页码上限；`page > page_end` → None（停止翻页）
/// - `base_url` 用于相对模板绝对化（通常传前页 final_url 或种子页 URL）
fn next_template_page(rt: &TaskRuntime, page: i64, base_url: &str) -> Option<String> {
    if rt.page_end > 0 && page > rt.page_end {
        return None;
    }
    build_template_url(&rt.page_url_template, page, base_url)
}

/// `scope_path` 为 list_page 的根路径（如 "/list_page"）
/// `base_url` 为当前页最终 URL（用于相对 URL → 绝对 URL）
fn find_next_page_url(
    extractions: &[FieldExtraction],
    tree_nodes: &[FieldTreeNode],
    scope_path: &str,
    base_url: &str,
) -> Option<String> {
    let type_by_path: HashMap<String, FieldType> = build_type_index(tree_nodes, scope_path);
    for ext in extractions {
        let is_pagination = type_by_path
            .get(&ext.field_path)
            .map(|t| matches!(t, FieldType::Pagination))
            .unwrap_or(false);
        if !is_pagination {
            continue;
        }
        for h in &ext.hits {
            let v = h.value.trim();
            if v.is_empty() {
                continue;
            }
            return Some(resolve_url(v, base_url));
        }
    }
    None
}

// ============================================================================
// 落库（US1 T036）
// ============================================================================

/// upsert 文章 + 写所有字段值
///
/// 返回 `Ok(true)` = 新建，`Ok(false)` = 已存在（跳过）
pub async fn upsert_article_with_fields(
    db: &DbPool,
    task_id: i64,
    source_type: &str,
    source_url: &str,
    canonical: &str,
    extractions: &[FieldExtraction],
) -> Result<bool, String> {
    let now = chrono::Utc::now().naive_utc();

    // 1. upsert crawler_articles（按 task_id + source_url_canonical 幂等）
    let (article_id, is_new) = match db {
        DbPool::Sqlite(pool) => {
            let existing: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM crawler_articles WHERE task_id = ? AND source_url_canonical = ?",
            )
            .bind(task_id)
            .bind(canonical)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询已有文章失败: {e}"))?;

            if let Some((id,)) = existing {
                // 已存在：除非 is_edited 否则更新 crawled_at + extra_fields_json
                let extra_json = build_extra_fields_json(extractions);
                sqlx::query(
                    "UPDATE crawler_articles \
                     SET crawled_at = ?, updated_at = ?, extra_fields_json = ? \
                     WHERE id = ? AND is_edited = 0",
                )
                .bind(now)
                .bind(now)
                .bind(&extra_json)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| format!("更新文章失败: {e}"))?;
                (id, false)
            } else {
                let title = find_first_value(extractions, "title")
                    .or_else(|| find_first_value(extractions, "name"));
                let content = find_first_value(extractions, "content");
                let extra_json = build_extra_fields_json(extractions);
                let result = sqlx::query(
                    "INSERT INTO crawler_articles \
                     (task_id, source_type, source_url, source_url_canonical, title, content, \
                      is_edited, crawled_at, created_at, updated_at, extra_fields_json) \
                     VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)",
                )
                .bind(task_id)
                .bind(source_type)
                .bind(source_url)
                .bind(canonical)
                .bind(&title)
                .bind(&content)
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(&extra_json)
                .execute(pool)
                .await
                .map_err(|e| format!("插入文章失败: {e}"))?;
                (result.last_insert_rowid(), true)
            }
        }
        DbPool::Postgres(pool) => {
            let existing: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM crawler_articles WHERE task_id = $1 AND source_url_canonical = $2",
            )
            .bind(task_id)
            .bind(canonical)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询已有文章失败: {e}"))?;

            if let Some((id,)) = existing {
                let extra_json = build_extra_fields_json(extractions);
                sqlx::query(
                    "UPDATE crawler_articles \
                     SET crawled_at = $1, updated_at = $2, extra_fields_json = $3 \
                     WHERE id = $4 AND is_edited = false",
                )
                .bind(now)
                .bind(now)
                .bind(&extra_json)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| format!("更新文章失败: {e}"))?;
                (id, false)
            } else {
                let title = find_first_value(extractions, "title")
                    .or_else(|| find_first_value(extractions, "name"));
                let content = find_first_value(extractions, "content");
                let extra_json = build_extra_fields_json(extractions);
                let new_id: (i64,) = sqlx::query_as(
                    "INSERT INTO crawler_articles \
                     (task_id, source_type, source_url, source_url_canonical, title, content, \
                      is_edited, crawled_at, created_at, updated_at, extra_fields_json) \
                     VALUES ($1, $2, $3, $4, $5, $6, false, $7, $8, $9, $10) \
                     RETURNING id",
                )
                .bind(task_id)
                .bind(source_type)
                .bind(source_url)
                .bind(canonical)
                .bind(&title)
                .bind(&content)
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(&extra_json)
                .fetch_one(pool)
                .await
                .map_err(|e| format!("插入文章失败: {e}"))?;
                (new_id.0, true)
            }
        }
    };

    // 2. 仅在新文章时写 field_values（避免重复刷屏）
    if is_new {
        write_field_values(db, article_id, extractions, now).await?;
    }

    Ok(is_new)
}

/// 写所有字段值到 crawler_article_field_values（含未命中行 is_hit=0）
async fn write_field_values(
    db: &DbPool,
    article_id: i64,
    extractions: &[FieldExtraction],
    now: chrono::NaiveDateTime,
) -> Result<(), String> {
    for ext in extractions {
        if ext.hits.is_empty() {
            // 未命中：写一行 is_hit=0
            match db {
                DbPool::Sqlite(pool) => {
                    let _ = sqlx::query(
                        "INSERT INTO crawler_article_field_values \
                         (article_id, field_node_id, field_path, scope, value_index, value_text, \
                          value_number, is_hit, created_at) \
                         VALUES (?, ?, ?, ?, 0, NULL, NULL, 0, ?)",
                    )
                    .bind(article_id)
                    .bind(ext.field_node_id)
                    .bind(&ext.field_path)
                    .bind(&ext.scope)
                    .bind(now)
                    .execute(pool)
                    .await;
                }
                DbPool::Postgres(pool) => {
                    let _ = sqlx::query(
                        "INSERT INTO crawler_article_field_values \
                         (article_id, field_node_id, field_path, scope, value_index, value_text, \
                          value_number, is_hit, created_at) \
                         VALUES ($1, $2, $3, $4, 0, NULL, NULL, false, $5)",
                    )
                    .bind(article_id)
                    .bind(ext.field_node_id)
                    .bind(&ext.field_path)
                    .bind(&ext.scope)
                    .bind(now)
                    .execute(pool)
                    .await;
                }
            }
            continue;
        }
        for (i, h) in ext.hits.iter().enumerate() {
            let value_number: Option<f64> = h.value.trim().parse::<f64>().ok();
            match db {
                DbPool::Sqlite(pool) => {
                    let _ = sqlx::query(
                        "INSERT INTO crawler_article_field_values \
                         (article_id, field_node_id, field_path, scope, value_index, value_text, \
                          value_number, is_hit, created_at) \
                         VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?)",
                    )
                    .bind(article_id)
                    .bind(ext.field_node_id)
                    .bind(&ext.field_path)
                    .bind(&ext.scope)
                    .bind(i as i32)
                    .bind(&h.value)
                    .bind(value_number)
                    .bind(now)
                    .execute(pool)
                    .await;
                }
                DbPool::Postgres(pool) => {
                    let _ = sqlx::query(
                        "INSERT INTO crawler_article_field_values \
                         (article_id, field_node_id, field_path, scope, value_index, value_text, \
                          value_number, is_hit, created_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, true, $8)",
                    )
                    .bind(article_id)
                    .bind(ext.field_node_id)
                    .bind(&ext.field_path)
                    .bind(&ext.scope)
                    .bind(i as i32)
                    .bind(&h.value)
                    .bind(value_number)
                    .bind(now)
                    .execute(pool)
                    .await;
                }
            }
        }
    }
    Ok(())
}

/// 把字段命中聚合为 extra_fields_json（简单单值字段 → {name: value}）
///
/// 仅收录每条字段的首条命中（多值字段建议消费 crawler_article_field_values 长表）
fn build_extra_fields_json(extractions: &[FieldExtraction]) -> Option<String> {
    let mut map = serde_json::Map::new();
    for ext in extractions {
        if ext.hits.is_empty() {
            continue;
        }
        // 取末段 name 作为 key（兼容嵌套字段路径）
        let key = ext
            .field_path
            .rsplit('/')
            .next()
            .unwrap_or(&ext.field_path)
            .to_string();
        // 多值时存数组
        let values: Vec<Value> = ext.hits.iter().map(|h| Value::String(h.value.clone())).collect();
        if values.len() == 1 {
            map.insert(key, values.into_iter().next().unwrap());
        } else {
            map.insert(key, Value::Array(values));
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map).to_string())
    }
}

/// 找出第一条 name 匹配的字段值（用于回填 article.title / content）
fn find_first_value(extractions: &[FieldExtraction], name: &str) -> Option<String> {
    for ext in extractions {
        let last = ext.field_path.rsplit('/').next().unwrap_or("");
        if last == name && !ext.hits.is_empty() {
            return Some(ext.hits[0].value.clone());
        }
    }
    None
}

/// 写入运行历史（best-effort，失败不阻塞返回）
#[allow(clippy::too_many_arguments)]
async fn insert_run_history(
    db: &DbPool,
    task_id: i64,
    task_name: &str,
    duration_ms: i64,
    status: &str,
    block_type: Option<String>,
    crawled: i64,
    new_count: i64,
    skipped: i64,
    failed: i64,
    error_message: Option<String>,
    // 045：finished_at — None = 运行中（留空）；Some = 已结束
    finished_at: Option<chrono::NaiveDateTime>,
) -> Result<i64, String> {
    let started_at = chrono::Utc::now().naive_utc() - chrono::Duration::milliseconds(duration_ms.max(0));
    let created_at = chrono::Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_run_histories \
                 (task_id, task_name, started_at, finished_at, duration_ms, status, block_type, \
                  crawled_count, new_count, skipped_count, failed_count, error_message, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(task_id)
            .bind(task_name)
            .bind(started_at)
            .bind(finished_at)
            .bind(duration_ms)
            .bind(status)
            .bind(&block_type)
            .bind(crawled)
            .bind(new_count)
            .bind(skipped)
            .bind(failed)
            .bind(&error_message)
            .bind(created_at)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(r.last_insert_rowid())
        }
        DbPool::Postgres(pool) => {
            let r = sqlx::query(
                "INSERT INTO crawler_run_histories \
                 (task_id, task_name, started_at, finished_at, duration_ms, status, block_type, \
                  crawled_count, new_count, skipped_count, failed_count, error_message, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
                 RETURNING id",
            )
            .bind(task_id)
            .bind(task_name)
            .bind(started_at)
            .bind(finished_at)
            .bind(duration_ms)
            .bind(status)
            .bind(&block_type)
            .bind(crawled)
            .bind(new_count)
            .bind(skipped)
            .bind(failed)
            .bind(&error_message)
            .bind(created_at)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
            let id_val: serde_json::Value = sqlx::Row::get(&r, "id");
            Ok(id_val.as_i64().unwrap_or(0))
        }
    }
}

/// 045：把开头插入的 running 行 update 成最终状态（status/duration/counts/error/finished_at）
async fn update_run_history(
    db: &DbPool,
    run_id: i64,
    duration_ms: i64,
    status: &str,
    block_type: Option<String>,
    crawled: i64,
    new_count: i64,
    skipped: i64,
    failed: i64,
    error_message: Option<String>,
) -> Result<(), String> {
    if run_id <= 0 {
        return Ok(()); // 开头插入失败（run_id=0）则无可更新，等价于旧行为
    }
    let finished_at = chrono::Utc::now().naive_utc();
    match db {
        DbPool::Sqlite(pool) => {
            sqlx::query(
                "UPDATE crawler_run_histories \
                 SET finished_at = ?, duration_ms = ?, status = ?, block_type = ?, \
                     crawled_count = ?, new_count = ?, skipped_count = ?, failed_count = ?, error_message = ? \
                 WHERE id = ?",
            )
            .bind(finished_at)
            .bind(duration_ms)
            .bind(status)
            .bind(&block_type)
            .bind(crawled)
            .bind(new_count)
            .bind(skipped)
            .bind(failed)
            .bind(&error_message)
            .bind(run_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        DbPool::Postgres(pool) => {
            sqlx::query(
                "UPDATE crawler_run_histories \
                 SET finished_at = $1, duration_ms = $2, status = $3, block_type = $4, \
                     crawled_count = $5, new_count = $6, skipped_count = $7, failed_count = $8, error_message = $9 \
                 WHERE id = $10",
            )
            .bind(finished_at)
            .bind(duration_ms)
            .bind(status)
            .bind(&block_type)
            .bind(crawled)
            .bind(new_count)
            .bind(skipped)
            .bind(failed)
            .bind(&error_message)
            .bind(run_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 更新任务的 next_run_at + last_run_at + 连续失败计数
async fn update_task_schedule(db: &DbPool, task_id: i64, status: &str) -> Result<(), String> {
    let now = chrono::Utc::now().naive_utc();
    let is_fail = status == "blocked" || status == "failed";
    let r = match db {
        DbPool::Sqlite(pool) => {
            if is_fail {
                sqlx::query(
                    "UPDATE crawler_tasks \
                     SET last_run_at = ?, \
                         consecutive_failures = consecutive_failures + 1, \
                         status = CASE \
                             WHEN consecutive_failures + 1 >= max_consecutive_failures \
                             THEN 'auto_blocked' ELSE status END, \
                         updated_at = ? \
                     WHERE id = ?",
                )
                .bind(now)
                .bind(now)
                .bind(task_id)
                .execute(pool)
                .await
            } else {
                sqlx::query(
                    "UPDATE crawler_tasks \
                     SET last_run_at = ?, consecutive_failures = 0, updated_at = ? \
                     WHERE id = ?",
                )
                .bind(now)
                .bind(now)
                .bind(task_id)
                .execute(pool)
                .await
            }
            .map(|_| ())
        }
        DbPool::Postgres(pool) => {
            if is_fail {
                sqlx::query(
                    "UPDATE crawler_tasks \
                     SET last_run_at = $1, \
                         consecutive_failures = consecutive_failures + 1, \
                         status = CASE \
                             WHEN consecutive_failures + 1 >= max_consecutive_failures \
                             THEN 'auto_blocked' ELSE status END, \
                         updated_at = $2 \
                     WHERE id = $3",
                )
                .bind(now)
                .bind(now)
                .bind(task_id)
                .execute(pool)
                .await
            } else {
                sqlx::query(
                    "UPDATE crawler_tasks \
                     SET last_run_at = $1, consecutive_failures = 0, updated_at = $2 \
                     WHERE id = $3",
                )
                .bind(now)
                .bind(now)
                .bind(task_id)
                .execute(pool)
                .await
            }
            .map(|_| ())
        }
    };
    r.map_err(|e| format!("更新调度时间失败: {e}"))?;
    Ok(())
}

// ============================================================================
// 测试运行（不落库，US1 T035）
// ============================================================================

/// 测试运行：抓第一条 list_url → 应用 list_page 字段 → 取前 N 条详情预览
pub async fn test_run(
    db: &DbPool,
    task_id: i64,
    limit: usize,
) -> Result<CrawlerTestPreview, String> {
    let rt = load_task(db, task_id).await?;
    let list_url = rt
        .list_urls
        .first()
        .ok_or_else(|| "任务无 list_urls".to_string())?
        .clone();

    let sys_proxy = futures::future::ready::<Option<String>>(None).await;
    let proxy = rt.proxy.clone().or(sys_proxy);

    let material = fetch_source_material(&list_url, rt.user_agent.as_deref(), proxy.as_deref())
        .await
        .map_err(|e| format!("抓取列表页失败: {e}"))?;

    let list_extractions = extract_layer(
        &rt.field_tree.list_page,
        &material,
        &[],
        Scope::ListPage.as_str(),
        &format!("/{}", Scope::ListPage.as_str()),
    );

    let mut detail_seen = std::collections::HashSet::<String>::new();
    let detail_links = collect_detail_links(
        &list_extractions,
        &rt.field_tree.list_page,
        &format!("/{}", Scope::ListPage.as_str()),
        &material.final_url,
        &mut detail_seen,
    );

    let mut articles = Vec::new();
    let mut warnings = Vec::new();
    for (i, ext) in list_extractions.iter().enumerate() {
        if let Some(err) = &ext.error {
            warnings.push(format!("字段 {} 提取失败: {err}", ext.field_path));
        }
        if i >= limit {
            break;
        }
    }

    let preview_count = detail_links.len().min(limit);
    for url in detail_links.iter().take(limit) {
        // 抓详情素材（best-effort，失败也尽量返回）
        let dm = fetch_source_material(url, rt.user_agent.as_deref(), proxy.as_deref()).await;
        let detail_extractions = match dm {
            Ok(m) => extract_layer(
                &rt.field_tree.detail_page,
                &m,
                &[],
                Scope::DetailPage.as_str(),
                &format!("/{}", Scope::DetailPage.as_str()),
            ),
            Err(e) => {
                warnings.push(format!("详情 {url} 抓取失败: {}", e.message));
                continue;
            }
        };

        let title = find_first_value(&detail_extractions, "title")
            .or_else(|| find_first_value(&detail_extractions, "name"));
        let content = find_first_value(&detail_extractions, "content");
        let content_snippet = content.map(|c| c.chars().take(200).collect::<String>());

        articles.push(TestPreviewArticle {
            source_url: url.clone(),
            title,
            content_snippet,
            pan_links: Vec::new(),
            direct_links: Vec::new(),
            images: Vec::new(),
            field_warnings: warnings.clone(),
        });
    }

    let list_item_ok = list_extractions.iter().any(|e| !e.hits.is_empty());
    let detail_link_ok = !detail_links.is_empty();

    Ok(CrawlerTestPreview {
        list_count: detail_links.len() as i64,
        preview_count: preview_count as i64,
        articles,
        selector_validation: SelectorValidation {
            list_item_ok,
            detail_link_ok,
            missing_fields: Vec::new(),
        },
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_url_absolute_passthrough() {
        assert_eq!(
            resolve_url("https://example.com/a", "https://other.com/"),
            "https://example.com/a"
        );
    }

    #[test]
    fn resolve_url_root_relative() {
        assert_eq!(
            resolve_url("/path/page", "https://example.com/dir/base"),
            "https://example.com/path/page"
        );
    }

    #[test]
    fn resolve_url_relative() {
        assert_eq!(
            resolve_url("page.html", "https://example.com/dir/base"),
            "https://example.com/dir/page.html"
        );
    }

    #[test]
    fn resolve_url_no_path_in_base() {
        assert_eq!(
            resolve_url("foo", "https://example.com"),
            "https://example.com/foo"
        );
    }

    #[test]
    fn build_reqwest_client_default_ua() {
        let c = build_reqwest_client(None, None);
        assert!(c.is_ok());
    }

    #[test]
    fn build_no_redirect_client_ok() {
        let c = build_no_redirect_client(None, None);
        assert!(c.is_ok());
    }

    #[test]
    fn extract_pagination_urls_extracts_hrefs() {
        let html = r#"<div><a class="pg" href="/p/2">2</a><a class="pg" href="/p/3">3</a></div>"#;
        let urls = extract_pagination_urls(html, ".pg");
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"/p/2".to_string()));
        assert!(urls.contains(&"/p/3".to_string()));
    }

    #[test]
    fn extract_pagination_urls_empty_selector_returns_empty() {
        let urls = extract_pagination_urls("<div>x</div>", "");
        assert!(urls.is_empty());
    }

    /// build_extra_fields_json 单值聚合
    #[test]
    fn extra_fields_json_single_value() {
        let ext = vec![FieldExtraction {
            field_path: "/detail_page/title".into(),
            field_node_id: Some(1),
            scope: "detail_page".into(),
            hits: vec![Hit {
                value: "hello".into(),
                source_fragment: "css".into(),
                location: None,
                context_html: None,
            }],
            error: None,
        }];
        let json = build_extra_fields_json(&ext).expect("Some");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["title"], Value::String("hello".into()));
    }

    /// build_extra_fields_json 多值聚合为数组
    #[test]
    fn extra_fields_json_multi_value() {
        let ext = vec![FieldExtraction {
            field_path: "/detail_page/tags".into(),
            field_node_id: Some(1),
            scope: "detail_page".into(),
            hits: vec![
                Hit { value: "a".into(), source_fragment: "css".into(), location: None, context_html: None },
                Hit { value: "b".into(), source_fragment: "css".into(), location: None, context_html: None },
            ],
            error: None,
        }];
        let json = build_extra_fields_json(&ext).expect("Some");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["tags"], Value::Array(vec![Value::String("a".into()), Value::String("b".into())]));
    }

    /// 未命中字段不出现在 extra_fields_json
    #[test]
    fn extra_fields_json_skips_empty_hits() {
        let ext = vec![
            FieldExtraction {
                field_path: "/detail_page/title".into(),
                field_node_id: Some(1),
                scope: "detail_page".into(),
                hits: Vec::new(),
                error: None,
            },
            FieldExtraction {
                field_path: "/detail_page/url".into(),
                field_node_id: Some(2),
                scope: "detail_page".into(),
                hits: vec![Hit {
                    value: "https://example.com".into(),
                    source_fragment: "css".into(),
                    location: None,
                    context_html: None,
                }],
                error: None,
            },
        ];
        let json = build_extra_fields_json(&ext).expect("Some");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("title").is_none());
        assert_eq!(v["url"], Value::String("https://example.com".into()));
    }

    /// find_first_value 按 name 匹配
    #[test]
    fn find_first_value_matches() {
        let ext = vec![FieldExtraction {
            field_path: "/detail_page/title".into(),
            field_node_id: Some(1),
            scope: "detail_page".into(),
            hits: vec![Hit {
                value: "T".into(),
                source_fragment: "css".into(),
                location: None,
                context_html: None,
            }],
            error: None,
        }];
        assert_eq!(find_first_value(&ext, "title"), Some("T".to_string()));
        assert_eq!(find_first_value(&ext, "name"), None);
    }

    /// 简单字段树（list_page + detail_page）能加载 + 提取 — 单元层测试
    /// 完整集成测试见 tests/ 目录（test_run 网络依赖）
    #[test]
    fn build_type_index_distinguishes_link_card() {
        use crate::models::crawler_field_node::FieldNodeRow;
        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let mk = |id: i64, parent: Option<i64>, name: &str, ft: &str| FieldNodeRow {
            id, task_id: 1, parent_id: parent, scope: "list_page".into(), name: name.into(),
            display_name: name.into(), field_type: ft.into(), source_layer: "html".into(),
            extractor_mode: "css".into(), rule_json: r#"{"selector":".x"}"#.into(),
            post_processors_json: None, script_index: None, sort_order: 0, is_active: true,
            created_at: ts, updated_at: ts,
        };
        let rows = vec![
            mk(1, None, "link_card", "link_card"),
            mk(2, Some(1), "title", "string"),
            mk(3, None, "url", "url"),
            mk(4, None, "title", "string"),
        ];
        let tree = crate::models::crawler_field_node::from_rows(rows);
        let index = build_type_index(&tree.list_page, "/list_page");
        assert!(matches!(
            index.get("/list_page/link_card"),
            Some(FieldType::LinkCard)
        ));
        assert!(matches!(
            index.get("/list_page/url"),
            Some(FieldType::Url)
        ));
        assert!(matches!(
            index.get("/list_page/title"),
            Some(FieldType::String)
        ));
    }

    /// 044：should_stop_after_page —— 全量模式永远不早停
    #[test]
    fn should_stop_after_page_force_full_never_stops() {
        let mut empty = 10i64; // 即便已累计 10 页空，全量模式也不停
        assert!(!should_stop_after_page(true, 5, 5, &mut empty, 3));
        assert_eq!(empty, 10, "force_full=true 时不得维护 empty_pages");
    }

    /// 044：本页有新增 → 清零，不停
    #[test]
    fn should_stop_after_page_resets_on_new() {
        let mut empty = 2i64;
        assert!(!should_stop_after_page(false, 5, 8, &mut empty, 3));
        assert_eq!(empty, 0);
    }

    /// 044：本页零新增、未达阈值 → 累加，不停
    #[test]
    fn should_stop_after_page_accumulates_below_limit() {
        let mut empty = 0i64;
        assert!(!should_stop_after_page(false, 5, 5, &mut empty, 3));
        assert_eq!(empty, 1);
        assert!(!should_stop_after_page(false, 5, 5, &mut empty, 3));
        assert_eq!(empty, 2);
    }

    /// 044：本页零新增、达阈值 → 停
    #[test]
    fn should_stop_after_page_stops_at_limit() {
        let mut empty = 2i64;
        assert!(should_stop_after_page(false, 5, 5, &mut empty, 3));
        assert_eq!(empty, 3);
    }

    /// 044：翻页序列 [新5, 旧0, 旧0, 旧0] → 第 4 次停；中间出新帖清零
    #[test]
    fn should_stop_after_page_simulates_pagination_sequence() {
        let mut empty = 0i64;
        // 第1页：5 新
        assert!(!should_stop_after_page(false, 0, 5, &mut empty, 3));
        // 第2-4页：零新增
        assert!(!should_stop_after_page(false, 5, 5, &mut empty, 3)); // empty=1
        assert!(!should_stop_after_page(false, 5, 5, &mut empty, 3)); // empty=2
        assert!(should_stop_after_page(false, 5, 5, &mut empty, 3));  // empty=3 → 停
        // 重置后：[旧0, 新1, 旧0, 旧0, 旧0] → 中间清零，第5次才停
        let mut empty2 = 0i64;
        assert!(!should_stop_after_page(false, 5, 5, &mut empty2, 3)); // empty=1
        assert!(!should_stop_after_page(false, 5, 6, &mut empty2, 3)); // 新增 → empty=0
        assert!(!should_stop_after_page(false, 6, 6, &mut empty2, 3)); // empty=1
        assert!(!should_stop_after_page(false, 6, 6, &mut empty2, 3)); // empty=2
        assert!(should_stop_after_page(false, 6, 6, &mut empty2, 3));  // empty=3 → 停
    }

    /// 045：build_template_url —— {page} 占位符替换 + 相对 URL 绝对化
    #[test]
    fn build_template_url_replaces_placeholder() {
        let u = build_template_url("https://site.com/page-{page}.html", 4, "https://site.com/");
        assert_eq!(u.as_deref(), Some("https://site.com/page-4.html"));
    }

    #[test]
    fn build_template_url_relative_resolves() {
        let u = build_template_url("page-{page}.html", 2, "https://site.com/list/");
        assert_eq!(u.as_deref(), Some("https://site.com/list/page-2.html"));
    }

    #[test]
    fn build_template_url_page_in_query() {
        let u = build_template_url("https://site.com/list?p={page}", 7, "https://site.com/");
        assert_eq!(u.as_deref(), Some("https://site.com/list?p=7"));
    }

    #[test]
    fn build_template_url_no_placeholder_returns_none() {
        assert_eq!(
            build_template_url("https://site.com/page-4.html", 4, "https://site.com/"),
            None,
            "无 {{page}} 占位符应返回 None"
        );
    }

    #[test]
    fn build_template_url_multiple_placeholders_returns_none() {
        assert_eq!(
            build_template_url("{page}/{page}", 1, "https://site.com/"),
            None,
            "多个 {{page}} 占位符应返回 None"
        );
    }

    /// US5 T055：find_next_page_url 从 pagination 字段命中取下一页 URL
    #[test]
    fn find_next_page_url_returns_first_pagination_hit() {
        use crate::models::crawler_field_node::FieldNodeRow;
        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let mk = |id: i64, parent: Option<i64>, name: &str, ft: &str| FieldNodeRow {
            id, task_id: 1, parent_id: parent, scope: "list_page".into(), name: name.into(),
            display_name: name.into(), field_type: ft.into(), source_layer: "html".into(),
            extractor_mode: "css".into(), rule_json: r#"{"selector":".x"}"#.into(),
            post_processors_json: None, script_index: None, sort_order: 0, is_active: true,
            created_at: ts, updated_at: ts,
        };
        let rows = vec![
            mk(1, None, "next_page", "pagination"),
            mk(2, None, "title", "string"),
        ];
        let tree = crate::models::crawler_field_node::from_rows(rows);
        let extractions = vec![
            FieldExtraction {
                field_path: "/list_page/title".into(),
                field_node_id: Some(2),
                scope: "list_page".into(),
                hits: vec![Hit {
                    value: "页标题".into(),
                    source_fragment: "css:.title".into(),
                    location: Some("node[0]".into()),
                    context_html: None,
                }],
                error: None,
            },
            FieldExtraction {
                field_path: "/list_page/next_page".into(),
                field_node_id: Some(1),
                scope: "list_page".into(),
                hits: vec![Hit {
                    value: "/list?page=2".into(),
                    source_fragment: "css:.next".into(),
                    location: Some("node[0]".into()),
                    context_html: None,
                }],
                error: None,
            },
        ];
        let next = find_next_page_url(&extractions, &tree.list_page, "/list_page", "https://example.com/list?page=1");
        assert_eq!(next.as_deref(), Some("https://example.com/list?page=2"));
    }

    /// US5 T055：无 pagination 字段或字段未命中 → 返回 None
    #[test]
    fn find_next_page_url_none_when_no_pagination_field() {
        use crate::models::crawler_field_node::FieldNodeRow;
        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let mk = |id: i64, name: &str, ft: &str| FieldNodeRow {
            id, task_id: 1, parent_id: None, scope: "list_page".into(), name: name.into(),
            display_name: name.into(), field_type: ft.into(), source_layer: "html".into(),
            extractor_mode: "css".into(), rule_json: r#"{"selector":".x"}"#.into(),
            post_processors_json: None, script_index: None, sort_order: 0, is_active: true,
            created_at: ts, updated_at: ts,
        };
        // 无 pagination 字段
        let tree = crate::models::crawler_field_node::from_rows(vec![mk(1, "title", "string")]);
        let extractions = vec![FieldExtraction {
            field_path: "/list_page/title".into(),
            field_node_id: Some(1),
            scope: "list_page".into(),
            hits: vec![Hit {
                value: "x".into(),
                source_fragment: "css:.title".into(),
                location: None,
                context_html: None,
            }],
            error: None,
        }];
        assert_eq!(
            find_next_page_url(&extractions, &tree.list_page, "/list_page", "https://example.com/"),
            None
        );

        // 有 pagination 字段但未命中
        let tree2 = crate::models::crawler_field_node::from_rows(vec![
            mk(1, "title", "string"),
            mk(2, "next_page", "pagination"),
        ]);
        let extractions2 = vec![FieldExtraction {
            field_path: "/list_page/next_page".into(),
            field_node_id: Some(2),
            scope: "list_page".into(),
            hits: vec![],
            error: None,
        }];
        assert_eq!(
            find_next_page_url(&extractions2, &tree2.list_page, "/list_page", "https://example.com/"),
            None
        );
    }

    /// **集成测试 field_tree_crawl_two_stage**（US1 T035 验收）
    ///
    /// 验证：加载 FieldTree → 列表页阶段提取 link_card + 子字段（cover）→
    ///      collect_detail_links 找出详情链接 → 详情页阶段对详情素材提取 title/content →
    ///      build_extra_fields_json 聚合为 JSON。
    ///
    /// 不触发网络请求：用静态 HTML fixture 直接构造 SourceMaterial。
    #[test]
    fn field_tree_crawl_two_stage() {
        use crate::services::crawler::source_layer::{
            MetaTag, ScriptBlock, SourceMaterial,
        };

        // ---- 列表页素材（手搓 HTML：3 条 .list-item，每条 a.link 内嵌 img.cover） ----
        let list_html = r#"<!DOCTYPE html><html><body>
          <div class="list">
            <div class="list-item">
              <a class="link" href="/p/1"><img class="cover" src="/c1.jpg" /></a>
            </div>
            <div class="list-item">
              <a class="link" href="/p/2"><img class="cover" src="/c2.jpg" /></a>
            </div>
            <div class="list-item">
              <a class="link" href="/p/3"><img class="cover" src="/c3.jpg" /></a>
            </div>
          </div>
        </body></html>"#;
        let list_material = SourceMaterial {
            final_url: "https://example.com/list".into(),
            status: 200,
            headers: HashMap::new(),
            html: list_html.into(),
            scripts: Vec::<ScriptBlock>::new(),
            metas: Vec::<MetaTag>::new(),
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 50,
        };

        // ---- 详情页素材（mock 单条） ----
        let detail_html = r#"<!DOCTYPE html><html><head><title>Hello Post</title></head>
          <body><article><h1 class="title">Hello Post</h1>
            <div class="content"><p>Body text here.</p></div>
          </article></body></html>"#;
        let detail_material = SourceMaterial {
            final_url: "https://example.com/p/1".into(),
            status: 200,
            headers: HashMap::new(),
            html: detail_html.into(),
            scripts: Vec::<ScriptBlock>::new(),
            metas: Vec::<MetaTag>::new(),
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 30,
        };

        // ---- 字段树：list_page 含 link_card + 子 cover；detail_page 含 title + content ----
        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let mk = |id: i64, parent: Option<i64>, scope: &str, name: &str, ft: &str, layer: &str, mode: &str, rule: &str| FieldNodeRow {
            id, task_id: 1, parent_id: parent, scope: scope.into(), name: name.into(),
            display_name: name.into(), field_type: ft.into(), source_layer: layer.into(),
            extractor_mode: mode.into(), rule_json: rule.into(),
            post_processors_json: None, script_index: None, sort_order: 0, is_active: true,
            created_at: ts, updated_at: ts,
        };
        let rows = vec![
            mk(1, None, "list_page", "link_card", "link_card", "html", "css",
               r#"{"selector":".list-item a.link","attr":"href"}"#),
            mk(2, Some(1), "list_page", "cover", "image", "html", "css",
               r#"{"selector":"img.cover","attr":"src"}"#),
            mk(3, None, "detail_page", "title", "string", "html", "css",
               r#"{"selector":"h1.title","attr":"text"}"#),
            mk(4, None, "detail_page", "content", "text", "html", "css",
               r#"{"selector":".content","attr":"text"}"#),
        ];
        let tree = crate::models::crawler_field_node::from_rows(rows);

        // ---- 第一阶段：列表页提取 ----
        let list_extractions = extract_layer(
            &tree.list_page,
            &list_material,
            &[],
            Scope::ListPage.as_str(),
            "/list_page",
        );
        // link_card 应命中 3 条
        let link_ext = list_extractions.iter().find(|e| e.field_path.ends_with("/link_card")).unwrap();
        assert_eq!(link_ext.hits.len(), 3, "link_card 应命中 3 条 detail 链接");

        // cover 在父命中的 3 个 HTML 片段上提取（每个片段 1 张图）→ 3 条命中
        let cover_ext = list_extractions.iter().find(|e| e.field_path.ends_with("/cover")).unwrap();
        assert_eq!(cover_ext.hits.len(), 3, "cover 应在每条父命中上各命中 1 次");

        // ---- collect_detail_links 应返回 3 条绝对 URL ----
        let mut seen = std::collections::HashSet::<String>::new();
        let detail_links = collect_detail_links(
            &list_extractions,
            &tree.list_page,
            &format!("/{}", Scope::ListPage.as_str()),
            &list_material.final_url,
            &mut seen,
        );
        assert_eq!(detail_links.len(), 3, "应收集 3 条 detail 链接");
        assert!(detail_links.iter().all(|u| u.starts_with("https://example.com/p/")));

        // ---- 第二阶段：详情页提取（用 mock 的 detail_material） ----
        let detail_extractions = extract_layer(
            &tree.detail_page,
            &detail_material,
            &[],
            Scope::DetailPage.as_str(),
            "/detail_page",
        );
        let title = find_first_value(&detail_extractions, "title").expect("title 必命中");
        assert_eq!(title, "Hello Post");
        let content = find_first_value(&detail_extractions, "content").expect("content 必命中");
        assert!(content.contains("Body text"));

        // ---- 聚合 extra_fields_json ----
        let json_str = build_extra_fields_json(&detail_extractions).expect("Some");
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["title"], Value::String("Hello Post".into()));
        assert!(v["content"].as_str().unwrap().contains("Body text"));

        // ---- extra_fields_json 在 list_page 阶段也聚合（多值 → 数组） ----
        let list_json = build_extra_fields_json(&list_extractions).expect("Some");
        let lv: Value = serde_json::from_str(&list_json).unwrap();
        assert_eq!(lv["link_card"], Value::Array(vec![
            Value::String("https://example.com/p/1".into()),
            Value::String("https://example.com/p/2".into()),
            Value::String("https://example.com/p/3".into()),
        ]));
        assert_eq!(lv["cover"].as_array().unwrap().len(), 3);
    }

    /// `collect_detail_links` 兜底过滤：LinkCard 字段配 attr=html 时命中 outerHTML
    /// 字符串（如 `<a class="..." href="/x">...</a>`），不应作为详情 URL 入库。
    /// 同理含内部空白的值（attr=text 抓到「点击下载」之类）也跳过。
    #[test]
    fn collect_detail_links_filters_html_hits() {
        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let mk = |id: i64, ft: &str, rule: &str| FieldNodeRow {
            id, task_id: 1, parent_id: None, scope: "list_page".into(),
            name: format!("f{id}"), display_name: format!("f{id}"),
            field_type: ft.into(), source_layer: "html".into(),
            extractor_mode: "css".into(), rule_json: rule.into(),
            post_processors_json: None, script_index: None, sort_order: 0, is_active: true,
            created_at: ts, updated_at: ts,
        };
        let rows = vec![
            // link_card 字段配 attr=html（典型误配：抓整个 <a> 元素 outerHTML）
            mk(1, "link_card", r#"{"selector":"a.card","attr":"html"}"#),
            // url 字段配 attr=href（正常：抓 href）
            mk(2, "url", r#"{"selector":"a.card","attr":"href"}"#),
        ];
        let tree = crate::models::crawler_field_node::from_rows(rows);

        let extractions = vec![
            FieldExtraction {
                field_path: "/list_page/f1".into(),
                field_node_id: None,
                scope: "list_page".into(),
                error: None,
                hits: vec![
                    crate::services::crawler::extractor::Hit {
                        value: r#"<a class="card" href="/p/1"><span>title</span></a>"#.into(),
                        source_fragment: "css:a.card".into(),
                        location: None,
                        context_html: None,
                    },
                    crate::services::crawler::extractor::Hit {
                        value: r#"<a class="card" href="/p/2">x</a>"#.into(),
                        source_fragment: "css:a.card".into(),
                        location: None,
                        context_html: None,
                    },
                ],
            },
            FieldExtraction {
                field_path: "/list_page/f2".into(),
                field_node_id: None,
                scope: "list_page".into(),
                error: None,
                hits: vec![
                    crate::services::crawler::extractor::Hit {
                        value: "/p/1".into(),
                        source_fragment: "css:a.card".into(),
                        location: None,
                        context_html: None,
                    },
                    crate::services::crawler::extractor::Hit {
                        value: "/p/2".into(),
                        source_fragment: "css:a.card".into(),
                        location: None,
                        context_html: None,
                    },
                    // 含内部空白的非 URL 值（attr=text 命中场景）
                    crate::services::crawler::extractor::Hit {
                        value: "点击 下载".into(),
                        source_fragment: "css:a.card".into(),
                        location: None,
                        context_html: None,
                    },
                ],
            },
        ];

        let mut seen = std::collections::HashSet::<String>::new();
        let links = collect_detail_links(
            &extractions,
            &tree.list_page,
            "/list_page",
            "https://example.com/list",
            &mut seen,
        );

        // 应只收集 2 条合法 URL（/p/1, /p/2），跳过 outerHTML 与含空白值
        assert_eq!(links.len(), 2, "应过滤掉 HTML 命中和含空白值，只剩 2 条合法 URL");
        assert!(links.iter().all(|u| u.starts_with("https://example.com/p/")));
    }

    /// **未命中字段不出错**：CSS 选择器不匹配任何元素时返回空 hits + 不抛错（FR-019 单字段失败不中断）
    #[test]
    fn field_tree_unhit_does_not_propagate_error() {
        use crate::services::crawler::source_layer::{MetaTag, ScriptBlock, SourceMaterial};

        let html = r#"<html><body><div>no matches</div></body></html>"#;
        let material = SourceMaterial {
            final_url: "https://example.com/x".into(),
            status: 200,
            headers: HashMap::new(),
            html: html.into(),
            scripts: Vec::<ScriptBlock>::new(),
            metas: Vec::<MetaTag>::new(),
            fetched_at: chrono::Utc::now().naive_utc(),
            duration_ms: 5,
        };

        let ts = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc();
        let row = FieldNodeRow {
            id: 1, task_id: 1, parent_id: None, scope: "list_page".into(),
            name: "title".into(), display_name: "title".into(),
            field_type: "string".into(), source_layer: "html".into(),
            extractor_mode: "css".into(),
            rule_json: r#"{"selector":".missing","attr":"text"}"#.into(),
            post_processors_json: None, script_index: None, sort_order: 0,
            is_active: true, created_at: ts, updated_at: ts,
        };
        let tree = crate::models::crawler_field_node::from_rows(vec![row]);

        let ext = extract_layer(
            &tree.list_page,
            &material,
            &[],
            Scope::ListPage.as_str(),
            "/list_page",
        );
        assert_eq!(ext.len(), 1);
        assert!(ext[0].hits.is_empty(), "未命中字段应为空 hits");
        assert!(ext[0].error.is_none(), "未命中（CSS 找不到元素）不应被记为 error");
    }
}
