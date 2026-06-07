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
      color: '#6366f1',
      bg: '#eef2ff',
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
    { label: '添加客户端', icon: <ApiOutlined />, path: '/clients' },
    { label: '创建规则', icon: <SendOutlined />, path: '/rules' },
    { label: '推送管理', icon: <RocketOutlined />, path: '/push' },
    { label: '资源管理', icon: <LinkOutlined />, path: '/resources' },
  ]

  return (
    <div style={{ height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
      {/* 欢迎区域 */}
      <div style={{
        background: 'linear-gradient(135deg, #6366f1 0%, #818cf8 100%)',
        borderRadius: 16,
        padding: '32px 36px',
        marginBottom: 24,
        color: '#fff',
        position: 'relative',
        overflow: 'hidden',
      }}>
        <div style={{ position: 'absolute', right: -30, top: -30, width: 180, height: 180, borderRadius: '50%', background: 'rgba(255,255,255,0.08)' }} />
        <div style={{ position: 'absolute', right: 60, bottom: -40, width: 120, height: 120, borderRadius: '50%', background: 'rgba(255,255,255,0.05)' }} />
        <Title level={3} style={{ color: '#fff', margin: 0, fontWeight: 600 }}>
          {greeting}，欢迎回来
        </Title>
        <Text style={{ color: 'rgba(255,255,255,0.8)', fontSize: 15, marginTop: 8, display: 'block' }}>
          TG Forwarding 消息转发管理平台
          {data?.version && ` · v${data.version}`}
        </Text>
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
                  <div style={{ fontSize: 28, fontWeight: 700, color: '#1e1b4b', lineHeight: 1 }}>
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
            title={<span style={{ fontWeight: 600, color: '#1e1b4b' }}>快速操作</span>}
            style={{ borderRadius: 12 }}
            styles={{ body: { padding: '16px 24px' } }}
          >
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
              {quickActions.map((action) => (
                <Button
                  key={action.label}
                  icon={action.icon}
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
            title={<span style={{ fontWeight: 600, color: '#1e1b4b' }}>调度任务</span>}
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
