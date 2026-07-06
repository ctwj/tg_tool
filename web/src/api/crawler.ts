import apiClient from './client'
import type {
  ApiResponse,
  CreateFieldNodeBody,
  CrawlerArticleDetail,
  CrawlerArticleListItem,
  CrawlerHistoryStats,
  CrawlerRunHistory,
  CrawlerRunHistoryDetail,
  CrawlerTask,
  CrawlerTaskInput,
  CrawlerTemplate,
  CrawlerTestPreview,
  FetchSourceRequest,
  FieldLibraryCategory,
  FieldStatsResponse,
  FieldTree,
  PaginatedResponse,
  ProbeRequest,
  ProbeResponse,
  ReorderFieldNodesBody,
  ScriptSandboxRequest,
  ScriptSandboxResponse,
  SourceMaterial,
} from '../types'

export interface TaskListParams {
  page?: number
  page_size?: number
  status?: string
  enabled?: boolean
  keyword?: string
}

export async function listTasks(params: TaskListParams = {}): Promise<PaginatedResponse<CrawlerTask>> {
  const res = await apiClient.get('/crawler/tasks', {
    params: {
      page: params.page ?? 1,
      page_size: params.page_size ?? 20,
      status: params.status,
      enabled: params.enabled,
      keyword: params.keyword,
    },
  })
  return res.data
}

export async function getTask(id: number): Promise<ApiResponse<CrawlerTask>> {
  const res = await apiClient.get(`/crawler/tasks/${id}`)
  return res.data
}

export async function createTask(data: CrawlerTaskInput): Promise<ApiResponse<CrawlerTask>> {
  const res = await apiClient.post('/crawler/tasks', data)
  return res.data
}

export async function updateTask(id: number, data: Partial<CrawlerTaskInput>): Promise<ApiResponse<CrawlerTask>> {
  const res = await apiClient.put(`/crawler/tasks/${id}`, data)
  return res.data
}

export async function deleteTask(id: number, cascadeArticles = false): Promise<ApiResponse> {
  const res = await apiClient.delete(`/crawler/tasks/${id}`, {
    params: { cascade_articles: cascadeArticles },
  })
  return res.data
}

export async function toggleTask(id: number, enabled: boolean): Promise<ApiResponse<CrawlerTask>> {
  const res = await apiClient.put(`/crawler/tasks/${id}/toggle`, { enabled })
  return res.data
}

export async function runTask(id: number): Promise<ApiResponse<{ task_id: number; started: boolean }>> {
  const res = await apiClient.post(`/crawler/tasks/${id}/run`)
  return res.data
}

export async function testTask(id: number, limit = 3): Promise<ApiResponse<CrawlerTestPreview>> {
  const res = await apiClient.post(`/crawler/tasks/${id}/test`, { limit })
  return res.data
}

export async function exportTask(id: number): Promise<Blob> {
  const res = await apiClient.get(`/crawler/tasks/${id}/export`, { responseType: 'blob' })
  return res.data
}

export async function importTask(data: CrawlerTaskInput): Promise<ApiResponse<CrawlerTask>> {
  const res = await apiClient.post('/crawler/tasks/import', data)
  return res.data
}

export async function listTemplates(): Promise<ApiResponse<CrawlerTemplate[]>> {
  const res = await apiClient.get('/crawler/templates')
  return res.data
}

// ─── 文章相关（US2） ────────────────────────────────────────────────────
export interface ArticleListParams {
  page?: number
  page_size?: number
  task_id?: number
  source_type?: string
  category?: string
  crawled_after?: string
  crawled_before?: string
  keyword?: string
}

export async function listArticles(
  params: ArticleListParams = {},
): Promise<{ success: boolean; data: { list: CrawlerArticleListItem[]; pagination: { page: number; page_size: number; total: number } } }> {
  const res = await apiClient.get('/crawler/articles', {
    params: {
      page: params.page ?? 1,
      page_size: params.page_size ?? 20,
      task_id: params.task_id,
      source_type: params.source_type,
      category: params.category,
      crawled_after: params.crawled_after,
      crawled_before: params.crawled_before,
      keyword: params.keyword,
    },
  })
  return res.data
}

export async function getArticleDetail(id: number): Promise<ApiResponse<CrawlerArticleDetail>> {
  const res = await apiClient.get(`/crawler/articles/${id}`)
  return res.data
}

export interface UpdateArticleBody {
  title?: string
  content?: string
  category?: string
  tags?: string
}

export async function updateArticle(
  id: number,
  body: UpdateArticleBody,
): Promise<ApiResponse<{ id: number; updated: number }>> {
  const res = await apiClient.put(`/crawler/articles/${id}`, body)
  return res.data
}

export async function deleteArticle(
  id: number,
): Promise<ApiResponse<{ id: number; articles_deleted: number }>> {
  const res = await apiClient.delete(`/crawler/articles/${id}`)
  return res.data
}

export async function batchDeleteArticles(
  ids: number[],
): Promise<ApiResponse<{ deleted: number; requested: number }>> {
  const res = await apiClient.post('/crawler/articles/batch-delete', { ids })
  return res.data
}

export async function retryImage(
  articleId: number,
  imageId: number,
): Promise<ApiResponse<{ id: number; article_id: number; reset: boolean }>> {
  const res = await apiClient.post(`/crawler/articles/${articleId}/images/${imageId}/retry`)
  return res.data
}

export async function checkArticleLinks(
  id: number,
): Promise<
  ApiResponse<{ article_id: number; checked: number; note?: string }>
> {
  const res = await apiClient.post(`/crawler/articles/${id}/links/check`)
  return res.data
}

// [feature 046 US4] 手动刷新文章字段（仅 script 字段，admin 权限）
export async function refreshArticleField(
  articleId: number,
  fieldName: string,
): Promise<
  ApiResponse<{
    old_value: string
    new_value: string
    duration_ms: number
  }>
> {
  const res = await apiClient.post(
    `/crawler/articles/${articleId}/fields/${encodeURIComponent(fieldName)}/refresh`,
  )
  return res.data
}

// ─── 历史与统计（US3） ───────────────────────────────────────────────────
export interface HistoryListParams {
  page?: number
  page_size?: number
  task_id?: number
  status?: string
  started_after?: string
  started_before?: string
}

export async function listHistories(
  params: HistoryListParams = {},
): Promise<{
  success: boolean
  data: {
    list: CrawlerRunHistory[]
    pagination: { page: number; page_size: number; total: number }
  }
}> {
  const res = await apiClient.get('/crawler/histories', {
    params: {
      page: params.page ?? 1,
      page_size: params.page_size ?? 20,
      task_id: params.task_id,
      status: params.status,
      started_after: params.started_after,
      started_before: params.started_before,
    },
  })
  return res.data
}

export async function getHistoryDetail(
  id: number,
): Promise<ApiResponse<CrawlerRunHistoryDetail>> {
  const res = await apiClient.get(`/crawler/histories/${id}`)
  return res.data
}

export interface HistoryStatsParams {
  task_id?: number
  days?: number
}

export async function getHistoryStats(
  params: HistoryStatsParams = {},
): Promise<ApiResponse<CrawlerHistoryStats>> {
  const res = await apiClient.get('/crawler/histories/stats', {
    params: {
      task_id: params.task_id,
      days: params.days ?? 7,
    },
  })
  return res.data
}

// ─── 字段配置器（feature 043 US1 T028） ──────────────────────────────────

/** `POST /api/crawler/tasks/fetch-source` — 抓取 URL 4-tab 素材 */
export async function fetchSource(req: FetchSourceRequest): Promise<ApiResponse<SourceMaterial>> {
  const res = await apiClient.post('/crawler/tasks/fetch-source', req)
  return res.data
}

/** US3：取详情页样本素材（POST /crawler/tasks/fetch-detail-sample） */
export interface FetchDetailSampleRequest {
  task_id: number
  list_url: string
  user_agent?: string
  proxy?: string
}

export interface FetchDetailSampleResponse {
  detail_url: string
  material: SourceMaterial
}

export async function fetchDetailSample(
  req: FetchDetailSampleRequest,
): Promise<ApiResponse<FetchDetailSampleResponse>> {
  const res = await apiClient.post('/crawler/tasks/fetch-detail-sample', req)
  return res.data
}

/** `POST /api/crawler/tasks/field-probe` — 字段验证探针 */
export async function runFieldProbe(req: ProbeRequest): Promise<ApiResponse<ProbeResponse>> {
  const res = await apiClient.post('/crawler/tasks/field-probe', req)
  return res.data
}

/** `POST /api/crawler/articles/script-sandbox` — 脚本字段沙盒试跑（不写库） */
export async function runScriptSandbox(
  req: ScriptSandboxRequest,
): Promise<ApiResponse<ScriptSandboxResponse>> {
  const res = await apiClient.post('/crawler/articles/script-sandbox', req)
  return res.data
}

/** `GET /api/crawler/field-library` — 预置字段库（按 category 分组） */
export async function getFieldLibrary(): Promise<ApiResponse<FieldLibraryCategory[]>> {
  const res = await apiClient.get('/crawler/field-library')
  return res.data
}

/** `GET /api/crawler/tasks/{id}/field-tree` — 任务字段树 */
export async function getTaskFieldTree(taskId: number): Promise<ApiResponse<FieldTree>> {
  const res = await apiClient.get(`/crawler/tasks/${taskId}/field-tree`)
  return res.data
}

/** `POST /api/crawler/tasks/{id}/field-nodes` — 新增字段节点 */
export async function createFieldNode(
  taskId: number,
  body: CreateFieldNodeBody,
): Promise<ApiResponse<{ id: number; task_id: number }>> {
  const res = await apiClient.post(`/crawler/tasks/${taskId}/field-nodes`, body)
  return res.data
}

/** `PUT /api/crawler/tasks/{id}/field-nodes/{node_id}` — 更新字段节点 */
export async function updateFieldNode(
  taskId: number,
  nodeId: number,
  body: CreateFieldNodeBody,
): Promise<ApiResponse<{ id: number }>> {
  const res = await apiClient.put(`/crawler/tasks/${taskId}/field-nodes/${nodeId}`, body)
  return res.data
}

/** `DELETE /api/crawler/tasks/{id}/field-nodes/{node_id}` — 删除字段节点（级联子孙） */
export async function deleteFieldNode(
  taskId: number,
  nodeId: number,
): Promise<ApiResponse<{ deleted_children: number }>> {
  const res = await apiClient.delete(`/crawler/tasks/${taskId}/field-nodes/${nodeId}`)
  return res.data
}

/** `PUT /api/crawler/tasks/{id}/field-nodes/reorder` — 同 parent 下批量更新 sort_order */
export async function reorderFieldNodes(
  taskId: number,
  body: ReorderFieldNodesBody,
): Promise<ApiResponse<{ updated: number }>> {
  const res = await apiClient.put(`/crawler/tasks/${taskId}/field-nodes/reorder`, body)
  return res.data
}

/** 内置字段树预置模板（GET /api/crawler/task-templates 单元素） */
export interface BuiltinTemplate {
  key: string
  name: string
  description: string
  source_type: string
  field_tree: FieldTree
}

/** `GET /api/crawler/task-templates` — 内置字段树预置模板列表 */
export async function getTaskTemplates(): Promise<ApiResponse<BuiltinTemplate[]>> {
  const res = await apiClient.get('/crawler/task-templates')
  return res.data
}

/** `POST /api/crawler/tasks/from-template` 请求体 */
export interface CreateTaskFromTemplateBody {
  template_key: string
  task_name: string
  list_url: string
  enabled?: boolean
}

/** `POST /api/crawler/tasks/from-template` — 基于模板创建任务 */
export async function createTaskFromTemplate(
  body: CreateTaskFromTemplateBody,
): Promise<ApiResponse<{ id: number; task: CrawlerTask; field_node_count: number }>> {
  const res = await apiClient.post('/crawler/tasks/from-template', body)
  return res.data
}

// ─── 字段命中率统计（Phase 8 T058 / FR-027） ──────────────────────────────

/** `GET /api/crawler/tasks/{id}/field-stats?days=30` — 任务级字段命中率 */
export async function getTaskFieldStats(
  taskId: number,
  days = 30,
): Promise<ApiResponse<FieldStatsResponse>> {
  const res = await apiClient.get(`/crawler/tasks/${taskId}/field-stats`, {
    params: { days },
  })
  return res.data
}
