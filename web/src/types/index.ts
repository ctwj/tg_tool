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

// ===== Crawler (feature 042) =====

export interface FieldSelector {
  css: string
  attr?: string | null
  regex?: string | null
}

export interface FieldSelectors {
  list_item: string
  detail_link: string
  detail_link_attr?: string | null
  title: FieldSelector
  content: FieldSelector
  category: FieldSelector
  tags: FieldSelector
  images: FieldSelector
  pan_links: FieldSelector
  direct_links: FieldSelector
}

export interface CrawlerTaskInput {
  name: string
  enabled: boolean
  list_urls: string[]
  selectors: FieldSelectors
  two_stage: boolean
  interval_minutes: number
  task_concurrency: number
  user_agent?: string | null
  request_delay_ms: number
  proxy?: string | null
  auto_link_check: boolean
  block_detection_config?: string | null
  max_consecutive_failures: number
  template_source?: string | null
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

