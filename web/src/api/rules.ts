import apiClient from './client'
import type { Rule, Message, PaginatedResponse } from '../types'

export async function listRules(page = 1, pageSize = 10): Promise<PaginatedResponse<Rule>> {
  const res = await apiClient.get('/rules', { params: { page, page_size: pageSize } })
  return res.data
}

export async function createRule(data: Partial<Rule>): Promise<ApiResponse<Rule>> {
  const res = await apiClient.post('/rules', data)
  return res.data
}

export async function getRule(id: number): Promise<ApiResponse<Rule>> {
  const res = await apiClient.get(`/rules/${id}`)
  return res.data
}

export async function updateRule(id: number, data: Partial<Rule>): Promise<ApiResponse<Rule>> {
  const res = await apiClient.put(`/rules/${id}`, data)
  return res.data
}

export async function deleteRule(id: number): Promise<ApiResponse> {
  const res = await apiClient.delete(`/rules/${id}`)
  return res.data
}

export async function toggleRule(id: number): Promise<ApiResponse> {
  const res = await apiClient.put(`/rules/${id}/toggle`)
  return res.data
}

export async function getRuleMessages(id: number, page = 1, pageSize = 10): Promise<PaginatedResponse<Message>> {
  const res = await apiClient.get(`/rules/${id}/messages`, { params: { page, page_size: pageSize } })
  return res.data
}

// Re-export ApiResponse since it's used directly
import type { ApiResponse } from '../types'
