import React, { useEffect, useState } from 'react'
import { Table, message } from 'antd'
import apiClient from '../api/client'

const CollectorHistory: React.FC = () => {
  const [data, setData] = useState<any[]>([])
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, pageSize: 20, total: 0 })

  const fetch = async (page = 1) => {
    setLoading(true)
    try {
      const res = await apiClient.get('/api/collectors/histories', { params: { page, page_size: 20 } })
      setData(res.data.data?.list ?? [])
      setPagination(prev => ({ ...prev, page, total: res.data.data?.pagination?.total ?? 0 }))
    } catch { message.error('获取采集历史失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetch() }, [])

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 80 },
    { title: '采集器ID', dataIndex: 'collector_id', key: 'collector_id', width: 100 },
    { title: '频道ID', dataIndex: 'channel_id', key: 'channel_id', width: 140 },
    { title: '消息ID', dataIndex: 'message_id', key: 'message_id', width: 100 },
    { title: '发布时间', dataIndex: 'post_time', key: 'post_time' },
    { title: '已推送', dataIndex: 'is_auto_push', key: 'is_auto_push', width: 80,
      render: (v: boolean) => v ? '是' : '否' },
    { title: '创建时间', dataIndex: 'created_at', key: 'created_at' },
  ]

  return (
    <div>
      <h2>采集历史</h2>
      <Table dataSource={data} columns={columns} rowKey="id" loading={loading}
        pagination={{ ...pagination, onChange: (p) => fetch(p) }} />
    </div>
  )
}

export default CollectorHistory
