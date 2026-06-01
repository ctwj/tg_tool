import React from 'react'
import { Navigate } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'

const AuthGuard: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { isAuthenticated, loading } = useAuth()

  if (loading) return <div style={{ textAlign: 'center', padding: 50 }}>加载中...</div>
  if (!isAuthenticated) return <Navigate to="/login" replace />

  return <>{children}</>
}

export default AuthGuard
