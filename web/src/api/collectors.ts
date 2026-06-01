import apiClient from './client'
import type { Collector, CollectorHistory, ApiResponse, PaginatedResponse } from '../types'

export async function listCollectors(page = 1, pageSize = 10): Promise<PaginatedResponse<Collector>> {
  const res = await apiClient.get('/api/collectors', { params: { page, page_size: pageSize } })
  return res.data
}

export async function createCollector(data: Partial<Collector>): Promise<ApiResponse> {
  const res = await apiClient.post('/api/collectors', data)
  return res.data
}

export async function getCollector(id: number): Promise<ApiResponse<Collector>> {
  const res = await apiClient.get(`/api/collectors/${id}`)
  return res.data
}

export async function updateCollector(id: number, data: Partial<Collector>): Promise<ApiResponse> {
  const res = await apiClient.put(`/api/collectors/${id}`, data)
  return res.data
}

export async function deleteCollector(id: number): Promise<ApiResponse> {
  const res = await apiClient.delete(`/api/collectors/${id}`)
  return res.data
}

export async function toggleCollector(id: number): Promise<ApiResponse> {
  const res = await apiClient.put(`/api/collectors/${id}/toggle`)
  return res.data
}

export async function fetchHistory(id: number): Promise<ApiResponse> {
  const res = await apiClient.post(`/api/collectors/${id}/fetch`)
  return res.data
}

export async function listHistories(page = 1, pageSize = 20): Promise<PaginatedResponse<CollectorHistory>> {
  const res = await apiClient.get('/api/collectors/histories', { params: { page, page_size: pageSize } })
  return res.data
}
