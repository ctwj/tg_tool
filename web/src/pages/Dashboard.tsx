import React, { useEffect, useState } from 'react'
import { Card, Col, Row, Typography, Tag, Space, Button } from 'antd'
import { ApiOutlined, SendOutlined, CloudDownloadOutlined, RocketOutlined, CheckCircleOutlined, LinkOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import apiClient from '../api/client'

const { Title, Text } = Typography

interface DashboardData {
  clients: { total: number; active: number; new: number; offline: number }
  rules: { total: number; active: number }
  collectors: { total: number; active: number }
  version: string
  schedulers?: {
    extract_running: boolean
    extract_next_run: string | null
    push_running: boolean
    push_next_run: string | null
  }
}

const Dashboard: React.FC = () => {
  const [data, setData] = useState<DashboardData | null>(null)
  const navigate = useNavigate()

  useEffect(() => {
    apiClient.get('/status').then(res => setData(res.data.data)).catch(() => {})
  }, [])

  const hour = new Date().getHours()
  const greeting = hour < 12 ? '上午好' : hour < 18 ? '下午好' : '晚上好'

  const statsCards = [
    {
      title: '客户端总数',
      value: data?.clients.total ?? 0,
      icon: <ApiOutlined />,
      color: '#0ea5e9',
      bg: '#e0f2fe',
    },
    {
      title: '在线客户端',
      value: data?.clients.active ?? 0,
      icon: <CheckCircleOutlined />,
      color: '#10b981',
      bg: '#ecfdf5',
    },
    {
      title: '转发规则',
      value: data?.rules?.total ?? 0,
      icon: <SendOutlined />,
      color: '#f59e0b',
      bg: '#fffbeb',
    },
    {
      title: '采集器',
      value: data?.collectors?.total ?? 0,
      icon: <CloudDownloadOutlined />,
      color: '#06b6d4',
      bg: '#ecfeff',
    },
  ]

  const quickActions = [
    { label: '添加客户端', icon: <ApiOutlined />, path: '/clients', color: '#0ea5e9' },
    { label: '创建规则', icon: <SendOutlined />, path: '/rules', color: '#f59e0b' },
    { label: '推送管理', icon: <RocketOutlined />, path: '/push', color: '#06b6d4' },
    { label: '资源管理', icon: <LinkOutlined />, path: '/resources', color: '#10b981' },
  ]

  return (
    <div style={{ height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
      {/* 欢迎区域 v2 — 左侧品牌问候 + 右侧关键指标 */}
      <div style={{
        background: 'linear-gradient(135deg, #0284c7 0%, #0ea5e9 50%, #38bdf8 100%)',
        borderRadius: 16,
        padding: '28px 32px',
        marginBottom: 24,
        color: '#fff',
        position: 'relative',
        overflow: 'hidden',
        // 多层光晕 + 细网格点装饰，提升精致感
        backgroundImage: `
          radial-gradient(circle at 88% 20%, rgba(255,255,255,0.18) 0%, transparent 35%),
          radial-gradient(circle at 95% 90%, rgba(255,255,255,0.1) 0%, transparent 30%),
          linear-gradient(135deg, #0284c7 0%, #0ea5e9 50%, #38bdf8 100%)
        `,
      }}>
        {/* 网格点纹理覆盖层 */}
        <div style={{
          position: 'absolute', inset: 0,
          backgroundImage: 'radial-gradient(rgba(255,255,255,0.12) 1px, transparent 1px)',
          backgroundSize: '20px 20px',
          opacity: 0.4,
          pointerEvents: 'none',
        }} />
        <div style={{ position: 'relative', display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 20 }}>
          {/* 左侧：品牌问候 */}
          <div style={{ flex: 1, minWidth: 260 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 10 }}>
              <div style={{
                width: 44, height: 44, borderRadius: 12,
                background: 'rgba(255,255,255,0.2)',
                backdropFilter: 'blur(8px)',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                fontSize: 20, fontWeight: 700, color: '#fff',
                border: '1px solid rgba(255,255,255,0.3)',
              }}>
                TG
              </div>
              <div>
                <Title level={3} style={{ color: '#fff', margin: 0, fontWeight: 600, lineHeight: 1.2 }}>
                  {greeting}，欢迎回来 👋
                </Title>
                <div style={{ fontSize: 13, color: 'rgba(255,255,255,0.85)', marginTop: 2 }}>
                  TG tools · TG工具箱
                  {data?.version && (
                    <span style={{
                      marginLeft: 8, padding: '1px 8px', borderRadius: 10,
                      background: 'rgba(255,255,255,0.2)', fontSize: 12,
                    }}>
                      v{data.version}
                    </span>
                  )}
                </div>
              </div>
            </div>
          </div>
          {/* 右侧：关键指标玻璃卡片 */}
          <div style={{ display: 'flex', gap: 12 }}>
            <div style={{
              padding: '12px 18px', borderRadius: 12,
              background: 'rgba(255,255,255,0.15)', backdropFilter: 'blur(8px)',
              border: '1px solid rgba(255,255,255,0.25)', minWidth: 120,
            }}>
              <div style={{ fontSize: 12, color: 'rgba(255,255,255,0.85)', display: 'flex', alignItems: 'center', gap: 6 }}>
                <ApiOutlined /> 在线客户端
              </div>
              <div style={{ fontSize: 24, fontWeight: 700, marginTop: 4, lineHeight: 1 }}>
                {data?.clients.active ?? 0}
                <span style={{ fontSize: 14, fontWeight: 400, opacity: 0.8 }}> / {data?.clients.total ?? 0}</span>
              </div>
            </div>
            <div style={{
              padding: '12px 18px', borderRadius: 12,
              background: 'rgba(255,255,255,0.15)', backdropFilter: 'blur(8px)',
              border: '1px solid rgba(255,255,255,0.25)', minWidth: 120,
            }}>
              <div style={{ fontSize: 12, color: 'rgba(255,255,255,0.85)', display: 'flex', alignItems: 'center', gap: 6 }}>
                <RocketOutlined /> 运行调度
              </div>
              <div style={{ fontSize: 24, fontWeight: 700, marginTop: 4, lineHeight: 1 }}>
                {(data?.schedulers?.extract_running ? 1 : 0) + (data?.schedulers?.push_running ? 1 : 0)}
                <span style={{ fontSize: 14, fontWeight: 400, opacity: 0.8 }}> / 2</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 统计卡片 */}
      <Row gutter={16} style={{ marginBottom: 24 }}>
        {statsCards.map((item) => (
          <Col span={6} key={item.title}>
            <Card
              style={{ borderRadius: 12 }}
              styles={{ body: { padding: '20px 24px' } }}
              hoverable
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
                <div className="stat-icon" style={{ background: item.bg, color: item.color }}>
                  {item.icon}
                </div>
                <div>
                  <div style={{ fontSize: 13, color: '#6b7280', marginBottom: 4 }}>{item.title}</div>
                  <div style={{ fontSize: 28, fontWeight: 700, color: '#0c4a6e', lineHeight: 1 }}>
                    {item.value}
                  </div>
                </div>
              </div>
            </Card>
          </Col>
        ))}
      </Row>

      {/* 快速操作 + 系统状态 */}
      <Row gutter={16}>
        <Col span={12}>
          <Card
            title={<span style={{ fontWeight: 600, color: '#0c4a6e' }}>快速操作</span>}
            style={{ borderRadius: 12 }}
            styles={{ body: { padding: '16px 24px' } }}
          >
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
              {quickActions.map((action) => (
                <Button
                  key={action.label}
                  icon={<span style={{ color: action.color }}>{action.icon}</span>}
                  size="large"
                  block
                  onClick={() => navigate(action.path)}
                  style={{
                    height: 52,
                    borderRadius: 10,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 8,
                    fontSize: 14,
                    borderColor: '#e5e7eb',
                  }}
                >
                  {action.label}
                </Button>
              ))}
            </div>
          </Card>
        </Col>
        <Col span={12}>
          <Card
            title={<span style={{ fontWeight: 600, color: '#0c4a6e' }}>调度任务</span>}
            style={{ borderRadius: 12 }}
            styles={{ body: { padding: '16px 24px' } }}
          >
            <Space direction="vertical" style={{ width: '100%' }} size={16}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <Space>
                  <CloudDownloadOutlined style={{ color: '#06b6d4', fontSize: 18 }} />
                  <Text strong>自动提取</Text>
                </Space>
                {data?.schedulers?.extract_running ? (
                  <Tag color="green">运行中</Tag>
                ) : (
                  <Tag>未启用</Tag>
                )}
              </div>
              {data?.schedulers?.extract_running && data.schedulers.extract_next_run && (
                <Text type="secondary" style={{ fontSize: 13 }}>
                  下次执行: {data.schedulers.extract_next_run}
                </Text>
              )}
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <Space>
                  <RocketOutlined style={{ color: '#f59e0b', fontSize: 18 }} />
                  <Text strong>自动推送</Text>
                </Space>
                {data?.schedulers?.push_running ? (
                  <Tag color="green">运行中</Tag>
                ) : (
                  <Tag>未启用</Tag>
                )}
              </div>
              {data?.schedulers?.push_running && data.schedulers.push_next_run && (
                <Text type="secondary" style={{ fontSize: 13 }}>
                  下次执行: {data.schedulers.push_next_run}
                </Text>
              )}
            </Space>
          </Card>
        </Col>
      </Row>
    </div>
  )
}

export default Dashboard
