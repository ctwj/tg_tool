import apiClient from './client'
import type { User, ApiResponse, PaginatedResponse } from '../types'

export async function listUsers(page = 1, pageSize = 10, keyword?: string): Promise<PaginatedResponse<User>> {
  const params: Record<string, any> = { page, page_size: pageSize }
  if (keyword) params.keyword = keyword
  const res = await apiClient.get('/users', { params })
  return res.data
}

export async function createUser(data: Partial<User> & { password: string }): Promise<ApiResponse> {
  const res = await apiClient.post('/users', data)
  return res.data
}

export async function getUser(id: number): Promise<ApiResponse<User>> {
  const res = await apiClient.get(`/users/${id}`)
  return res.data
}

export async function updateUser(id: number, data: Partial<User>): Promise<ApiResponse> {
  const res = await apiClient.put(`/users/${id}`, data)
  return res.data
}

export async function deleteUser(id: number): Promise<ApiResponse> {
  const res = await apiClient.delete(`/users/${id}`)
  return res.data
}
