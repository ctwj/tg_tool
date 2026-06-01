import apiClient from './client'
import type { FileItem, ApiResponse, PaginatedResponse } from '../types'

export async function listFiles(page = 1, pageSize = 10): Promise<PaginatedResponse<FileItem>> {
  const res = await apiClient.get('/files', { params: { page, page_size: pageSize } })
  return res.data
}

export async function deleteFile(id: number): Promise<ApiResponse> {
  const res = await apiClient.delete(`/files/${id}`)
  return res.data
}

export function getFileDownloadUrl(filename: string): string {
  return `/api/files/download/${filename}`
}
