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
export interface SchedulersStatus {
  extract_running: boolean
  extract_next_run?: string
  extract_interval_minutes: number
  push_running: boolean
  push_next_run?: string
  push_interval_minutes: number
  forward_running: boolean
  forward_interval_secs: number
}

// Forward queue (image forward tasks)
export interface ForwardTask {
  id: number
  remote_id: string
  channel_id?: number
  message_id?: number
  title?: string
  description?: string
  link?: string
  file_id?: string
  status: 'pending' | 'forwarded' | 'failed'
  retry_count: number
  error?: string
  created_at: string
  updated_at: string
}

export interface QueueStatusResponse {
  pending: number
  forwarded: number
  failed: number
  tasks: ForwardTask[]
  failed_tasks: ForwardTask[]
}
