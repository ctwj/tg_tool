import { useEffect, useState, useCallback } from 'react'
import { useNavigate } from 'react-router-dom'
import type { User } from '../types'
import apiClient from '../api/client'

const TOKEN_KEY = 'token'
const USER_KEY = 'user'

export function useAuth() {
  const navigate = useNavigate()
  const [user, setUser] = useState<User | null>(() => {
    const stored = localStorage.getItem(USER_KEY)
    return stored ? JSON.parse(stored) : null
  })
  const [loading, setLoading] = useState(false)

  const fetchUser = useCallback(async () => {
    try {
      setLoading(true)
      const res = await apiClient.get('/auth/me')
      const userData = res.data.data as User
      setUser(userData)
      localStorage.setItem(USER_KEY, JSON.stringify(userData))
    } catch {
      // Token invalid, clear
      localStorage.removeItem(TOKEN_KEY)
      localStorage.removeItem(USER_KEY)
      setUser(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    const token = localStorage.getItem(TOKEN_KEY)
    if (token && !user) {
      fetchUser()
    }
  }, [user, fetchUser])

  const login = async (username: string, password: string) => {
    const res = await apiClient.post('/auth/login', { username, password })
    const { token } = res.data.data
    localStorage.setItem(TOKEN_KEY, token)
    apiClient.defaults.headers.common.Authorization = `Bearer ${token}`
    await fetchUser()
    navigate('/dashboard')
  }

  const register = async (username: string, password: string, email?: string) => {
    await apiClient.post('/auth/register', { username, password, email })
  }

  const logout = () => {
    apiClient.post('/auth/logout').catch(() => {})
    localStorage.removeItem(TOKEN_KEY)
    localStorage.removeItem(USER_KEY)
    setUser(null)
    delete apiClient.defaults.headers.common.Authorization
  }

  const isAdmin = user?.role != null && user.role >= 10
  const isRoot = user?.role != null && user.role >= 100
  const isAuthenticated = !!user && user.status === 1

  return {
    user,
    loading,
    login,
    register,
    logout,
    fetchUser,
    isAdmin,
    isRoot,
    isAuthenticated,
  }
}
