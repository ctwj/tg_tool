import apiClient from './client'
import type { PushHistory, ApiResponse, PaginatedResponse } from '../types'

export async function triggerPush(data?: Record<string, any>): Promise<ApiResponse> {
  const res = await apiClient.post('/push/trigger', data || {})
  return res.data
}

export async function getStats(): Promise<ApiResponse<{ total: number; success: number; failed: number }>> {
  const res = await apiClient.get('/push/stats')
  return res.data
}

export async function listHistories(page = 1, pageSize = 10): Promise<PaginatedResponse<PushHistory>> {
  const res = await apiClient.get('/push/histories', { params: { page, page_size: pageSize } })
  return res.data
}

export async function retryPush(): Promise<ApiResponse> {
  const res = await apiClient.post('/push/retry')
  return res.data
}

export async function updateScheduler(data: Record<string, any>): Promise<ApiResponse> {
  const res = await apiClient.put('/push/scheduler', data)
  return res.data
}
