import axios from 'axios'

const apiClient = axios.create({
  baseURL: '/api',
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
})

// Request interceptor: attach token
apiClient.interceptors.request.use(
  (config) => {
    const token = localStorage.getItem('token')
    if (token) {
      config.headers.Authorization = `Bearer ${token}`
    }
    return config
  },
  (error) => Promise.reject(error),
)

// Response interceptor: unified error handling
apiClient.interceptors.response.use(
  (response) => {
    const data = response.data
    if (data.success === false) {
      return Promise.reject(new Error(data.message || '请求失败'))
    }
    return response
  },
  (error) => {
    if (error.response) {
      const { status, data } = error.response
      if (status === 401) {
        localStorage.removeItem('token')
        localStorage.removeItem('user')
        window.location.href = '/login'
        return Promise.reject(new Error('未登录或登录已过期'))
      }
      const message = data?.message || `请求失败 (${status})`
      return Promise.reject(new Error(message))
    }
    if (error.request) {
      return Promise.reject(new Error('网络错误，请检查网络连接'))
    }
    return Promise.reject(error)
  },
)

export default apiClient
