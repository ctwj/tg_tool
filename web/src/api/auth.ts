import apiClient from './client'
import type { User, LoginForm, RegisterForm, ApiResponse } from '../types'

export async function login(data: LoginForm): Promise<ApiResponse<{ token: string }>> {
  const res = await apiClient.post('/auth/login', data)
  return res.data
}

export async function register(data: RegisterForm): Promise<ApiResponse<{ token: string }>> {
  const res = await apiClient.post('/auth/register', data)
  return res.data
}

export async function logout(): Promise<ApiResponse> {
  const res = await apiClient.post('/auth/logout')
  return res.data
}

export async function getMe(): Promise<ApiResponse<User>> {
  const res = await apiClient.get('/auth/me')
  return res.data
}

export async function updateMe(data: Partial<User>): Promise<ApiResponse<User>> {
  const res = await apiClient.put('/auth/me', data)
  return res.data
}

export async function generateToken(): Promise<ApiResponse<{ token: string }>> {
  const res = await apiClient.post('/auth/token')
  return res.data
}
