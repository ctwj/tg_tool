import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Tag, Popconfirm, Spin, Typography, Pagination, Tooltip, Statistic, Row, Col, Card } from 'antd'
import { ThunderboltOutlined, EditOutlined, DeleteOutlined, ReloadOutlined, BarChartOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { ExtractedResource, ResourceStats, ExtractionResult } from '../types'

const { Text } = Typography

const Resources: React.FC = () => {
  const [resources, setResources] = useState<ExtractedResource[]>([])
  const [loading, setLoading] = useState(false)
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined)
  const [categoryFilter, setCategoryFilter] = useState<string | undefined>(undefined)

  // 编辑弹窗
  const [editModalOpen, setEditModalOpen] = useState(false)
  const [editForm] = Form.useForm()
  const [editingId, setEditingId] = useState<number | null>(null)

  // 统计
  const [stats, setStats] = useState<ResourceStats | null>(null)
  const [statsVisible, setStatsVisible] = useState(false)

  // 提取中
  const [extracting, setExtracting] = useState(false)

  // 网盘类型选项
  const categoryOptions = [
    { label: '夸克网盘', value: 'quark' },
    { label: '阿里云盘', value: 'aliyun' },
    { label: '百度网盘', value: 'baidu' },
    { label: 'UC网盘', value: 'uc' },
    { label: '115网盘', value: '115' },
    { label: '123网盘', value: '123pan' },
    { label: '天翼网盘', value: 'tianyi' },
    { label: '迅雷网盘', value: 'xunlei' },
  ]

  const categoryLabel = (cat?: string) => {
    const found = categoryOptions.find(o => o.value === cat)
    return found?.label || cat || '未知'
  }

  // 加载资源列表
  const fetchResources = async () => {
    setLoading(true)
    try {
      const params = new URLSearchParams()
      params.set('page', String(page))
      params.set('page_size', String(pageSize))
      if (statusFilter) params.set('status', statusFilter)
      if (categoryFilter) params.set('category', categoryFilter)

      const resp = await apiClient.get(`/resources?${params}`)
      if (resp.data?.success) {
        setResources(resp.data.data?.list || [])
        setTotal(resp.data.data?.pagination?.total || 0)
      }
    } catch {
      message.error('加载资源列表失败')
    } finally {
      setLoading(false)
    }
  }

  // 加载统计
  const fetchStats = async () => {
    try {
      const resp = await apiClient.get('/resources/stats')
      if (resp.data?.success) {
        setStats(resp.data.data)
      }
    } catch {
      // ignore
    }
  }

  useEffect(() => {
    fetchResources()
  }, [page, pageSize, statusFilter, categoryFilter])

  // 触发提取
  const handleExtract = async () => {
    setExtracting(true)
    try {
      const resp = await apiClient.post('/resources/extract', { batch_size: 1000 })
      if (resp.data?.success) {
        const result: ExtractionResult = resp.data.data
        message.success(`提取完成：扫描 ${result.total_scanned} 条，提取 ${result.extracted} 条，跳过 ${result.skipped} 条`)
        fetchResources()
        fetchStats()
      } else {
        message.error(resp.data?.message || '提取失败')
      }
    } catch {
      message.error('提取请求失败')
    } finally {
      setExtracting(false)
    }
  }

  // 编辑资源
  const handleEdit = (record: ExtractedResource) => {
    setEditingId(record.id)
    editForm.setFieldsValue({
      title: record.title,
      description: record.description,
      tags: record.tags,
      category: record.category,
    })
    setEditModalOpen(true)
  }

  const handleEditSubmit = async () => {
    if (!editingId) return
    try {
      const values = await editForm.validateFields()
      const resp = await apiClient.put(`/resources/${editingId}`, values)
      if (resp.data?.success) {
        message.success('资源已更新')
        setEditModalOpen(false)
        fetchResources()
      }
    } catch {
      message.error('更新失败')
    }
  }

  // 删除资源
  const handleDelete = async (id: number) => {
    try {
      const resp = await apiClient.delete(`/resources/${id}`)
      if (resp.data?.success) {
        message.success('资源已删除')
        fetchResources()
        fetchStats()
      }
    } catch {
      message.error('删除失败')
    }
  }

  const columns = [
    {
      title: '标题',
      dataIndex: 'title',
      key: 'title',
      width: 300,
      ellipsis: true,
      render: (text: string) => <Text strong>{text}</Text>,
    },
    {
      title: '网盘类型',
      dataIndex: 'category',
      key: 'category',
      width: 120,
      render: (cat: string) => (
        <Tag color="blue">{categoryLabel(cat)}</Tag>
      ),
    },
    {
      title: '提取模式',
      dataIndex: 'extract_mode',
      key: 'extract_mode',
      width: 100,
      render: (mode: string) => (
        <Tag color={mode === 'ai' ? 'purple' : 'green'}>{mode === 'ai' ? 'AI' : '规则'}</Tag>
      ),
    },
    {
      title: '标签',
      dataIndex: 'tags',
      key: 'tags',
      width: 150,
      ellipsis: true,
      render: (tags: string) =>
        tags ? tags.split(',').map((t, i) => <Tag key={i}>{t}</Tag>) : '-',
    },
    {
      title: '推送状态',
      dataIndex: 'is_pushed',
      key: 'is_pushed',
      width: 100,
      render: (pushed: boolean) => pushed ? <Tag color="green">已推送</Tag> : <Tag color="orange">未推送</Tag>,
    },
    {
      title: '已编辑',
      dataIndex: 'is_edited',
      key: 'is_edited',
      width: 80,
      render: (edited: boolean) => edited ? <Tag color="cyan">是</Tag> : '-',
    },
    {
      title: '时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 170,
      render: (t: string) => t?.replace('T', ' ')?.substring(0, 19) || '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 120,
      render: (_: unknown, record: ExtractedResource) => (
        <Space>
          <Tooltip title="编辑">
            <Button size="small" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          </Tooltip>
          <Popconfirm title="确定删除此资源？" onConfirm={() => handleDelete(record.id)}>
            <Tooltip title="删除">
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div style={{ padding: 24 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Typography.Title level={3} style={{ margin: 0 }}>资源管理</Typography.Title>
        <Space>
          <Select
            placeholder="推送状态"
            allowClear
            style={{ width: 120 }}
            value={statusFilter}
            onChange={setStatusFilter}
            options={[
              { label: '未推送', value: 'unpushed' },
              { label: '已推送', value: 'pushed' },
              { label: '全部', value: 'all' },
            ]}
          />
          <Select
            placeholder="网盘类型"
            allowClear
            style={{ width: 130 }}
            value={categoryFilter}
            onChange={setCategoryFilter}
            options={categoryOptions}
          />
          <Tooltip title="统计">
            <Button icon={<BarChartOutlined />} onClick={() => { setStatsVisible(!statsVisible); fetchStats() }} />
          </Tooltip>
          <Button icon={<ReloadOutlined />} onClick={fetchResources}>刷新</Button>
          <Button type="primary" icon={<ThunderboltOutlined />} loading={extracting} onClick={handleExtract}>
            触发提取
          </Button>
        </Space>
      </div>

      {statsVisible && stats && (
        <Card size="small" style={{ marginBottom: 16 }}>
          <Row gutter={16}>
            <Col span={6}><Statistic title="总资源" value={stats.total} /></Col>
            <Col span={6}><Statistic title="已推送" value={stats.pushed} valueStyle={{ color: '#52c41a' }} /></Col>
            <Col span={6}><Statistic title="未推送" value={stats.unpushed} valueStyle={{ color: '#faad14' }} /></Col>
            <Col span={6}>
              <div>
                <Text type="secondary">按类型</Text>
                <div>{Object.entries(stats.by_category || {}).map(([k, v]) => (
                  <Tag key={k}>{categoryLabel(k)}: {v}</Tag>
                ))}</div>
              </div>
            </Col>
          </Row>
        </Card>
      )}

      <Spin spinning={loading}>
        <Table
          dataSource={resources}
          columns={columns}
          rowKey="id"
          pagination={false}
          size="middle"
          scroll={{ x: 1200 }}
        />
      </Spin>

      <div style={{ textAlign: 'right', marginTop: 16 }}>
        <Pagination
          current={page}
          pageSize={pageSize}
          total={total}
          showTotal={(t) => `共 ${t} 条`}
          showSizeChanger
          onChange={(p, ps) => { setPage(p); setPageSize(ps) }}
        />
      </div>

      <Modal
        title="编辑资源"
        open={editModalOpen}
        onOk={handleEditSubmit}
        onCancel={() => setEditModalOpen(false)}
        okText="保存"
      >
        <Form form={editForm} layout="vertical">
          <Form.Item name="title" label="标题" rules={[{ required: true, message: '标题不能为空' }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="tags" label="标签（逗号分隔）">
            <Input />
          </Form.Item>
          <Form.Item name="category" label="网盘类型">
            <Select options={categoryOptions} allowClear />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Resources
