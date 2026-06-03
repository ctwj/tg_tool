import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Tag, Popconfirm, Spin, Typography, Pagination, Tooltip, Statistic, Row, Col, Card, Switch, InputNumber, Divider } from 'antd'
import { ThunderboltOutlined, EditOutlined, DeleteOutlined, ReloadOutlined, BarChartOutlined, SettingOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { ExtractedResource, ResourceStats, ExtractionResult } from '../types'
import PageHeader from '../components/PageHeader'

const { Text } = Typography

const DEFAULT_AI_PROMPT = `从以下 Telegram 消息中提取结构化资源信息。请返回 JSON 格式：{"title":"资源标题","url":["链接列表"],"description":"描述","category":"网盘类型","tags":"标签,逗号分隔"}

消息内容：`

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

  // 提取配置
  const [extractConfig, setExtractConfig] = useState({
    extract_mode: 'rule',
    auto_extract: false,
    extract_interval: 30,
    ai_prompt: '',
  })
  const [extractSaving, setExtractSaving] = useState(false)
  const [configVisible, setConfigVisible] = useState(false)

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

  // 加载提取配置
  useEffect(() => {
    const fetchExtractConfig = async () => {
      try {
        const res = await apiClient.get('/options')
        const data = res.data.data ?? {}
        setExtractConfig({
          extract_mode: data.push_extract_mode || 'rule',
          auto_extract: data.push_auto_extract === '1' || data.push_auto_extract === 'true',
          extract_interval: parseInt(data.push_extract_interval || '30', 10),
          ai_prompt: data.push_ai_prompt || '',
        })
      } catch { /* ignore */ }
    }
    fetchExtractConfig()
  }, [])

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

  // 保存提取配置
  const saveExtractConfig = async () => {
    setExtractSaving(true)
    try {
      await apiClient.put('/push/extract-config', {
        extract_mode: extractConfig.extract_mode,
        auto_extract: extractConfig.auto_extract ? '1' : '0',
        extract_interval: String(extractConfig.extract_interval),
        ai_prompt: extractConfig.ai_prompt,
      })
      message.success('提取配置已保存')
      setConfigVisible(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setExtractSaving(false)
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
      render: (text: string) => <Text strong style={{ color: '#1e1b4b' }}>{text}</Text>,
    },
    {
      title: '网盘类型',
      dataIndex: 'category',
      key: 'category',
      width: 120,
      render: (cat: string) => (
        <Tag color="#6366f1" style={{ margin: 0 }}>{categoryLabel(cat)}</Tag>
      ),
    },
    {
      title: '提取模式',
      dataIndex: 'extract_mode',
      key: 'extract_mode',
      width: 100,
      render: (mode: string) => (
        <Tag color={mode === 'ai' ? '#8b5cf6' : '#10b981'} style={{ margin: 0 }}>{mode === 'ai' ? 'AI' : '规则'}</Tag>
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
      render: (pushed: boolean) => pushed ? <Tag color="green" style={{ margin: 0 }}>已推送</Tag> : <Tag color="orange" style={{ margin: 0 }}>未推送</Tag>,
    },
    {
      title: '已编辑',
      dataIndex: 'is_edited',
      key: 'is_edited',
      width: 80,
      render: (edited: boolean) => edited ? <Tag color="cyan" style={{ margin: 0 }}>是</Tag> : '-',
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
        <Space size={4}>
          <Tooltip title="编辑">
            <Button size="small" type="text" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          </Tooltip>
          <Popconfirm title="确定删除此资源？" onConfirm={() => handleDelete(record.id)}>
            <Tooltip title="删除">
              <Button size="small" type="text" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title="资源管理"
        description="管理从 Telegram 消息中提取的资源"
        extra={
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
            <Button icon={<SettingOutlined />} onClick={() => setConfigVisible(true)}>提取配置</Button>
            <Button icon={<ReloadOutlined />} onClick={fetchResources}>刷新</Button>
            <Button type="primary" icon={<ThunderboltOutlined />} loading={extracting} onClick={handleExtract}>
              触发提取
            </Button>
          </Space>
        }
      />

      {statsVisible && stats && (
        <Card size="small" style={{ marginBottom: 16, borderRadius: 12 }}>
          <Row gutter={16}>
            <Col span={6}><Statistic title="总资源" value={stats.total} /></Col>
            <Col span={6}><Statistic title="已推送" value={stats.pushed} valueStyle={{ color: '#10b981' }} /></Col>
            <Col span={6}><Statistic title="未推送" value={stats.unpushed} valueStyle={{ color: '#f59e0b' }} /></Col>
            <Col span={6}>
              <div>
                <Text type="secondary">按类型</Text>
                <div>{Object.entries(stats.by_category || {}).map(([k, v]) => (
                  <Tag key={k} style={{ margin: 2 }}>{categoryLabel(k)}: {v}</Tag>
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
          style={{ background: '#fff', borderRadius: 12 }}
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

      {/* 提取配置弹窗 */}
      <Modal
        title="提取配置"
        open={configVisible}
        onCancel={() => setConfigVisible(false)}
        footer={null}
        width={560}
      >
        <div style={{ marginTop: 16 }}>
          <div style={{ marginBottom: 20 }}>
            <div style={{ marginBottom: 8, fontWeight: 500 }}>提取模式</div>
            <Space>
              <Select
                value={extractConfig.extract_mode}
                onChange={v => setExtractConfig({ ...extractConfig, extract_mode: v })}
                style={{ width: 200 }}
                options={[
                  { label: '规则提取（推荐）', value: 'rule' },
                  { label: 'AI 增强', value: 'ai' },
                ]}
              />
              {extractConfig.extract_mode === 'ai' && (
                <Tag color="purple">AI 模式已启用</Tag>
              )}
            </Space>
            <div style={{ fontSize: 12, color: '#999', marginTop: 4 }}>
              规则提取：基于正则匹配，速度快；AI 增强：调用大模型，提取质量更高
            </div>
          </div>

          <Divider style={{ margin: '16px 0' }} />

          <div style={{ marginBottom: 20 }}>
            <div style={{ marginBottom: 8, fontWeight: 500 }}>自动提取</div>
            <Space>
              <Switch
                checked={extractConfig.auto_extract}
                onChange={v => setExtractConfig({ ...extractConfig, auto_extract: v })}
              />
              {extractConfig.auto_extract && <Tag color="green">已启用</Tag>}
            </Space>
            <div style={{ fontSize: 12, color: '#999', marginTop: 4 }}>
              启用后将按设定间隔自动触发资源提取
            </div>
          </div>

          {extractConfig.auto_extract && (
            <div style={{ marginBottom: 20 }}>
              <div style={{ marginBottom: 8, fontWeight: 500 }}>提取间隔（分钟）</div>
              <InputNumber
                min={5}
                max={1440}
                value={extractConfig.extract_interval}
                onChange={v => setExtractConfig({ ...extractConfig, extract_interval: v || 30 })}
                style={{ width: 200 }}
              />
            </div>
          )}

          {extractConfig.extract_mode === 'ai' && (
            <>
              <Divider style={{ margin: '16px 0' }} />
              <div style={{ marginBottom: 20 }}>
                <div style={{ marginBottom: 8, fontWeight: 500 }}>
                  AI 提示词模板
                  <span style={{ fontWeight: 'normal', color: '#999', marginLeft: 8 }}>
                    （留空使用默认提示词）
                  </span>
                </div>
                <Input.TextArea
                  value={extractConfig.ai_prompt || DEFAULT_AI_PROMPT}
                  onChange={e => setExtractConfig({ ...extractConfig, ai_prompt: e.target.value })}
                  rows={6}
                  placeholder={DEFAULT_AI_PROMPT}
                />
              </div>
            </>
          )}

          <div style={{ textAlign: 'right' }}>
            <Button
              type="primary"
              onClick={saveExtractConfig}
              loading={extractSaving}
            >
              保存配置
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  )
}

export default Resources
