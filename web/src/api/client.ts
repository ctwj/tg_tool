import axios from 'axios'

export interface ApiErrorData {
  captcha_required?: boolean
  [key: string]: unknown
}

export class ApiError extends Error {
  data?: ApiErrorData
  constructor(message: string, data?: ApiErrorData) {
    super(message)
    this.data = data
  }
}

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
      const err = new ApiError(data.message || '请求失败', data.data)
      return Promise.reject(err)
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
