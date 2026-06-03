import React, { useEffect, useState } from 'react'
import { Table, Button, Tag, Typography, message, Select, Space } from 'antd'
import { ArrowLeftOutlined } from '@ant-design/icons'
import { useParams, useLocation, useNavigate } from 'react-router-dom'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'

const { Paragraph } = Typography

interface CollectorHistory {
  id: number
  collector_id: number
  channel_id: number
  message_id: number
  post_time: string
  raw_data: string | null
  is_auto_push: boolean
  remote_id: string | null
  created_at: string
  is_extracted: boolean
}

const CollectorHistory: React.FC = () => {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  const navigate = useNavigate()
  const state = location.state as { channel_name?: string; channel_id?: number } | null

  const [data, setData] = useState<CollectorHistory[]>([])
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, pageSize: 20, total: 0 })
  const [extractedFilter, setExtractedFilter] = useState<boolean | undefined>(undefined)

  const fetch = async (page = 1) => {
    if (!id) return
    setLoading(true)
    try {
      const params: Record<string, string> = {
        collector_id: id,
        page: String(page),
        page_size: '20',
      }
      if (extractedFilter !== undefined) {
        params.is_extracted = String(extractedFilter)
      }
      const res = await apiClient.get('/collectors/histories', { params })
      setData(res.data.data?.list ?? [])
      setPagination(prev => ({ ...prev, page, total: res.data.data?.pagination?.total ?? 0 }))
    } catch { message.error('获取采集记录失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetch() }, [id, extractedFilter])

  // 解析 raw_data
  const parseRawData = (raw: string | null): { text: string; mediaType?: string } => {
    if (!raw) return { text: '(无内容)' }
    try {
      const d = JSON.parse(raw)
      return { text: d.text || '(无文本)', mediaType: d.media_type }
    } catch {
      return { text: raw.substring(0, 100) }
    }
  }

  const columns = [
    { title: '消息ID', dataIndex: 'message_id', key: 'message_id', width: 90 },
    {
      title: '内容',
      key: 'content',
      render: (_: any, r: CollectorHistory) => {
        const parsed = parseRawData(r.raw_data)
        return (
          <div>
            <Paragraph ellipsis={{ rows: 2, expandable: true, symbol: '展开' }} style={{ marginBottom: 0 }}>
              {parsed.text}
            </Paragraph>
            {parsed.mediaType && (
              <Tag color="blue" style={{ marginTop: 4 }}>
                {parsed.mediaType === 'photo' ? '图片' : parsed.mediaType === 'document' ? '文件' : parsed.mediaType}
              </Tag>
            )}
          </div>
        )
      },
    },
    {
      title: '来源', key: 'source', width: 100,
      render: (_: any, r: CollectorHistory) => (
        <Tag color={r.is_auto_push ? 'green' : '#6366f1'} style={{ margin: 0 }}>
          {r.is_auto_push ? '实时' : '手动'}
        </Tag>
      ),
    },
    {
      title: '已提取', key: 'is_extracted', width: 90,
      render: (_: any, r: CollectorHistory) => (
        <Tag color={r.is_extracted ? '#6366f1' : 'default'} style={{ margin: 0 }}>
          {r.is_extracted ? '是' : '否'}
        </Tag>
      ),
    },
    {
      title: '采集时间', dataIndex: 'post_time', key: 'post_time', width: 170,
      render: (v: string) => v ? new Date(v + 'Z').toLocaleString('zh-CN') : '-',
    },
  ]

  const titleSuffix = state?.channel_name || state?.channel_id || `#${id}`

  return (
    <div>
      <PageHeader
        title={`采集记录 — ${titleSuffix}`}
        description={`共 ${pagination.total} 条记录`}
        extra={
          <Button icon={<ArrowLeftOutlined />} onClick={() => navigate('/collectors')}>
            返回采集器
          </Button>
        }
      />

      <div style={{ marginBottom: 16 }}>
        <Space>
          <span style={{ color: '#6b7280', fontSize: 14 }}>提取状态：</span>
          <Select
            value={extractedFilter}
            onChange={setExtractedFilter}
            style={{ width: 150 }}
            allowClear
            placeholder="全部"
            options={[
              { label: '已提取', value: true },
              { label: '未提取', value: false },
            ]}
          />
        </Space>
      </div>

      <Table
        dataSource={data}
        columns={columns}
        rowKey="id"
        loading={loading}
        style={{ background: '#fff', borderRadius: 12 }}
        pagination={{
          current: pagination.page,
          total: pagination.total,
          pageSize: pagination.pageSize,
          onChange: (p) => fetch(p),
          showTotal: (t) => `共 ${t} 条`,
          showSizeChanger: false,
        }}
      />
    </div>
  )
}

export default CollectorHistory
