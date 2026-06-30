// API common types
export interface ApiResponse<T = unknown> {
  success: boolean
  message?: string
  data?: T
}

export interface PaginatedData<T> {
  list: T[]
  pagination: {
    page: number
    page_size: number
    total: number
  }
}

export interface PaginatedResponse<T> extends ApiResponse<PaginatedData<T>> {}

// User types
export interface User {
  id: number
  username: string
  display_name?: string
  email?: string
  role: number
  status: number
  access_token?: string
  created_at: string
}

export interface LoginForm {
  username: string
  password: string
  captcha_key?: string
  captcha_code?: string
}

export interface RegisterForm {
  username: string
  password: string
  email?: string
}

// Client (Telegram) types
export interface Client {
  id: string
  user_id: number
  client_type: 'Client' | 'Bot'
  phone?: string
  token?: string
  name?: string
  username?: string
  status: 'new' | 'active' | 'wait_code' | 'wait_password' | 'offline'
  session_path?: string
  created_at: string
  updated_at: string
}

export interface ClientAuthRequest {
  type: 'code' | 'password'
  value: string
}

export interface Chat {
  id: number
  name: string
  type: string
}

// Rule types
export interface Rule {
  id: number
  user_id: number
  source_chat_id: number
  source_chat_name?: string
  forward_method: 'Chat' | 'Webhook'
  forward_config?: string
  forward_target?: string
  is_active: boolean
  remark?: string
  forward_client_id?: string
  filter_mode?: 'none' | 'include' | 'exclude'
  keywords?: string
  media_filter?: 'all' | 'photo' | 'document' | 'text'
  source_client_id?: string
  created_at: string
  updated_at: string
}

// Collector types
export interface Collector {
  id: number
  user_id: number
  client_id?: string
  channel_id: number
  channel_name?: string
  collector_type: string
  is_active: boolean
  remark?: string
  created_at: string
  updated_at: string
  total_messages: number
  today_messages: number
  unextracted_messages: number
}

export interface CollectorHistory {
  id: number
  collector_id: number
  channel_id: number
  message_id: number
  post_time?: string
  raw_data?: string
  is_auto_push: boolean
  remote_id?: string
  created_at: string
}

// Message types
export interface Message {
  id: number
  rule_id: number
  chat_id?: number
  message_id?: number
  content?: string
  raw_data?: string
  status: 'pending' | 'success' | 'failed'
  error_reason?: string
  created_at: string
}

// Push types
export interface PushHistory {
  id: number
  batch_id: string
  target?: string
  status: 'success' | 'failed'
  data_count: number
  message?: string
  error_msg?: string
  pushed_at: string
  pushed_count?: number
  skipped_image_count?: number
  skipped_link_count?: number
}

// 推送跳过明细（资源链接/图片转存未通过）
export interface PushSkipRecord {
  resource_id: number
  title?: string
  skip_reason: 'image_not_forwarded' | 'link_invalid'
  urls_invalid?: string | null
  detail?: string | null
}

// 推送历史详情（含跳过明细）
export interface PushHistoryDetail {
  history: PushHistory
  skip_records: PushSkipRecord[]
}

// Push config — 多推送配置
export interface PushConfig {
  id: number
  name: string
  api_url: string
  api_token?: string
  target: string
  auth_type: 'none' | 'bearer' | 'custom_header' | 'query'
  auth_key: string
  http_method: 'POST' | 'PUT' | 'PATCH'
  body_template?: string
  custom_headers: string
  batch_size: number
  data_source_type: 'all' | 'selected'
  collector_ids?: number[]
  collector_count: number
  auto_push: boolean
  push_interval: number
  link_check_before_push: boolean
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface PushSchedulerConfig {
  auto_push: boolean
  interval_minutes: number
  api_url: string
  api_token: string
  target: string
  batch_size: number
  // 通用推送配置
  auth_type: 'none' | 'bearer' | 'custom_header' | 'query'
  auth_key: string
  http_method: 'POST' | 'PUT' | 'PATCH'
  body_template: string
  custom_headers: Array<{ key: string; value: string }>
}

// File types
export interface FileItem {
  id: number
  filename: string
  uploader_id: number
  link?: string
  created_at: string
}

// Option types
export interface Option {
  id: number
  key: string
  value?: string
}

// AI Endpoint types
export interface AiEndpoint {
  id: string
  name: string
  ai_type: 'openai' | 'nvidia' | 'zhipu'
  url: string
  key: string
  model: string
  enable: boolean
  request_delay: number
}

// Resource types
export interface ExtractedResource {
  id: number
  collector_history_id: number
  title: string
  url?: string
  description?: string
  category?: string
  tags?: string
  img?: string
  source: string
  extra?: string
  extract_mode: 'rule' | 'ai'
  is_pushed: boolean
  is_edited: boolean
  img_forward_status?: string | null  // 'pending' | 'forwarded' | 'failed' | null
  image_message_id?: number | null    // 图床群组A 的消息ID（阶段1 完成后写入）
  file_id?: string | null              // Bot 二次转发获取的图片 file_id（阶段2 完成后写入）
  link_status?: string | null  // 'valid' | 'invalid' | 'unknown'（链接有效性检测聚合状态）
  created_at: string
  updated_at: string
}

export interface ResourceStats {
  total: number
  pushed: number
  unpushed: number
  by_category: Record<string, number>
  by_extract_mode: Record<string, number>
}

export interface ExtractConfig {
  extract_mode: 'rule' | 'ai'
  auto_extract: boolean
  extract_interval: number
  ai_endpoints: string
  ai_prompt: string
}

export interface ExtractionResult {
  total_scanned: number
  extracted: number
  skipped: number
  errors: number
}

// Resource draft view (single extraction result)
export interface ResourceDraftView {
  title: string
  url: string[]
  description: string
  category: string
  tags: string
  source: string
}

export interface SingleExtractResponse {
  resources: ResourceDraftView[]
  extract_mode: string
}

// Resource detail (view extraction comparison)
export interface ResourceDetailResponse {
  resource: ExtractedResource
  raw_text?: string
  raw_data?: string
  media_type?: string
  has_history: boolean
  channel_name?: string
}

// System status
export interface SystemStatus {
  version: string
  uptime: number
  clients: {
    total: number
    active: number
  }
}

// Extract history (scheduler dashboard)
export interface ExtractHistory {
  id: number
  status: 'success' | 'failed'
  total_scanned: number
  extracted: number
  skipped: number
  errors: number
  message?: string
  executed_at: string
}

export interface ExtractHistoryListResult {
  list: ExtractHistory[]
  pagination: { page: number; page_size: number; total: number }
}

export interface ExtractHistoryStats {
  total: number
  success: number
  failed: number
  last_extracted: number
}

// Scheduler status (from /status schedulers block)
// 推送配置调度信息（feature 039）— 每个 active 自动推送配置独立展示
export interface PushConfigSchedule {
  id: number
  name: string
  /** 推送间隔（分钟），来自 push_configs.push_interval */
  push_interval: number
  /** 上次推送本地时间 "YYYY-MM-DD HH:MM:SS"；null=从未推送过或调度器未运行 */
  last_run_at: string | null
  /** 下次预计推送本地时间；null=调度器未运行或全新配置尚未触发首次 */
  next_run: string | null
}

export interface SchedulersStatus {
  extract_running: boolean
  extract_next_run?: string
  extract_interval_minutes: number
  push_running: boolean
  push_next_run?: string
  // 保留：扫描周期（分钟），前端不再用作"推送间隔"展示
  push_interval_minutes: number
  // 活跃自动推送配置数 (is_active=1 AND auto_push=1)
  push_active_configs: number
  // feature 039 新增：系统扫描周期（秒），通常为 60
  push_scan_interval_secs?: number
  // feature 039 新增：每个 active 自动推送配置的调度信息数组
  push_configs?: PushConfigSchedule[]
  forward_running: boolean
  forward_interval_secs: number
}

// Forward queue (image forward tasks)
export interface ForwardTask {
  id: number
  remote_id: string
  channel_id?: number
  message_id?: number
  // 群组A 中的消息 ID（阶段1 完成后写入），用于阶段2 Bot forwardMessage
  image_message_id?: number | null
  title?: string
  description?: string
  link?: string
  file_id?: string
  // awaiting_bot = 阶段1 完成、待阶段2 Bot 转发取 file_id
  status: 'pending' | 'awaiting_bot' | 'forwarded' | 'failed'
  retry_count: number
  error?: string
  created_at: string
  updated_at: string
  collector_id?: number
  channel_name?: string
}

export interface QueueStatusResponse {
  pending: number
  forwarded: number
  failed: number
  // 死信数（failed 且 retry_count >= 5，不再自动重试，需人工处理）
  dead?: number
  tasks: ForwardTask[]
  failed_tasks: ForwardTask[]
}

// ===== Crawler (feature 043) =====
// 043：旧 FieldSelector / FieldSelectors 类型已删除（直接取代 042 抓取路径），
// 字段配置由独立的字段树 API（crawler_task_field_nodes 表）承载。
// US1 T027 将补齐完整的字段树/字段库/Probe TypeScript 类型。

export interface CrawlerTaskInput {
  name: string
  enabled: boolean
  list_urls: string[]
  /** 历史字段：单阶段模式已下线，后端忽略，恒为 true（保留兼容老数据导入） */
  two_stage?: boolean
  interval_minutes: number
  task_concurrency: number
  user_agent?: string | null
  request_delay_ms: number
  proxy?: string | null
  auto_link_check: boolean
  block_detection_config?: string | null
  max_consecutive_failures: number
  template_source?: string | null
  /** 自动翻页：CSS 选择器，一次性匹配页面所有分页链接（含数字页/上一页/下一页/末页）。
   *  空/null = 未启用。引擎把所有命中的 href 去重后批量抓取 */
  pagination_selector?: string | null
  /** 最大抓取页数（含 list_urls 种子页），0=不限 */
  max_pages?: number
  /** 043 US5：字段树 pagination 字段驱动的最大翻页深度，默认 10；0=不限 */
  max_pagination_depth?: number
}

export interface CrawlerTask extends CrawlerTaskInput {
  id: number
  status: string  // 'active' | 'paused' | 'auto_blocked' | 'deleted'
  consecutive_failures: number
  last_run_at: string | null
  next_run_at: string | null
  created_at: string
  updated_at: string
}

export interface CrawlerTemplate {
  key: string
  name: string
  site_type: string
  description: string
  config: CrawlerTaskInput
}

// 测试运行预览（不落库）
export interface CrawlerTestPreview {
  list_count: number
  preview_count: number
  articles: Array<{
    source_url: string
    title: string | null
    content_snippet: string | null
    pan_links: Array<{ platform: string; url: string; extract_code: string | null }>
    direct_links: string[]
    images: string[]
    field_warnings: string[]
  }>
  selector_validation: {
    list_item_ok: boolean
    detail_link_ok: boolean
    missing_fields: string[]
  }
}

// ─── 文章相关（US2） ─────────────────────────────────────────────────────
export interface CrawlerArticleListItem {
  id: number
  task_id: number | null
  source_type: string
  title: string | null
  category: string | null
  thumbnail: string | null
  pan_link_count: number
  direct_link_count: number
  image_count: number
  is_edited: boolean
  crawled_at: string
}

export interface CrawlerArticleLink {
  id: number
  article_id: number
  link_type: 'pan' | 'direct'
  platform: string | null
  url: string
  url_canonical: string
  extract_code: string | null
  validity_status: 'valid' | 'invalid' | 'pending' | 'unknown'
  validity_reason: string | null
  last_checked_at: string | null
  created_at: string
  updated_at: string
}

export interface CrawlerArticleImage {
  id: number
  article_id: number
  original_url: string
  url_canonical: string
  local_path: string | null
  image_message_id: number | null
  file_id: string | null
  status: 'pending' | 'downloaded' | 'uploading' | 'uploaded' | 'failed'
  retry_count: number
  last_error: string | null
  created_at: string
  updated_at: string
}

export interface CrawlerArticleDetail {
  id: number
  task_id: number | null
  source_type: string
  source_url: string
  source_url_canonical: string
  title: string | null
  content: string | null
  category: string | null
  tags: string | null
  is_edited: boolean
  crawled_at: string
  created_at: string
  updated_at: string
  // 展平字段
  links: CrawlerArticleLink[]
  images: CrawlerArticleImage[]
  task_name: string | null
  // 字段树提取结果（GET /articles/:id 顶层并列返回；前端 setDetail 时合并进来）
  extra_fields?: Record<string, unknown>
  field_values?: ArticleFieldValue[]
  field_stats?: FieldHitStats[]
}

// ─── 历史与统计（US3） ──────────────────────────────────────────────────
export interface CrawlerRunHistory {
  id: number
  task_id: number
  task_name: string
  started_at: string
  finished_at: string | null
  duration_ms: number | null
  status: 'success' | 'partial' | 'failed' | 'blocked'
  block_type: string | null
  crawled_count: number
  new_count: number
  skipped_count: number
  failed_count: number
  error_message: string | null
  created_at: string
}

export interface CrawlerRunHistoryDetail {
  // 展平 CrawlerRunHistory 字段
  id: number
  task_id: number
  task_name: string
  started_at: string
  finished_at: string | null
  duration_ms: number | null
  status: 'success' | 'partial' | 'failed' | 'blocked'
  block_type: string | null
  crawled_count: number
  new_count: number
  skipped_count: number
  failed_count: number
  error_message: string | null
  created_at: string
  // 详情扩展
  blocked_response_excerpt: string | null
}

export interface CrawlerHistoryStats {
  total_runs: number
  success: number
  partial: number
  failed: number
  blocked: number
  block_breakdown: Record<string, number>
  last_run_at: string | null
  auto_blocked_tasks: number
}

// ============================================================================
// Feature 043 — Visual Field Configurator (US1 T027)
// ============================================================================

/** 字段作用域：列表页 / 详情页 */
export type FieldScope = 'list_page' | 'detail_page'

/** 源码 tab 一致的来源层 */
export type SourceLayer = 'html' | 'header' | 'script' | 'meta' | 'url'

/** 匹配模式：7 种（6 同步 + follow_url 异步两阶段） */
export type ExtractorMode =
  | 'css'
  | 'regex'
  | 'prefix_suffix'
  | 'json_path'
  | 'meta_attr'
  | 'header_field'
  | 'follow_url'

/** 字段类型 */
export type FieldType =
  | 'string'
  | 'text'
  | 'url'
  | 'image'
  | 'number'
  | 'datetime'
  | 'link_card'
  | 'pagination'
  | 'custom'

/** 6 种匹配模式对应的规则参数（discriminated union） */
export interface CssRule {
  mode: 'css'
  spec: {
    selector: string
    attr: string  // text | html | <attr-name>
  }
}
export interface RegexRule {
  mode: 'regex'
  spec: {
    pattern: string
    group: number
    flags?: string
  }
}
export interface PrefixSuffixRule {
  mode: 'prefix_suffix'
  spec: {
    prefix: string
    suffix: string
    include_boundary?: boolean
    case_sensitive?: boolean
  }
}
export interface JsonPathRule {
  mode: 'json_path'
  spec: { path: string }
}
export interface MetaAttrRule {
  mode: 'meta_attr'
  spec: {
    attr_name: string
    attr_value: string
    content_key?: string
  }
}
export interface HeaderFieldRule {
  mode: 'header_field'
  spec: { header_name: string }
}

/** 字段规则（避免与 042 转发 Rule 冲突，重命名为 FieldRule） */
export type FieldRule =
  | CssRule
  | RegexRule
  | PrefixSuffixRule
  | JsonPathRule
  | MetaAttrRule
  | HeaderFieldRule
  | FollowUrlRule

/**
 * 6 种同步模式的子规则 —— 用于 FollowUrlRule.transit/extract 子规则。
 * 故意不含 FollowUrlRule 变体（编译期杜绝无限嵌套）。
 */
export type SubRule =
  | CssRule
  | RegexRule
  | PrefixSuffixRule
  | JsonPathRule
  | MetaAttrRule
  | HeaderFieldRule

/** follow_url 两阶段提取规则：先抓中转 URL → fetch → 在响应上抓最终值 */
export interface FollowUrlRule {
  mode: 'follow_url'
  spec: {
    /** 在当前 material 上提取中转 URL 的子规则（必填） */
    transit: SubRule
    /** transit 子规则作用的 source_layer，默认 'html' */
    transit_layer?: SourceLayer
    /** source_layer='script' 时指定 script_index */
    transit_script_index?: number | null
    /** 二次请求后 extract 子规则作用的 source_layer，默认 'html' */
    target_layer?: SourceLayer
    /** source_layer='script' 时指定 script_index */
    target_script_index?: number | null
    /** 在二次请求 material 上提取最终值的子规则（必填） */
    extract: SubRule
  }
}

/** 后处理操作 */
export type PostProcessorOp =
  | 'trim'
  | 'html_entity_decode'
  | 'absolutize_url'
  | 'first'
  | 'all'
  | 'dedupe'

export interface PostProcessor {
  op: PostProcessorOp
}

/** 字段节点 spec（应用层视图，已解析 rule/post_processors） */
export interface FieldNodeSpec {
  id?: number
  task_id?: number
  parent_id?: number | null
  scope: FieldScope
  name: string
  display_name: string
  field_type: FieldType
  source_layer: SourceLayer
  extractor_mode: ExtractorMode
  rule: FieldRule
  post_processors?: PostProcessor[]
  script_index?: number | null
  sort_order?: number
  is_active?: boolean
}

/**
 * 快捷创建字段的预填配置（在 SourceViewer 的 meta/header/script tab 行内
 * 点「创建为字段」时生成，由 FieldNodeEditor 消费以预填表单）。
 *
 * scope 由父组件（CrawlerFieldConfigurator）按当前素材 tab 注入。
 */
export interface QuickFieldPreset {
  scope: FieldScope
  /** 推荐字段名（小写英文，符合 NAME_REGEX） */
  suggested_name?: string
  /** 推荐显示名（中文） */
  suggested_display_name?: string
  /** 推荐字段类型 */
  field_type?: FieldType
  source_layer: SourceLayer
  extractor_mode: ExtractorMode
  rule: FieldRule
  script_index?: number | null
}

/** 字段节点（树形：spec + children） */
export interface FieldNode {
  spec: FieldNodeSpec | null
  /** DB 行（spec 解析失败时回退显示用） */
  row?: unknown
  /** 解析错误（spec=null 时存在） */
  error?: string
  children: FieldNode[]
}

/** 字段树：list_page + detail_page 双根 */
export interface FieldTree {
  list_page: FieldNode[]
  detail_page: FieldNode[]
}

/** 预置字段库条目 */
export interface FieldLibraryEntry {
  id: number
  key: string
  display_name: string
  field_type: FieldType
  category: string
  description?: string | null
  suggested_extractor?: ExtractorMode | null
  sort_order: number
  created_at?: string
  updated_at?: string
}

/** 预置字段库分类视图 */
export interface FieldLibraryCategory {
  category: string
  label: string
  entries: FieldLibraryEntry[]
}

// ---- ProbeRequest / ProbeResponse / ProbeError ----

export interface ProbeRequest {
  url: string
  user_agent?: string
  proxy?: string
  source_layer: SourceLayer
  rule: FieldRule
  post_processors?: PostProcessor[]
  script_index?: number | null
  parent_hits?: string[]
  require_parent?: boolean
  /** US2: 父字段定义（与 parent_hits 互斥；优先使用） */
  parent_field?: ParentFieldDef | null
  /** US2: 每条父命中下返回的子样本数上限（默认 3） */
  per_parent_sample_limit?: number | null
  /** US2: 父字段节点 ID（handler 解析为 parent_field） */
  parent_node_id?: number | null
}

/** US2: 父字段定义（handler 通过 parent_node_id 查表填充） */
export interface ParentFieldDef {
  source_layer: SourceLayer
  rule: FieldRule
  post_processors?: PostProcessor[]
  script_index?: number | null
}

export interface ProbeSample {
  value: string
  source_fragment: string
  location?: string | null
}

/** US2: 按父命中分组的子字段结果 */
export interface PerParentSample {
  /** 父命中序号（0-based） */
  parent_index: number
  /** 父命中片段摘要 */
  parent_fragment: string
  /** 子字段在该父作用域下的首个命中值（None=未命中） */
  child_value?: string | null
  /** 子字段在该父作用域下是否命中 */
  child_hit: boolean
  /** 子字段在该父作用域下的全部命中样本（受 per_parent_sample_limit 截断） */
  child_samples?: ProbeSample[] | null
}

export interface ProbeResponse {
  hit_count: number
  samples: ProbeSample[]
  /** US2: 父子嵌套验证时填充（按父命中序号排列） */
  per_parent?: PerParentSample[] | null
  fetched_url: string
  fetched_at: string
  duration_ms: number
}

export type ProbeStage = 'fetch' | 'parse' | 'match'

export type ProbeCategory =
  | 'url_unreachable'
  | 'http_4xx_5xx'
  | 'blocked'
  | 'invalid_rule'
  | 'zero_hits'
  | 'parent_empty'

export interface ProbeError {
  stage: ProbeStage
  category: ProbeCategory
  message: string
  hint?: string | null
}

// ---- SourceMaterial (fetch-source) ----

export interface ScriptBlock {
  index: number
  src?: string | null
  content?: string | null
}

export type MetaKeyKind = 'name' | 'property' | 'http_equiv' | 'other'

export interface MetaTag {
  key_kind: MetaKeyKind
  key: string
  content: string
}

export interface SourceMaterial {
  final_url: string
  status: number
  headers: Record<string, string>
  html: string
  scripts: ScriptBlock[]
  metas: MetaTag[]
  fetched_at: string
  duration_ms: number
}

export interface FetchSourceRequest {
  url: string
  user_agent?: string
  proxy?: string
}

// ---- 字段节点 CRUD 请求体 ----

export interface CreateFieldNodeBody {
  parent_id?: number | null
  scope: FieldScope
  name: string
  display_name: string
  source_layer: SourceLayer
  extractor_mode: ExtractorMode
  rule: FieldRule
  post_processors?: PostProcessor[]
  script_index?: number | null
  sort_order?: number
  is_active?: boolean
  field_type?: FieldType
}

export interface ReorderFieldNodesBody {
  parent_id: number | null
  scope: FieldScope
  ordered_ids: number[]
}

/** 文章字段值长表行（crawler_article_field_values） */
export interface ArticleFieldValue {
  id: number
  article_id: number
  field_node_id?: number | null
  field_path: string
  scope: FieldScope
  value_index: number
  value_text?: string | null
  value_number?: number | null
  is_hit: boolean
  created_at: string
}

/** 字段命中统计（按 field_path 聚合） */
export interface FieldHitStats {
  field_path: string
  total: number
  hit: number
  missed: number
}

/** 单字段任务级命中率（contracts C7 / FR-027） */
export interface FieldStat {
  field_node_id: number | null
  field_path: string
  field_name: string | null
  field_display_name: string | null
  total_articles: number
  hit_articles: number
  /** 0~1，保留 2 位小数 */
  hit_rate: number
  /** healthy(≥0.80) | degraded(0.10~0.80) | stale_warning(<0.10) */
  status: 'healthy' | 'degraded' | 'stale_warning'
}

/** `GET /api/crawler/tasks/{id}/field-stats` 响应 data */
export interface FieldStatsResponse {
  window_days: number
  stats: FieldStat[]
}

