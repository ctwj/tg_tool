import apiClient from './client'
import type {
  ApiResponse,
  CrawlerArticleDetail,
  CrawlerArticleListItem,
  CrawlerHistoryStats,
  CrawlerRunHistory,
  CrawlerRunHistoryDetail,
  CrawlerTask,
  CrawlerTaskInput,
  CrawlerTemplate,
  CrawlerTestPreview,
  PaginatedResponse,
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

export async function saveAsTemplate(
  id: number,
  name: string,
  description?: string,
): Promise<ApiResponse<CrawlerTemplate>> {
  const res = await apiClient.post(`/crawler/tasks/${id}/save-as-template`, { name, description })
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
