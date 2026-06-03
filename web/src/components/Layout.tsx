import React, { useState } from 'react'
import { Layout as AntLayout, Menu, Avatar, Dropdown } from 'antd'
import {
  DashboardOutlined,
  ApiOutlined,
  SendOutlined,
  CloudDownloadOutlined,
  RocketOutlined,
  DatabaseOutlined,
  UserOutlined,
  FileOutlined,
  SettingOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
} from '@ant-design/icons'
import { useNavigate, useLocation, Outlet } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'

const { Header, Sider, Content } = AntLayout

const menuItems = [
  { key: '/dashboard', icon: <DashboardOutlined />, label: '仪表盘' },
  { key: '/clients', icon: <ApiOutlined />, label: '客户端管理' },
  { key: '/rules', icon: <SendOutlined />, label: '转发规则' },
  { key: '/collectors', icon: <CloudDownloadOutlined />, label: '采集器' },
  { key: '/push', icon: <RocketOutlined />, label: '推送管理' },
  { key: '/resources', icon: <DatabaseOutlined />, label: '资源管理' },
  { key: '/users', icon: <UserOutlined />, label: '用户管理' },
  { key: '/files', icon: <FileOutlined />, label: '文件管理' },
  { key: '/settings', icon: <SettingOutlined />, label: '系统设置' },
]

const pageTitles: Record<string, string> = {
  '/dashboard': '仪表盘',
  '/clients': '客户端管理',
  '/rules': '转发规则',
  '/collectors': '采集器管理',
  '/push': '推送管理',
  '/resources': '资源管理',
  '/users': '用户管理',
  '/files': '文件管理',
  '/settings': '系统设置',
}

// 根据路径获取当前标题（支持子路径）
const getTitle = (pathname: string) => {
  if (pathname.startsWith('/collectors/') && pathname.endsWith('/history')) return '采集记录'
  return pageTitles[pathname] || 'TG Forwarding'
}

// 根据路径获取侧边栏选中的 key（子路径选中父菜单项）
const getMenuKey = (pathname: string) => {
  if (pathname.startsWith('/collectors/')) return '/collectors'
  return pathname
}

const Layout: React.FC = () => {
  const [collapsed, setCollapsed] = useState(false)
  const navigate = useNavigate()
  const location = useLocation()
  const { user, logout } = useAuth()

  const currentTitle = getTitle(location.pathname)
  const selectedKey = getMenuKey(location.pathname)

  const userMenu = {
    items: [
      {
        key: 'profile',
        icon: <UserOutlined />,
        label: user?.display_name || user?.username || '用户',
        disabled: true,
      },
      { type: 'divider' as const },
      {
        key: 'logout',
        icon: <LogoutOutlined />,
        label: '退出登录',
        danger: true,
        onClick: () => {
          logout()
          navigate('/login')
        },
      },
    ],
  }

  return (
    <AntLayout style={{ minHeight: '100vh' }}>
      <Sider
        trigger={null}
        collapsible
        collapsed={collapsed}
        width={240}
        style={{
          background: 'linear-gradient(180deg, #1e1b4b 0%, #312e81 100%)',
          boxShadow: '2px 0 8px rgba(0,0,0,0.15)',
        }}
      >
        {/* Logo */}
        <div style={{
          height: 64,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          borderBottom: '1px solid rgba(255,255,255,0.08)',
          cursor: 'pointer',
        }}
          onClick={() => navigate('/dashboard')}
        >
          <div style={{
            width: 36,
            height: 36,
            borderRadius: 10,
            background: 'linear-gradient(135deg, #6366f1, #818cf8)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 18,
            fontWeight: 700,
            color: '#fff',
            flexShrink: 0,
          }}>
            TG
          </div>
          {!collapsed && (
            <span style={{
              marginLeft: 12,
              fontSize: 16,
              fontWeight: 600,
              color: '#fff',
              whiteSpace: 'nowrap',
              letterSpacing: '0.5px',
            }}>
              TG Forwarding
            </span>
          )}
        </div>

        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[selectedKey]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
          style={{
            background: 'transparent',
            border: 'none',
            marginTop: 8,
          }}
        />
      </Sider>
      <AntLayout>
        <Header style={{
          padding: '0 24px',
          background: '#fff',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid #f0f0f0',
          height: 64,
          position: 'sticky',
          top: 0,
          zIndex: 10,
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <div
              style={{
                cursor: 'pointer',
                fontSize: 18,
                color: '#6366f1',
                width: 32,
                height: 32,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                borderRadius: 8,
                transition: 'background 0.2s',
              }}
              onClick={() => setCollapsed(!collapsed)}
              onMouseEnter={e => (e.currentTarget.style.background = '#f5f3ff')}
              onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
            >
              {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            </div>
            <span style={{ fontSize: 16, fontWeight: 500, color: '#1e1b4b' }}>
              {currentTitle}
            </span>
          </div>
          <Dropdown menu={userMenu} placement="bottomRight">
            <div style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              cursor: 'pointer',
              padding: '4px 8px',
              borderRadius: 8,
              transition: 'background 0.2s',
            }}
              onMouseEnter={e => (e.currentTarget.style.background = '#f5f3ff')}
              onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
            >
              <Avatar
                size={32}
                icon={<UserOutlined />}
                style={{ background: '#6366f1' }}
              />
              <span style={{ fontSize: 14, color: '#374151' }}>
                {user?.display_name || user?.username || '用户'}
              </span>
            </div>
          </Dropdown>
        </Header>
        <Content style={{
          margin: 24,
          padding: 0,
          minHeight: 280,
        }}>
          <Outlet />
        </Content>
      </AntLayout>
    </AntLayout>
  )
}

export default Layout
