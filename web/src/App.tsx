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
import './index.css'

function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        token: {
          colorPrimary: '#6366F1',
          borderRadius: 8,
          colorBgLayout: '#f5f3ff',
          fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif',
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
