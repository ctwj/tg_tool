import apiClient from './client'
import type { Client, ClientAuthRequest, Chat, ApiResponse } from '../types'

export async function listClients(): Promise<ApiResponse<{ list: Client[] }>> {
  const res = await apiClient.get('/clients')
  return res.data
}

export async function addClient(data: Partial<Client>): Promise<ApiResponse<Client>> {
  const res = await apiClient.post('/clients', data)
  return res.data
}

export async function removeClient(id: string): Promise<ApiResponse> {
  const res = await apiClient.delete(`/clients/${id}`)
  return res.data
}

export async function getClientStatus(id: string): Promise<ApiResponse<{ status: string }>> {
  const res = await apiClient.get(`/clients/${id}`)
  return res.data
}

export async function startClient(id: string): Promise<ApiResponse> {
  const res = await apiClient.post(`/clients/${id}/start`)
  return res.data
}

export async function stopClient(id: string): Promise<ApiResponse> {
  const res = await apiClient.post(`/clients/${id}/stop`)
  return res.data
}

export async function authClient(id: string, data: ClientAuthRequest): Promise<ApiResponse> {
  const res = await apiClient.post(`/clients/${id}/auth`, data)
  return res.data
}

export async function getChats(id: string, listType?: string): Promise<ApiResponse<{ chats: Chat[] }>> {
  const params = listType ? { list_type: listType } : {}
  const res = await apiClient.get(`/clients/${id}/chats`, { params })
  return res.data
}
