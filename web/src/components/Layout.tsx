import React, { useState } from 'react'
import { Layout as AntLayout, Menu, Avatar, Dropdown, theme } from 'antd'
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

const Layout: React.FC = () => {
  const [collapsed, setCollapsed] = useState(false)
  const navigate = useNavigate()
  const location = useLocation()
  const { user, logout } = useAuth()
  const { token: { colorBgContainer, borderRadiusLG } } = theme.useToken()

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
        onClick: () => {
          logout()
          navigate('/login')
        },
      },
    ],
  }

  return (
    <AntLayout style={{ minHeight: '100vh' }}>
      <Sider trigger={null} collapsible collapsed={collapsed}>
        <div style={{
          height: 32,
          margin: 16,
          color: '#fff',
          fontSize: collapsed ? 14 : 18,
          fontWeight: 'bold',
          textAlign: 'center',
          lineHeight: '32px',
          overflow: 'hidden',
        }}>
          {collapsed ? 'TG' : 'TG Forwarding'}
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <AntLayout>
        <Header style={{
          padding: '0 24px',
          background: colorBgContainer,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}>
          <div style={{ cursor: 'pointer', fontSize: 18 }} onClick={() => setCollapsed(!collapsed)}>
            {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
          </div>
          <Dropdown menu={userMenu} placement="bottomRight">
            <Avatar icon={<UserOutlined />} style={{ cursor: 'pointer' }} />
          </Dropdown>
        </Header>
        <Content style={{
          margin: 24,
          padding: 24,
          background: colorBgContainer,
          borderRadius: borderRadiusLG,
          minHeight: 280,
        }}>
          <Outlet />
        </Content>
      </AntLayout>
    </AntLayout>
  )
}

export default Layout
