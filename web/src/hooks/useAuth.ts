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
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const login = async (username: string, password: string) => {
    const res = await apiClient.post('/auth/login', { username, password })
    const { token } = res.data.data
    localStorage.setItem(TOKEN_KEY, token)
    apiClient.defaults.headers.common.Authorization = `Bearer ${token}`

    // Try to fetch user info; if it fails (no auth middleware yet),
    // create a minimal user from the token so the UI works
    try {
      const meRes = await apiClient.get('/auth/me')
      const userData = meRes.data.data as User
      setUser(userData)
      localStorage.setItem(USER_KEY, JSON.stringify(userData))
    } catch {
      // Auth middleware not yet applied — set a flag so isAuthenticated = true
      const minimalUser: User = {
        id: 0,
        username,
        role: 1,
        status: 1,
        created_at: new Date().toISOString(),
      }
      setUser(minimalUser)
      localStorage.setItem(USER_KEY, JSON.stringify(minimalUser))
    }

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
    navigate('/login')
  }

  const isAdmin = user?.role != null && user.role >= 10
  const isRoot = user?.role != null && user.role >= 100
  const isAuthenticated = !!localStorage.getItem(TOKEN_KEY)

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
