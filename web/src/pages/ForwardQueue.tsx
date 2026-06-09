import React, { useEffect, useState } from 'react'
import { Card, Row, Col, Table, Statistic, Empty, Button, Popconfirm, message, Tooltip, Space, Badge } from 'antd'
import {
  ReloadOutlined,
  ClockCircleOutlined,
  CheckCircleOutlined,
  WarningOutlined,
  RetweetOutlined,
  PictureOutlined,
} from '@ant-design/icons'
import apiClient from '../api/client'
import type { QueueStatusResponse, ForwardTask } from '../types'
import PageHeader from '../components/PageHeader'

const ForwardQueue: React.FC = () => {
  const [queue, setQueue] = useState<QueueStatusResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [retryingId, setRetryingId] = useState<number | null>(null)
  const [retryAllLoading, setRetryAllLoading] = useState(false)

  const fetchQueue = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/image-forward/queue')
      if (res.data?.success) {
        setQueue(res.data.data)
      }
    } catch {
      /* ignore */
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchQueue()
    // 30 秒轮询队列状态
    const poll = setInterval(fetchQueue, 30000)
    return () => clearInterval(poll)
  }, [])

  // 单条重试
  const handleRetry = async (id: number) => {
    setRetryingId(id)
    try {
      const res = await apiClient.post(`/image-forward/retry/${id}`)
      if (res.data?.success) {
        message.success('任务已重置为待转发')
        await fetchQueue()
      } else {
        message.error(res.data?.error || '重试失败')
      }
    } catch {
      message.error('重试请求失败')
    } finally {
      setRetryingId(null)
    }
  }

  // 全部重试
  const handleRetryAll = async () => {
    setRetryAllLoading(true)
    try {
      const res = await apiClient.post('/image-forward/retry-all')
      if (res.data?.success) {
        const n = res.data.retried ?? 0
        message.success(n > 0 ? `已重试 ${n} 个任务` : '没有需要重试的任务')
        await fetchQueue()
      } else {
        message.error('全部重试失败')
      }
    } catch {
      message.error('全部重试请求失败')
    } finally {
      setRetryAllLoading(false)
    }
  }

  // 手动刷新
  const refreshAll = () => {
    fetchQueue()
    message.success('已刷新')
  }

  const failedColumns = [
    { title: 'ID', dataIndex: 'id', width: 70 },
    {
      title: '标题',
      dataIndex: 'title',
      ellipsis: true,
      render: (t: string, r: ForwardTask) => t || r.remote_id || '-',
    },
    {
      title: '错误原因',
      dataIndex: 'error',
      width: 200,
      ellipsis: true,
      render: (e: string) =>
        e ? (
          <Tooltip title={e}>
            <span style={{ color: '#ef4444' }}>{e}</span>
          </Tooltip>
        ) : (
          '-'
        ),
    },
    {
      title: '重试次数',
      dataIndex: 'retry_count',
      width: 90,
      render: (n: number) => <Badge count={n} style={{ backgroundColor: n >= 3 ? '#ef4444' : '#f59e0b' }} />,
    },
    {
      title: '更新时间',
      dataIndex: 'updated_at',
      width: 160,
      render: (t: string) => t?.replace('T', ' ').substring(0, 19) || '-',
    },
    {
      title: '操作',
      width: 90,
      render: (_: unknown, r: ForwardTask) => (
        <Popconfirm
          title="重试此失败任务？"
          onConfirm={() => handleRetry(r.id)}
          okText="重试"
          cancelText="取消"
        >
          <Button
            type="link"
            size="small"
            icon={<RetweetOutlined />}
            loading={retryingId === r.id}
          >
            重试
          </Button>
        </Popconfirm>
      ),
    },
  ]

  const pending = queue?.pending ?? 0
  const forwarded = queue?.forwarded ?? 0
  const failed = queue?.failed ?? 0

  return (
    <div style={{ height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
      <PageHeader
        title="转发队列"
        description="图片转发队列状态与失败任务重试"
        extra={
          <Space>
            <Button icon={<ReloadOutlined />} onClick={refreshAll} loading={loading}>
              刷新
            </Button>
            <Popconfirm
              title={`重试全部 ${failed} 个失败任务？`}
              onConfirm={handleRetryAll}
              disabled={failed === 0}
              okText="全部重试"
              cancelText="取消"
            >
              <Button
                type="primary"
                icon={<RetweetOutlined />}
                loading={retryAllLoading}
                disabled={failed === 0}
              >
                全部重试{failed > 0 ? ` (${failed})` : ''}
              </Button>
            </Popconfirm>
          </Space>
        }
      />

      {/* 统计卡片 */}
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={8}>
          <Card
            loading={loading && !queue}
            style={{ borderRadius: 12 }}
          >
            <Statistic
              title="待转发"
              value={pending}
              prefix={<ClockCircleOutlined style={{ color: '#0ea5e9' }} />}
              valueStyle={{ color: '#0ea5e9' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card loading={loading && !queue} style={{ borderRadius: 12 }}>
            <Statistic
              title="已转发"
              value={forwarded}
              prefix={<CheckCircleOutlined style={{ color: '#10b981' }} />}
              valueStyle={{ color: '#10b981' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card loading={loading && !queue} style={{ borderRadius: 12 }}>
            <Statistic
              title="失败"
              value={failed}
              prefix={<WarningOutlined style={{ color: '#ef4444' }} />}
              valueStyle={{ color: failed > 0 ? '#ef4444' : undefined }}
            />
          </Card>
        </Col>
      </Row>

      {/* 失败任务表格 */}
      <Card
        title={
          <Space>
            <PictureOutlined style={{ color: '#ef4444' }} />
            <span>失败任务</span>
            {failed > 0 && (
              <Badge count={failed} overflowCount={999} style={{ backgroundColor: '#ef4444' }} />
            )}
          </Space>
        }
        size="small"
        style={{ borderRadius: 12, marginBottom: 16 }}
      >
        <Table
          dataSource={queue?.failed_tasks || []}
          columns={failedColumns}
          rowKey="id"
          loading={loading}
          pagination={false}
          size="small"
          locale={{ emptyText: <Empty description="暂无失败任务，队列运行正常" /> }}
        />
      </Card>
    </div>
  )
}

export default ForwardQueue
