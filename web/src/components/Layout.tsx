import React, { useState, useEffect } from 'react'
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
  FieldTimeOutlined,
  PictureOutlined,
} from '@ant-design/icons'
import { useNavigate, useLocation, Outlet } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'

const { Header, Sider, Content } = AntLayout

// 二级菜单结构：仪表盘 + 运行监控（仪表盘下）+ 业务分组
const menuItems = [
  { key: '/dashboard', icon: <DashboardOutlined />, label: '仪表盘' },
  {
    key: 'group-monitor',
    icon: <FieldTimeOutlined />,
    label: '运行监控',
    children: [
      { key: '/scheduler', icon: <FieldTimeOutlined />, label: '调度监控' },
      { key: '/forward-queue', icon: <PictureOutlined />, label: '转发队列' },
    ],
  },
  {
    key: 'group-collect',
    icon: <CloudDownloadOutlined />,
    label: '消息采集',
    children: [
      { key: '/clients', icon: <ApiOutlined />, label: '客户端管理' },
      { key: '/collectors', icon: <CloudDownloadOutlined />, label: '采集器' },
      { key: '/resources', icon: <DatabaseOutlined />, label: '资源管理' },
      { key: '/push', icon: <RocketOutlined />, label: '推送管理' },
    ],
  },
  {
    key: 'group-tgtool',
    icon: <SendOutlined />,
    label: 'TG工具',
    children: [
      { key: '/rules', icon: <SendOutlined />, label: '转发规则' },
    ],
  },
  {
    key: 'group-system',
    icon: <SettingOutlined />,
    label: '系统管理',
    children: [
      { key: '/files', icon: <FileOutlined />, label: '文件管理' },
      { key: '/users', icon: <UserOutlined />, label: '用户管理' },
      { key: '/settings', icon: <SettingOutlined />, label: '系统设置' },
      { key: '/api-status', icon: <ApiOutlined />, label: 'API 状态' },
    ],
  },
]

// 路由 → 所属分组映射（用于自动展开对应二级菜单）
const PATH_TO_GROUP: Record<string, string> = {
  '/scheduler': 'group-monitor',
  '/forward-queue': 'group-monitor',
  '/rules': 'group-tgtool',
  '/clients': 'group-collect',
  '/collectors': 'group-collect',
  '/resources': 'group-collect',
  '/push': 'group-collect',
  '/files': 'group-system',
  '/users': 'group-system',
  '/settings': 'group-system',
  '/api-status': 'group-system',
}

const pageTitles: Record<string, string> = {
  '/dashboard': '仪表盘',
  '/clients': '客户端管理',
  '/rules': '转发规则',
  '/collectors': '采集器管理',
  '/push': '推送管理',
  '/scheduler': '调度监控',
  '/forward-queue': '转发队列',
  '/resources': '资源管理',
  '/users': '用户管理',
  '/files': '文件管理',
  '/settings': '系统设置',
  '/api-status': 'API 状态',
}

// 根据路径获取当前标题（支持子路径）
const getTitle = (pathname: string) => {
  if (pathname.startsWith('/collectors/') && pathname.endsWith('/history')) return '采集记录'
  return pageTitles[pathname] || 'TG tools'
}

// 根据路径获取侧边栏选中的 key（子路径选中父菜单项）
const getMenuKey = (pathname: string) => {
  if (pathname.startsWith('/collectors/')) return '/collectors'
  return pathname
}

// 根据路径获取应展开的分组 key
const getOpenGroup = (pathname: string): string[] => {
  if (pathname.startsWith('/collectors/')) return ['group-collect']
  const g = PATH_TO_GROUP[pathname]
  return g ? [g] : []
}

// 菜单视觉与交互优化：选中强调条、统一高度、分组标题、hover 微动效、无障碍
const menuStyle = `
/* 统一菜单项高度（触摸友好 42px）+ 圆角 + 间距节奏 */
.sidebar-menu.ant-menu-dark .ant-menu-item,
.sidebar-menu.ant-menu-dark .ant-menu-submenu-title {
  height: 42px !important;
  line-height: 42px !important;
  border-radius: 8px;
  margin: 3px 10px !important;
  width: calc(100% - 20px) !important;
  transition: background 0.2s ease, color 0.2s ease, transform 0.15s ease;
}
/* hover：背景渐变 + 轻微右移 + 图标放大 */
.sidebar-menu.ant-menu-dark .ant-menu-item:hover,
.sidebar-menu.ant-menu-dark .ant-menu-submenu-title:hover {
  background: rgba(255, 255, 255, 0.07) !important;
  transform: translateX(2px);
}
/* 选中态：紫色渐变背景 + 左侧强调条（现代侧边栏标志性设计） */
.sidebar-menu.ant-menu-dark .ant-menu-item-selected {
  background: linear-gradient(90deg, rgba(14, 165, 233, 0.3), rgba(14, 165, 233, 0.05)) !important;
  font-weight: 500;
  position: relative;
}
.sidebar-menu.ant-menu-dark .ant-menu-item-selected::before {
  content: '';
  position: absolute;
  left: -10px;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 22px;
  background: linear-gradient(180deg, #7dd3fc, #0ea5e9);
  border-radius: 0 3px 3px 0;
  box-shadow: 0 0 8px rgba(125, 211, 252, 0.6);
}
/* 分组标题增强：字重 + 字间距，层级清晰 */
.sidebar-menu.ant-menu-dark .ant-menu-submenu > .ant-menu-submenu-title {
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.3px;
}
.sidebar-menu.ant-menu-dark .ant-menu-submenu-selected > .ant-menu-submenu-title {
  color: #bae6fd !important;
}
/* 图标动效：hover 时与文字联动 */
.sidebar-menu.ant-menu-dark .ant-menu-item .anticon,
.sidebar-menu.ant-menu-dark .ant-menu-submenu-title .anticon {
  transition: transform 0.2s ease;
}
.sidebar-menu.ant-menu-dark .ant-menu-item:hover .anticon,
.sidebar-menu.ant-menu-dark .ant-menu-submenu-title:hover .anticon {
  transform: scale(1.12);
}
/* 二级菜单项：缩进节奏 + 较小高度 + 较小字号 */
.sidebar-menu.ant-menu-dark .ant-menu-sub .ant-menu-item {
  height: 38px !important;
  line-height: 38px !important;
  margin: 2px 10px 2px 30px !important;
  width: calc(100% - 40px) !important;
  font-size: 13px;
}
/* 二级菜单选中强调条对齐缩进 */
.sidebar-menu.ant-menu-dark .ant-menu-sub .ant-menu-item-selected::before {
  left: -30px;
}
/* 展开箭头过渡 */
.sidebar-menu.ant-menu-dark .ant-menu-submenu-arrow {
  transition: transform 0.2s ease;
}
/* 收起态 tooltip 文字优化 */
.ant-tooltip-inner {
  font-size: 13px;
  border-radius: 8px;
}
/* 无障碍：尊重用户 reduced-motion 偏好 */
@media (prefers-reduced-motion: reduce) {
  .sidebar-menu.ant-menu-dark *,
  .sidebar-menu.ant-menu-dark *::before {
    transition: none !important;
    transform: none !important;
  }
}
`

const Layout: React.FC = () => {
  const [collapsed, setCollapsed] = useState(false)
  const navigate = useNavigate()
  const location = useLocation()
  const { user, logout } = useAuth()

  const currentTitle = getTitle(location.pathname)
  const selectedKey = getMenuKey(location.pathname)
  // 受控展开的分组：默认展开当前路由所属分组 + 用户手动展开的分组
  const [openKeys, setOpenKeys] = useState<string[]>(() => getOpenGroup(location.pathname))

  // 路由变化时，确保当前路由所属分组已展开
  useEffect(() => {
    const group = getOpenGroup(location.pathname)
    setOpenKeys(prev => {
      const merged = new Set([...prev, ...group])
      return Array.from(merged)
    })
  }, [location.pathname])

  const onOpenChange = (keys: string[]) => {
    setOpenKeys(keys)
  }

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
    <AntLayout style={{ height: '100vh', overflow: 'hidden' }}>
      <style>{menuStyle}</style>
      <Sider
        trigger={null}
        collapsible
        collapsed={collapsed}
        width={240}
        style={{
          background: 'linear-gradient(180deg, #0c4a6e 0%, #075985 100%)',
          boxShadow: '2px 0 8px rgba(0,0,0,0.15)',
          overflowY: 'auto',
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
            background: 'linear-gradient(135deg, #0088FF, #4FC3F7)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
          }}>
            <img src="/logo.svg" alt="TG tools" style={{ width: 22, height: 22 }} />
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
              TG tools
            </span>
          )}
        </div>

        <Menu
          className="sidebar-menu"
          theme="dark"
          mode="inline"
          selectedKeys={[selectedKey]}
          openKeys={collapsed ? [] : openKeys}
          onOpenChange={onOpenChange}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
          style={{
            background: 'transparent',
            border: 'none',
            marginTop: 8,
          }}
        />
      </Sider>
      <AntLayout style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        <Header style={{
          padding: '0 24px',
          background: '#fff',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid #f0f0f0',
          height: 64,
          flexShrink: 0,
          zIndex: 10,
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <div
              style={{
                cursor: 'pointer',
                fontSize: 18,
                color: '#0ea5e9',
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
            <span style={{ fontSize: 16, fontWeight: 500, color: '#0c4a6e' }}>
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
                style={{ background: '#0ea5e9' }}
              />
              <span style={{ fontSize: 14, color: '#374151' }}>
                {user?.display_name || user?.username || '用户'}
              </span>
            </div>
          </Dropdown>
        </Header>
        <Content style={{
          flex: 1,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          padding: '20px 24px',
        }}>
          <Outlet />
        </Content>
      </AntLayout>
    </AntLayout>
  )
}

export default Layout
