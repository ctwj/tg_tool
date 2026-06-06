import React from 'react'
import { Table, Tag } from 'antd'
import PageHeader from '../components/PageHeader'

interface ApiEndpoint {
  method: string
  path: string
  description: string
  auth: string
}

const endpoints: ApiEndpoint[] = [
  {
    method: 'GET',
    path: '/api/status',
    description: '系统健康检查，返回版本号、数据库状态、客户端统计。数据库异常时返回 503',
    auth: '公开',
  },
]

const methodColor: Record<string, string> = {
  GET: 'green',
  POST: 'blue',
  PUT: 'orange',
  DELETE: 'red',
}

const ApiStatus: React.FC = () => {
  const columns = [
    {
      title: '方法',
      dataIndex: 'method',
      width: 80,
      render: (method: string) => (
        <Tag color={methodColor[method]} style={{ fontFamily: 'monospace', fontWeight: 600 }}>
          {method}
        </Tag>
      ),
    },
    {
      title: '端点路径',
      dataIndex: 'path',
      width: 240,
      render: (path: string) => (
        <code style={{ background: '#f5f3ff', padding: '2px 8px', borderRadius: 4, fontSize: 13, color: '#6366f1' }}>
          {path}
        </code>
      ),
    },
    {
      title: '说明',
      dataIndex: 'description',
    },
    {
      title: '认证',
      dataIndex: 'auth',
      width: 80,
      render: (auth: string) => (
        <Tag color={auth === '公开' ? 'cyan' : 'default'}>{auth}</Tag>
      ),
    },
  ]

  return (
    <div style={{ height: '100%', overflowY: 'auto' }}>
      <PageHeader title="API 状态" description="系统对外暴露的 API 端点列表" />
      <Table
        dataSource={endpoints}
        columns={columns}
        rowKey="path"
        pagination={false}
        style={{ background: '#fff', borderRadius: 12 }}
      />
    </div>
  )
}

export default ApiStatus
