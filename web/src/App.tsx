import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { ConfigProvider } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import Layout from './components/Layout'
import AuthGuard from './components/AuthGuard'
import Login from './pages/Login'
import Dashboard from './pages/Dashboard'
import Clients from './pages/Clients'
import ClientAuth from './pages/ClientAuth'
import Rules from './pages/Rules'
import Collectors from './pages/Collectors'
import CollectorHistory from './pages/CollectorHistory'
import Push from './pages/Push'
import Resources from './pages/Resources'
import Users from './pages/Users'
import Files from './pages/Files'
import Settings from './pages/Settings'
import ApiStatus from './pages/ApiStatus'
import Scheduler from './pages/Scheduler'
import ForwardQueue from './pages/ForwardQueue'
import './index.css'

function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        token: {
          // 品牌主色 + 圆角 + 背景（与 index.css :root 变量同步）
          colorPrimary: '#0ea5e9',
          borderRadius: 8,
          colorBgLayout: '#f0f9ff',
          fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif',
          // 语义状态色（全站 Tag/Badge/Statistic/Alert 自动统一）
          colorSuccess: '#10b981',
          colorError: '#ef4444',
          colorWarning: '#f59e0b',
          colorInfo: '#0ea5e9',
          // 文字层级（WCAG AA 对比度，与页面内联样式一致）
          colorText: '#1f2937',
          colorTextSecondary: '#6b7280',
          colorTextTertiary: '#9ca3af',
          // 边框
          colorBorder: '#e5e7eb',
          colorBorderSecondary: '#bae6fd',
        },
        components: {
          Button: { borderRadius: 6 },
          Card: { borderRadiusLG: 12 },
          Table: { borderRadius: 8 },
        },
      }}
    >
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route path="/" element={<AuthGuard><Layout /></AuthGuard>}>
            <Route index element={<Navigate to="/dashboard" replace />} />
            <Route path="dashboard" element={<Dashboard />} />
            <Route path="clients" element={<Clients />} />
            <Route path="client-auth" element={<ClientAuth />} />
            <Route path="rules" element={<Rules />} />
            <Route path="collectors" element={<Collectors />} />
            <Route path="collectors/:id/history" element={<CollectorHistory />} />
            <Route path="push" element={<Push />} />
            <Route path="scheduler" element={<Scheduler />} />
            <Route path="forward-queue" element={<ForwardQueue />} />
            <Route path="resources" element={<Resources />} />
            <Route path="users" element={<Users />} />
            <Route path="files" element={<Files />} />
            <Route path="settings" element={<Settings />} />
            <Route path="api-status" element={<ApiStatus />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </BrowserRouter>
    </ConfigProvider>
  )
}

export default App
