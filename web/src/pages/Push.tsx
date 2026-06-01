import React, { useEffect, useState } from 'react'
import { Card, Button, Statistic, Row, Col, Table, message, Space } from 'antd'
import { RocketOutlined, ReloadOutlined } from '@ant-design/icons'
import apiClient from '../api/client'

const Push: React.FC = () => {
  const [stats, setStats] = useState({ total: 0, success: 0, failed: 0 })
  const [histories, setHistories] = useState<any[]>([])
  const [loading, setLoading] = useState(false)

  const fetchStats = async () => {
    try {
      const res = await apiClient.get('/api/push/stats')
      setStats(res.data.data ?? { total: 0, success: 0, failed: 0 })
    } catch {}
  }

  const fetchHistories = async () => {
    setLoading(true)
    try { const res = await apiClient.get('/api/push/histories'); setHistories(res.data.data?.list ?? []) }
    catch { message.error('获取推送历史失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchStats(); fetchHistories() }, [])

  const triggerPush = async () => {
    try { await apiClient.post('/api/push/trigger', {}); message.success('推送已触发'); fetchStats(); fetchHistories() }
    catch (e: any) { message.error(e.message || '推送失败') }
  }

  const retryFailed = async () => {
    try { await apiClient.post('/api/push/retry'); message.success('重试已触发'); fetchStats(); fetchHistories() }
    catch (e: any) { message.error(e.message || '重试失败') }
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '批次ID', dataIndex: 'batch_id', key: 'batch_id', width: 140, ellipsis: true },
    { title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === 'success' ? <span style={{color:'green'}}>成功</span> : <span style={{color:'red'}}>失败</span> },
    { title: '数据量', dataIndex: 'data_count', key: 'data_count', width: 80 },
    { title: '消息', dataIndex: 'message', key: 'message', ellipsis: true },
    { title: '推送时间', dataIndex: 'pushed_at', key: 'pushed_at' },
  ]

  return (
    <div>
      <h2>推送管理</h2>
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col span={8}><Card><Statistic title="总推送" value={stats.total} prefix={<RocketOutlined />} /></Card></Col>
        <Col span={8}><Card><Statistic title="成功" value={stats.success} valueStyle={{ color: '#3f8600' }} /></Card></Col>
        <Col span={8}><Card><Statistic title="失败" value={stats.failed} valueStyle={{ color: '#cf1322' }} /></Card></Col>
      </Row>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<RocketOutlined />} onClick={triggerPush}>手动推送</Button>
        <Button icon={<ReloadOutlined />} onClick={retryFailed}>重试失败</Button>
        <Button onClick={() => { fetchStats(); fetchHistories() }}>刷新</Button>
      </Space>
      <Table dataSource={histories} columns={columns} rowKey="id" loading={loading} />
    </div>
  )
}

export default Push
