import React, { useEffect, useState, useMemo, useCallback } from 'react'
import { Button, Table, message, Space, Modal, Form, Input, InputNumber, Switch, Statistic, Card, Row, Col, Tag, Typography, Select, Alert, Tooltip, Tabs, Radio, Popconfirm } from 'antd'
import { RocketOutlined, ReloadOutlined, SettingOutlined, BarChartOutlined, PlusOutlined, DeleteOutlined, ApiOutlined, SafetyCertificateOutlined, CodeOutlined, SendOutlined, ThunderboltOutlined, CopyOutlined, CheckCircleOutlined, CloseCircleOutlined, CloudUploadOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import type { PushConfig, PushHistoryDetail } from '../types'

const { Text } = Typography

// 示例数据（用于预览）
const MOCK_RESOURCES = [
  { title: '阿里云盘资源分享', url: 'https://www.aliyundrive.com/s/abc123', description: '测试资源描述', category: 'aliyun', tags: '分享,测试', img: '', source: 'tg_channel_1', extra: '' },
]
const MOCK_TARGET = 'external_api'

// HTTP 方法颜色映射
const METHOD_COLORS: Record<string, string> = {
  POST: 'blue',
  PUT: 'orange',
  PATCH: 'purple',
}

// 认证方式选项
const AUTH_OPTIONS = [
  { value: 'custom_header', label: '自定义 Header' },
  { value: 'bearer', label: 'Bearer Token' },
  { value: 'query', label: 'Query 参数' },
  { value: 'none', label: '无认证' },
]

const Push: React.FC = () => {
  // ─── 配置列表状态 ───
  const [configs, setConfigs] = useState<PushConfig[]>([])
  const [configsLoading, setConfigsLoading] = useState(false)

  // ─── 推送历史状态 ───
  const [histories, setHistories] = useState<any[]>([])
  const [histLoading, setHistLoading] = useState(false)
  const [histTotal, setHistTotal] = useState(0)
  const [histPage, setHistPage] = useState(1)

  // ─── 推送历史详情（跳过明细） ───
  const [histDetailOpen, setHistDetailOpen] = useState(false)
  const [histDetailLoading, setHistDetailLoading] = useState(false)
  const [histDetail, setHistDetail] = useState<PushHistoryDetail | null>(null)

  // ─── 统计 ───
  const [statsOpen, setStatsOpen] = useState(false)
  const [stats, setStats] = useState({ total: 0, success: 0, failed: 0 })

  // ─── 配置编辑弹窗 ───
  const [editOpen, setEditOpen] = useState(false)
  const [editSaving, setEditSaving] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [form] = Form.useForm()
  const [formValues, setFormValues] = useState<Record<string, any>>({})
  const [authState, setAuthState] = useState<Record<string, { token: string; key: string }>>({
    bearer: { token: '', key: '' },
    custom_header: { token: '', key: 'X-API-Token' },
    query: { token: '', key: 'token' },
  })

  // ─── 采集器列表（数据源选择用） ───
  const [collectors, setCollectors] = useState<any[]>([])

  // ─── 推送中状态 ───
  const [pushingIds, setPushingIds] = useState<Set<number>>(new Set())

  // ─── Tab 控制 ───
  const [activeTab, setActiveTab] = useState('configs')

  const { containerRef, scrollY } = useTableScrollY()

  // ─── 数据加载 ───

  const fetchConfigs = useCallback(async () => {
    setConfigsLoading(true)
    try {
      const res = await apiClient.get('/push/configs')
      setConfigs(res.data?.data?.list ?? [])
    } catch { message.error('获取推送配置失败') }
    finally { setConfigsLoading(false) }
  }, [])

  const fetchHistories = useCallback(async (p: number = 1) => {
    setHistLoading(true)
    try {
      const res = await apiClient.get(`/push/histories?page=${p}&page_size=20`)
      const data = res.data.data
      setHistories(data?.list ?? [])
      setHistTotal(data?.pagination?.total ?? 0)
      setHistPage(p)
    } catch { message.error('获取推送历史失败') }
    finally { setHistLoading(false) }
  }, [])

  const openHistDetail = useCallback(async (id: number) => {
    setHistDetailOpen(true)
    setHistDetailLoading(true)
    setHistDetail(null)
    try {
      const res = await apiClient.get(`/push/histories/${id}`)
      if (res.data?.success) {
        setHistDetail(res.data.data)
      } else {
        message.error(res.data?.message || '获取详情失败')
      }
    } catch {
      message.error('获取详情失败')
    } finally {
      setHistDetailLoading(false)
    }
  }, [])

  const fetchStats = useCallback(async () => {
    try {
      const res = await apiClient.get('/push/stats')
      setStats(res.data.data ?? { total: 0, success: 0, failed: 0 })
    } catch { /* ignore */ }
  }, [])

  const fetchCollectors = useCallback(async () => {
    try {
      const res = await apiClient.get('/collectors')
      setCollectors(res.data?.data?.list ?? [])
    } catch { /* ignore */ }
  }, [])

  useEffect(() => {
    fetchConfigs()
    fetchStats()
    fetchCollectors()
  }, [])

  // ─── 配置操作 ───

  const openCreateConfig = () => {
    setEditingId(null)
    form.resetFields()
    form.setFieldsValue({
      name: '',
      api_url: '',
      api_token: '',
      auth_type: 'custom_header',
      auth_key: 'X-API-Token',
      http_method: 'POST',
      body_template: '',
      custom_headers: [],
      batch_size: 1000,
      data_source_type: 'all',
      collector_ids: [],
      auto_push: false,
      push_interval: 30,
    })
    setFormValues({
      name: '', api_url: '', api_token: '',
      auth_type: 'custom_header', auth_key: 'X-API-Token',
      http_method: 'POST', body_template: '', custom_headers: [],
      batch_size: 1000, data_source_type: 'all', collector_ids: [],
      auto_push: false, push_interval: 30,
    })
    setEditOpen(true)
  }

  const openEditConfig = (record: PushConfig) => {
    setEditingId(record.id)
    const parsedHeaders = parseCustomHeaders(record.custom_headers)
    const fv = {
      name: record.name,
      api_url: record.api_url,
      api_token: record.api_token || '',
      auth_type: record.auth_type,
      auth_key: record.auth_key,
      http_method: record.http_method,
      body_template: record.body_template || '',
      custom_headers: parsedHeaders,
      batch_size: record.batch_size,
      data_source_type: record.data_source_type,
      collector_ids: record.data_source_type === 'selected' ? (record.collector_ids || []) : [],
      auto_push: record.auto_push,
      push_interval: record.push_interval,
    }
    form.setFieldsValue(fv)
    setFormValues({ ...fv })
    if (record.api_token) {
      setAuthState(prev => ({
        ...prev,
        [record.auth_type]: { token: record.api_token || '', key: record.auth_key },
      }))
    }
    setEditOpen(true)
  }

  const saveConfig = async (values: any) => {
    setEditSaving(true)
    try {
      const headersJson = JSON.stringify((values.custom_headers || []).filter((h: any) => h?.key?.trim()))
      const body: any = {
        name: values.name,
        api_url: values.api_url || '',
        api_token: values.api_token || '',
        auth_type: values.auth_type || 'custom_header',
        auth_key: values.auth_key || '',
        http_method: values.http_method || 'POST',
        body_template: values.body_template || '',
        custom_headers: headersJson,
        batch_size: values.batch_size || 1000,
        data_source_type: values.data_source_type || 'all',
        collector_ids: values.data_source_type === 'selected' ? (values.collector_ids || []) : [],
        auto_push: values.auto_push || false,
        push_interval: values.push_interval || 30,
      }
      if (editingId) {
        await apiClient.put(`/push/configs/${editingId}`, body)
        message.success('配置已更新')
      } else {
        await apiClient.post('/push/configs', body)
        message.success('配置已创建')
      }
      setEditOpen(false)
      fetchConfigs()
    } catch (e: any) {
      message.error(e.response?.data?.message || e.message || '保存失败')
    } finally {
      setEditSaving(false)
    }
  }

  const deleteConfig = async (id: number) => {
    try {
      await apiClient.delete(`/push/configs/${id}`)
      message.success('配置已删除')
      fetchConfigs()
    } catch (e: any) {
      message.error(e.response?.data?.message || '删除失败')
    }
  }

  const toggleConfig = async (id: number) => {
    try {
      await apiClient.put(`/push/configs/${id}/toggle`)
      fetchConfigs()
    } catch (e: any) {
      message.error(e.response?.data?.message || '操作失败')
    }
  }

  const duplicateConfig = async (record: PushConfig) => {
    // 从 API 获取详情（含 collector_ids）
    let collectorIds: number[] = []
    try {
      const res = await apiClient.get(`/push/configs/${record.id}`)
      const detail = res.data?.data
      if (detail?.collector_ids) {
        collectorIds = detail.collector_ids
      }
    } catch { /* fallback to empty */ }

    setEditingId(null) // 新建模式
    const parsedHeaders = parseCustomHeaders(record.custom_headers)
    const fv = {
      name: '', // 名称留空，让用户自己填
      api_url: record.api_url,
      api_token: record.api_token || '',
      auth_type: record.auth_type,
      auth_key: record.auth_key,
      http_method: record.http_method,
      body_template: record.body_template || '',
      custom_headers: parsedHeaders,
      batch_size: record.batch_size,
      data_source_type: record.data_source_type,
      collector_ids: record.data_source_type === 'selected' ? collectorIds : [],
      auto_push: record.auto_push,
      push_interval: record.push_interval,
    }
    form.resetFields()
    form.setFieldsValue(fv)
    setFormValues({ ...fv })
    if (record.api_token) {
      setAuthState(prev => ({
        ...prev,
        [record.auth_type]: { token: record.api_token || '', key: record.auth_key },
      }))
    }
    setEditOpen(true)
  }

  const triggerPushForConfig = async (id: number, name: string) => {
    setPushingIds(prev => new Set(prev).add(id))
    try {
      const res = await apiClient.post(`/push/configs/${id}/trigger`, {})
      if (res.data?.success) {
        message.success(`「${name}」推送完成，处理 ${res.data?.data?.processed_count ?? 0} 条`)
      } else {
        message.warning(res.data?.message || '推送未成功')
      }
      fetchHistories(histPage)
      fetchStats()
    } catch (e: any) {
      message.error(e.response?.data?.message || e.message || '推送失败')
    } finally {
      setPushingIds(prev => {
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    }
  }

  const retryFailed = async () => {
    try {
      const res = await apiClient.post('/push/retry')
      message.success(res.data?.message || '重试已触发')
      fetchHistories(histPage); fetchStats()
    } catch (e: any) {
      message.error(e.message || '重试失败')
    }
  }

  const parseCustomHeaders = (str: string): Array<{ key: string; value: string }> => {
    try {
      const arr = JSON.parse(str || '[]')
      return Array.isArray(arr) ? arr.map((h: any) => ({ key: h.key || '', value: h.value || '' })) : []
    } catch { return [] }
  }

  const handleValuesChange = (_changed: any, allValues: any) => {
    if (_changed.auth_type !== undefined) {
      const newType = _changed.auth_type
      const saved = authState[newType] || { token: '', key: '' }
      const currentToken = allValues.api_token
      const currentKey = allValues.auth_key
      const oldType = formValues.auth_type
      if (oldType && currentToken) {
        setAuthState(prev => ({
          ...prev,
          [oldType]: { token: currentToken, key: currentKey || prev[oldType]?.key || '' },
        }))
      }
      form.setFieldsValue({ api_token: saved.token, auth_key: saved.key })
      allValues.api_token = saved.token
      allValues.auth_key = saved.key
    } else {
      const curType = allValues.auth_type || 'custom_header'
      setAuthState(prev => ({
        ...prev,
        [curType]: {
          token: allValues.api_token ?? prev[curType]?.token ?? '',
          key: allValues.auth_key ?? prev[curType]?.key ?? '',
        },
      }))
    }
    setFormValues({ ...allValues })
  }

  // ====== 实时预览计算 ======
  const preview = useMemo(() => {
    const v = formValues
    const method = v.http_method || 'POST'
    const url = v.api_url || ''
    const authType = v.auth_type || 'custom_header'
    const authKey = v.auth_key || ''
    const token = v.api_token || ''
    const customHeaders: Array<{ key: string; value: string }> = v.custom_headers || []

    const headers: Array<{ key: string; value: string; isAuth?: boolean }> = [
      { key: 'Content-Type', value: 'application/json' },
    ]
    if (authType === 'bearer' && token) {
      headers.push({ key: 'Authorization', value: `Bearer ${token}`, isAuth: true })
    } else if (authType === 'custom_header' && authKey && token) {
      headers.push({ key: authKey, value: token, isAuth: true })
    }
    const authHeaderKeys = headers.map(h => h.key.toLowerCase())
    for (const h of customHeaders) {
      if (h.key?.trim() && !authHeaderKeys.includes(h.key.toLowerCase())) {
        headers.push({ key: h.key, value: h.value || '' })
      }
    }

    let previewUrl = url
    if (authType === 'query' && authKey && token) {
      const sep = url.includes('?') ? '&' : '?'
      previewUrl = `${url}${sep}${encodeURIComponent(authKey)}=${encodeURIComponent(token)}`
    }

    const template = v.body_template || ''
    const defaultTemplate = '{"resources": {{resources}}}'
    const activeTemplate = template || defaultTemplate
    let body: string | null = null
    let bodyError: string | null = null
    try {
      const vars: Record<string, string> = {
        resources: JSON.stringify(MOCK_RESOURCES),
        count: String(MOCK_RESOURCES.length),
        target: v.target || MOCK_TARGET,
        timestamp: String(Math.floor(Date.now() / 1000)),
      }
      let rendered = activeTemplate
      for (const [k, val] of Object.entries(vars)) {
        rendered = rendered.replace(new RegExp(`\\{\\{${k}\\}\\}`, 'g'), val)
      }
      JSON.parse(rendered)
      body = JSON.stringify(JSON.parse(rendered), null, 2)
    } catch {
      bodyError = '模板渲染结果不是有效的 JSON，请检查语法'
    }

    return { method, url: previewUrl, headers, body, bodyError }
  }, [formValues])

  // JSON 语法高亮渲染
  const renderJsonHighlight = (json: string) => {
    return json.replace(/("(?:\\.|[^"\\])*")\s*:/g, '<span style="color:#7dd3fc">$1</span>:')
      .replace(/:\s*("(?:\\.|[^"\\])*")/g, ': <span style="color:#7dd3fc">$1</span>')
      .replace(/:\s*(\d+)/g, ': <span style="color:#fbbf24">$1</span>')
      .replace(/:\s*(true|false|null)/g, ': <span style="color:#c084fc">$1</span>')
  }

  // ─── 配置列表列定义 ───
  const configColumns = [
    {
      title: '名称', dataIndex: 'name', key: 'name', width: 140,
      render: (v: string) => <Text strong>{v}</Text>,
    },
    {
      title: 'API 地址', dataIndex: 'api_url', key: 'api_url', width: 220, ellipsis: true,
      render: (v: string) => v ? <Text copyable={{ text: v }} style={{ fontSize: 12 }}>{v.replace(/^https?:\/\//, '').split('/')[0]}</Text> : <Text type="secondary">未配置</Text>,
    },
    {
      title: '数据源', key: 'data_source', width: 130,
      render: (_: any, r: PushConfig) => r.data_source_type === 'all'
        ? <Tag color="blue">全部采集器</Tag>
        : <Tooltip title={`已选 ${r.collector_count} 个采集器`}><Tag color="green">指定采集器 ({r.collector_count})</Tag></Tooltip>,
    },
    {
      title: '状态', dataIndex: 'is_active', key: 'is_active', width: 80,
      render: (v: boolean) => v
        ? <Tag icon={<CheckCircleOutlined />} color="success" style={{ margin: 0 }}>启用</Tag>
        : <Tag icon={<CloseCircleOutlined />} color="default" style={{ margin: 0 }}>禁用</Tag>,
    },
    {
      title: '定时', key: 'schedule', width: 90,
      render: (_: any, r: PushConfig) => r.auto_push
        ? <Tag color="purple">{r.push_interval}分钟</Tag>
        : <Text type="secondary">手动</Text>,
    },
    {
      title: '操作', key: 'actions', width: 280, fixed: 'right' as const,
      render: (_: any, r: PushConfig) => (
        <Space size={4}>
          <Tooltip title="手动推送">
            <Button
              type="primary" size="small" icon={<CloudUploadOutlined />}
              loading={pushingIds.has(r.id)}
              disabled={!r.is_active || !r.api_url}
              onClick={() => triggerPushForConfig(r.id, r.name)}
            />
          </Tooltip>
          <Tooltip title="复制配置">
            <Button size="small" icon={<CopyOutlined />} onClick={() => duplicateConfig(r)} />
          </Tooltip>
          <Tooltip title={r.is_active ? '禁用' : '启用'}>
            <Switch size="small" checked={r.is_active} onChange={() => toggleConfig(r.id)} />
          </Tooltip>
          <Button size="small" icon={<SettingOutlined />} onClick={() => openEditConfig(r)}>编辑</Button>
          <Popconfirm title={`确定删除「${r.name}」？`} onConfirm={() => deleteConfig(r.id)} okText="删除" cancelText="取消">
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  // ─── 推送历史列定义 ───
  const historyColumns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '批次ID', dataIndex: 'batch_id', key: 'batch_id', width: 180, ellipsis: true },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === 'success'
        ? <Tag color="green" style={{ margin: 0 }}>成功</Tag>
        : <Tag color="red" style={{ margin: 0 }}>失败</Tag>,
    },
    { title: '数据量', dataIndex: 'data_count', key: 'data_count', width: 80 },
    {
      title: '跳过统计', key: 'skip', width: 180,
      render: (_: any, r: any) => {
        const img = r.skipped_image_count || 0
        const link = r.skipped_link_count || 0
        const pushed = r.pushed_count ?? r.data_count ?? 0
        if (img === 0 && link === 0) return <Text type="secondary">-</Text>
        return (
          <Space size={4} wrap>
            <Tag color="green" style={{ margin: 0 }}>推 {pushed}</Tag>
            {img > 0 && <Tooltip title="图片未转存跳过"><Tag color="orange" style={{ margin: 0 }}>图 {img}</Tag></Tooltip>}
            {link > 0 && <Tooltip title="链接失效跳过"><Tag color="red" style={{ margin: 0 }}>链 {link}</Tag></Tooltip>}
          </Space>
        )
      },
    },
    { title: '消息', dataIndex: 'message', key: 'message', ellipsis: true },
    {
      title: '错误信息', dataIndex: 'error_msg', key: 'error_msg', ellipsis: true,
      render: (v: string) => v ? <Text type="danger">{v}</Text> : '-',
    },
    {
      title: '推送时间', dataIndex: 'pushed_at', key: 'pushed_at', width: 170,
      render: (v: string) => v ? new Date(v + 'Z').toLocaleString('zh-CN') : '-',
    },
    {
      title: '操作', key: 'action', width: 80, fixed: 'right' as const,
      render: (_: any, r: any) => (
        <Button size="small" onClick={() => openHistDetail(r.id)}>详情</Button>
      ),
    },
  ]

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="推送管理"
        description="管理消息推送和调度配置"
        extra={
          <Space>
            <Button icon={<BarChartOutlined />} onClick={() => { fetchStats(); setStatsOpen(true) }}>推送统计</Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreateConfig}>添加配置</Button>
          </Space>
        }
      />

      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Tabs
          activeKey={activeTab}
          onChange={(key) => {
            setActiveTab(key)
            if (key === 'history') fetchHistories(1)
          }}
          style={{ height: '100%' }}
          items={[
            {
              key: 'configs',
              label: '推送配置',
              children: (
                <Table
                  dataSource={configs}
                  columns={configColumns}
                  rowKey="id"
                  loading={configsLoading}
                  scroll={{ y: scrollY - 40 }}
                  pagination={false}
                  style={{ background: '#fff', borderRadius: 12 }}
                  locale={{ emptyText: (
                    <div style={{ padding: '40px 0' }}>
                      <CloudUploadOutlined style={{ fontSize: 40, color: '#d9d9d9', marginBottom: 12 }} />
                      <p style={{ color: '#999' }}>暂无推送配置</p>
                      <Button type="primary" icon={<PlusOutlined />} onClick={openCreateConfig}>创建第一个推送配置</Button>
                    </div>
                  )}}
                />
              ),
            },
            {
              key: 'history',
              label: '推送历史',
              children: (
                <div>
                  <Space style={{ marginBottom: 12 }}>
                    <Button icon={<ReloadOutlined />} onClick={retryFailed}>重试失败</Button>
                    <Button onClick={() => { fetchHistories(histPage); fetchStats() }}>刷新</Button>
                  </Space>
                  <Table
                    dataSource={histories}
                    columns={historyColumns}
                    rowKey="id"
                    loading={histLoading}
                    scroll={{ y: scrollY - 100 }}
                    pagination={{
                      current: histPage, total: histTotal, pageSize: 20,
                      onChange: (p) => fetchHistories(p),
                      showTotal: (t) => `共 ${t} 条`, size: 'small',
                    }}
                    style={{ background: '#fff', borderRadius: 12 }}
                  />
                </div>
              ),
            },
          ]}
        />
      </div>

      {/* 推送历史详情弹窗（含跳过明细 skip_records） */}
      <Modal
        title="推送历史详情"
        open={histDetailOpen}
        onCancel={() => setHistDetailOpen(false)}
        footer={<Button onClick={() => setHistDetailOpen(false)}>关闭</Button>}
        width={760}
      >
        {histDetailLoading ? (
          <div style={{ textAlign: 'center', padding: '40px 0', color: '#999' }}>加载中...</div>
        ) : histDetail ? (
          <div>
            <Row gutter={16} style={{ marginBottom: 16 }}>
              <Col span={6}><Card size="small"><Statistic title="实际推送" value={histDetail.history.pushed_count ?? histDetail.history.data_count ?? 0} valueStyle={{ color: '#3f8600' }} /></Card></Col>
              <Col span={6}><Card size="small"><Statistic title="图片未转存跳过" value={histDetail.history.skipped_image_count ?? 0} valueStyle={{ color: '#d48806' }} /></Card></Col>
              <Col span={6}><Card size="small"><Statistic title="链接失效跳过" value={histDetail.history.skipped_link_count ?? 0} valueStyle={{ color: '#cf1322' }} /></Card></Col>
              <Col span={6}><Card size="small"><Statistic title="状态" value={histDetail.history.status === 'success' ? '成功' : '失败'} valueStyle={{ color: histDetail.history.status === 'success' ? '#3f8600' : '#cf1322' }} /></Card></Col>
            </Row>
            <div style={{ marginBottom: 8 }}>
              <Text strong>跳过明细</Text>
              <Text type="secondary" style={{ marginLeft: 8, fontSize: 12 }}>
                （共 {histDetail.skip_records?.length ?? 0} 条）
              </Text>
            </div>
            <Table
              size="small"
              rowKey="resource_id"
              dataSource={histDetail.skip_records || []}
              pagination={false}
              locale={{ emptyText: '无跳过记录' }}
              scroll={{ y: 320 }}
              columns={[
                { title: '资源ID', dataIndex: 'resource_id', key: 'resource_id', width: 80 },
                { title: '标题', dataIndex: 'title', key: 'title', ellipsis: true, render: (v: string) => v || '-' },
                {
                  title: '原因', dataIndex: 'skip_reason', key: 'skip_reason', width: 130,
                  render: (v: string) => v === 'image_not_forwarded'
                    ? <Tag color="orange" style={{ margin: 0 }}>图片未转存</Tag>
                    : <Tag color="red" style={{ margin: 0 }}>链接失效</Tag>,
                },
                { title: '失效链接', dataIndex: 'urls_invalid', key: 'urls_invalid', ellipsis: true, render: (v: string) => v || '-' },
                { title: '详情', dataIndex: 'detail', key: 'detail', ellipsis: true, render: (v: string) => v || '-' },
              ]}
            />
          </div>
        ) : null}
      </Modal>

      {/* 推送统计弹窗 */}
      <Modal
        title="推送统计"
        open={statsOpen}
        onCancel={() => setStatsOpen(false)}
        footer={<Button onClick={() => setStatsOpen(false)}>关闭</Button>}
        width={520}
      >
        <Row gutter={16}>
          <Col span={8}>
            <Card><Statistic title="总推送" value={stats.total} prefix={<RocketOutlined />} /></Card>
          </Col>
          <Col span={8}>
            <Card><Statistic title="成功" value={stats.success} valueStyle={{ color: '#3f8600' }} /></Card>
          </Col>
          <Col span={8}>
            <Card><Statistic title="失败" value={stats.failed} valueStyle={{ color: '#cf1322' }} /></Card>
          </Col>
        </Row>
      </Modal>

      {/* ====== 配置编辑弹窗 — 左右分栏 ====== */}
      <Modal
        title={<span><SendOutlined style={{ marginRight: 8 }} />{editingId ? '编辑推送配置' : '新建推送配置'}</span>}
        open={editOpen}
        onCancel={() => setEditOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={editSaving}
        okText={editingId ? '保存修改' : '创建配置'}
        width={1080}
        styles={{ body: { maxHeight: 'calc(100vh - 200px)', overflowY: 'auto', paddingRight: 4 } }}
      >
        <Form form={form} onFinish={saveConfig} onValuesChange={handleValuesChange} layout="vertical" size="middle">
        <Row gutter={20}>
          {/* ===== 左侧：配置表单 ===== */}
          <Col span={14}>

              {/* 基本配置 */}
              <Card
                size="small"
                title={<span><SettingOutlined style={{ marginRight: 6, color: '#1677ff' }} />基本配置</span>}
                style={{ marginBottom: 16 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <Form.Item name="name" label="配置名称"
                  rules={[{ required: true, message: '请填写配置名称' }]}>
                  <Input placeholder="如：官网推送、备份API" />
                </Form.Item>
              </Card>

              {/* 连接配置 */}
              <Card
                size="small"
                title={<span><ApiOutlined style={{ marginRight: 6, color: '#1677ff' }} />连接配置</span>}
                style={{ marginBottom: 16 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <Form.Item name="api_url" label="推送 API 地址"
                  rules={[{ required: true, message: '请填写推送 API 地址' }]}
                  extra={<Text type="secondary" style={{ fontSize: 12 }}>接收推送数据的外部 API 地址</Text>}>
                  <Input placeholder="https://your-api.com/push" />
                </Form.Item>
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item name="http_method" label="HTTP 方法">
                      <Select options={[
                        { value: 'POST', label: 'POST' },
                        { value: 'PUT', label: 'PUT' },
                        { value: 'PATCH', label: 'PATCH' },
                      ]} />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Form.Item name="batch_size" label="每批推送数量"
                      extra={<Text type="secondary" style={{ fontSize: 12 }}>单次推送最大消息数</Text>}>
                      <InputNumber min={1} max={10000} style={{ width: '100%' }} />
                    </Form.Item>
                  </Col>
                </Row>
              </Card>

              {/* 认证配置 */}
              <Card
                size="small"
                title={<span><SafetyCertificateOutlined style={{ marginRight: 6, color: '#52c41a' }} />认证配置</span>}
                style={{ marginBottom: 16 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item name="auth_type" label="认证方式">
                      <Select options={AUTH_OPTIONS} />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Tooltip
                      title={
                        formValues.auth_type === 'bearer' ? '将发送: Authorization: Bearer {凭证}'
                        : formValues.auth_type === 'custom_header' ? `将发送: ${formValues.auth_key || 'X-API-Token'}: {凭证}`
                        : formValues.auth_type === 'query' ? `将附加: ?${formValues.auth_key || 'token'}={凭证}`
                        : '不会发送任何认证信息'
                      }
                    >
                      <Form.Item name="api_token" label="认证凭证"
                        extra={
                          <Tag color={formValues.auth_type === 'none' ? 'default' : 'processing'} style={{ fontSize: 11, marginTop: 2 }}>
                            {formValues.auth_type === 'bearer' ? 'Authorization Header'
                              : formValues.auth_type === 'custom_header' ? 'Custom Header'
                              : formValues.auth_type === 'query' ? 'Query Parameter'
                              : '未启用认证'}
                          </Tag>
                        }>
                        {formValues.auth_type === 'none' ? (
                          <Input disabled placeholder="无认证模式" />
                        ) : (
                          <Input.Password placeholder="your-secret-token" />
                        )}
                      </Form.Item>
                    </Tooltip>
                  </Col>
                </Row>
                <Form.Item noStyle shouldUpdate={(prev, cur) => prev.auth_type !== cur.auth_type}>
                  {({ getFieldValue }) => {
                    const at = getFieldValue('auth_type')
                    if (at === 'custom_header') return (
                      <Form.Item name="auth_key" label="Header 名称"
                        extra={<Text type="secondary" style={{ fontSize: 12 }}>自定义认证 Header 的 Key</Text>}>
                        <Input placeholder="X-API-Token" />
                      </Form.Item>
                    )
                    if (at === 'query') return (
                      <Form.Item name="auth_key" label="参数名称"
                        extra={<Text type="secondary" style={{ fontSize: 12 }}>URL Query 参数的 Key</Text>}>
                        <Input placeholder="token" />
                      </Form.Item>
                    )
                    return null
                  }}
                </Form.Item>
              </Card>

              {/* 请求体模板 */}
              <Card
                size="small"
                title={<span><CodeOutlined style={{ marginRight: 6, color: '#722ed1' }} />请求体模板</span>}
                style={{ marginBottom: 16 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <div style={{ marginBottom: 8 }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    可用变量：{' '}
                    {['resources', 'count', 'target', 'timestamp'].map(v => (
                      <Tag key={v} style={{ fontSize: 11, margin: '0 2px', fontFamily: 'monospace' }}>{'{{' + v + '}}'}</Tag>
                    ))}
                  </Text>
                </div>
                <Form.Item name="body_template">
                  <Input.TextArea
                    rows={5}
                    placeholder={'{"resources": {{resources}}, "count": {{count}}}'}
                    maxLength={10000}
                    showCount
                    style={{ fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace", fontSize: 13 }}
                  />
                </Form.Item>
              </Card>

              {/* 自定义 Header */}
              <Card
                size="small"
                title={<span><SettingOutlined style={{ marginRight: 6, color: '#fa8c16' }} />自定义 Header</span>}
                style={{ marginBottom: 16 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <Form.List name="custom_headers">
                  {(fields, { add, remove }) => (
                    <>
                      {fields.map(({ key, name, ...restField }) => (
                        <Row key={key} gutter={8} align="middle" style={{ marginBottom: 8 }}>
                          <Col span={10}>
                            <Form.Item {...restField} name={[name, 'key']} style={{ marginBottom: 0 }}
                              rules={[{ required: true, message: 'Key 必填' }]}>
                              <Input placeholder="Header Key" style={{ fontFamily: 'monospace' }} />
                            </Form.Item>
                          </Col>
                          <Col span={12}>
                            <Form.Item {...restField} name={[name, 'value']} style={{ marginBottom: 0 }}>
                              <Input placeholder="Header Value" style={{ fontFamily: 'monospace' }} />
                            </Form.Item>
                          </Col>
                          <Col span={2}>
                            <Tooltip title="删除此 Header">
                              <Button type="text" danger icon={<DeleteOutlined />} onClick={() => remove(name)} style={{ cursor: 'pointer' }} />
                            </Tooltip>
                          </Col>
                        </Row>
                      ))}
                      {fields.length < 10 && (
                        <Button type="dashed" onClick={() => add()} icon={<PlusOutlined />} block style={{ cursor: 'pointer' }}>
                          添加 Header（最多 10 条）
                        </Button>
                      )}
                    </>
                  )}
                </Form.List>
              </Card>

          </Col>

          {/* ===== 右侧：数据源 + 定时推送 + 实时预览 ===== */}
          <Col span={10} style={{ position: 'sticky', top: 0, alignSelf: 'flex-start' }}>
            {/* 数据源 */}
            <Card
              size="small"
              title={<span><CloudUploadOutlined style={{ marginRight: 6, color: '#13c2c2' }} />数据源</span>}
              style={{ marginBottom: 16 }}
              styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
            >
              <Form.Item name="data_source_type" label="推送数据范围">
                <Radio.Group>
                  <Radio value="all">全部采集器</Radio>
                  <Radio value="selected">指定采集器</Radio>
                </Radio.Group>
              </Form.Item>
              <Form.Item noStyle shouldUpdate={(prev: any, cur: any) => prev.data_source_type !== cur.data_source_type}>
                {({ getFieldValue }) => {
                  if (getFieldValue('data_source_type') === 'selected') return (
                    <Form.Item name="collector_ids" label="选择采集器"
                      rules={[{ required: true, message: '请至少选择一个采集器' }]}>
                      <Select
                        mode="multiple"
                        placeholder="选择要推送的采集器"
                        options={collectors.map((c: any) => ({
                          value: c.id,
                          label: c.channel_name || `频道 ${c.channel_id}`,
                        }))}
                        optionFilterProp="label"
                        showSearch
                      />
                    </Form.Item>
                  )
                  return null
                }}
              </Form.Item>
            </Card>

            {/* 定时推送 */}
            <Card
              size="small"
              title={<span><ThunderboltOutlined style={{ marginRight: 6, color: '#eb2f96' }} />定时推送</span>}
              style={{ marginBottom: 16 }}
              styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
            >
              <Form.Item name="auto_push" label="自动定时推送" valuePropName="checked"
                extra="开启后按设定间隔自动推送未推送的采集数据">
                <Switch checkedChildren="开" unCheckedChildren="关" />
              </Form.Item>
              <Form.Item noStyle shouldUpdate={(prev: any, cur: any) => prev.auto_push !== cur.auto_push}>
                {({ getFieldValue }) => getFieldValue('auto_push') ? (
                  <Form.Item name="push_interval" label="推送间隔（分钟）"
                    extra="每隔多少分钟自动推送一次，最小 1 分钟">
                    <InputNumber min={1} max={1440} style={{ width: '100%' }} />
                  </Form.Item>
                ) : null}
              </Form.Item>
            </Card>

            {/* 实时预览面板 */}
            <div style={{
              background: '#0f172a',
              borderRadius: 10,
              overflow: 'hidden',
              border: '1px solid #1e293b',
              boxShadow: '0 4px 24px rgba(0,0,0,0.12)',
            }}>
              {/* 预览标题栏 — 模拟终端 */}
              <div style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '10px 16px',
                background: '#1e293b',
                borderBottom: '1px solid #334155',
              }}>
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#ef4444' }} />
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#fbbf24' }} />
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#0ea5e9' }} />
                <Text style={{ color: '#94a3b8', fontSize: 12, marginLeft: 8, fontFamily: 'monospace' }}>
                  Request Preview
                </Text>
              </div>

              <div style={{ padding: 16 }}>
                {/* 请求行 */}
                <div style={{
                  display: 'flex', alignItems: 'center', gap: 8,
                  marginBottom: 16, padding: '8px 12px',
                  background: '#1e293b', borderRadius: 6,
                }}>
                  <Tag color={METHOD_COLORS[preview.method] || 'blue'} style={{
                    fontWeight: 700, fontSize: 12, minWidth: 52, textAlign: 'center',
                    margin: 0, borderRadius: 4,
                  }}>
                    {preview.method}
                  </Tag>
                  {preview.url ? (
                    <span style={{
                      color: '#e2e8f0', fontSize: 12, fontFamily: 'monospace',
                      wordBreak: 'break-all', lineHeight: '18px',
                    }}>
                      {preview.url}
                    </span>
                  ) : (
                    <span style={{ color: '#64748b', fontSize: 12, fontStyle: 'italic' }}>
                      https://your-api.com/push
                    </span>
                  )}
                </div>

                {!preview.url && (
                  <div style={{ marginBottom: 12 }}>
                    <Alert
                      message="API 地址未填写，预览使用占位数据"
                      type="info"
                      showIcon
                      style={{ background: '#1e293b', border: '1px solid #334155', borderRadius: 6 }}
                    />
                  </div>
                )}

                {/* 请求头 */}
                {preview.headers.length > 0 && (
                  <div style={{ marginBottom: 16 }}>
                    <div style={{ color: '#94a3b8', fontSize: 11, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                      Request Headers
                    </div>
                    <div style={{
                      background: '#1e293b', borderRadius: 6, padding: '8px 12px',
                      borderLeft: '2px solid #3b82f6',
                    }}>
                      {preview.headers.map((h, i) => (
                        <div key={i} style={{
                          fontSize: 12, fontFamily: "'SFMono-Regular', Consolas, monospace",
                          lineHeight: '22px', display: 'flex', gap: 4,
                        }}>
                          <span style={{ color: h.isAuth ? '#fbbf24' : '#7dd3fc', flexShrink: 0 }}>
                            {h.key}:
                          </span>
                          <span style={{
                            color: h.isAuth ? '#fde68a' : '#7dd3fc',
                            wordBreak: 'break-all',
                          }}>
                            {h.value}
                          </span>
                          {h.isAuth && (
                            <Tag color="gold" style={{ fontSize: 9, lineHeight: '16px', margin: '0 0 0 4px', padding: '0 4px' }}>
                              AUTH
                            </Tag>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* 请求体 */}
                <div>
                  <div style={{ color: '#94a3b8', fontSize: 11, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                    Request Body
                  </div>
                  {preview.bodyError ? (
                    <Alert message={preview.bodyError} type="error" style={{ marginTop: 4, borderRadius: 6 }} showIcon />
                  ) : preview.body ? (
                    <div style={{
                      background: '#1e293b', borderRadius: 6, padding: '10px 12px',
                      borderLeft: '2px solid #a855f7',
                      maxHeight: 280, overflow: 'auto',
                    }}>
                      <pre
                        style={{
                          fontSize: 11.5, lineHeight: '17px', margin: 0,
                          fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
                          color: '#e2e8f0', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                        }}
                        dangerouslySetInnerHTML={{
                          __html: preview.body ? renderJsonHighlight(preview.body) : ''
                        }}
                      />
                    </div>
                  ) : (
                    <div style={{ background: '#1e293b', borderRadius: 6, padding: '12px', color: '#64748b', fontSize: 12, fontStyle: 'italic' }}>
                      模板为空，将使用默认格式
                    </div>
                  )}
                </div>
              </div>
            </div>
          </Col>
        </Row>
        </Form>
      </Modal>
    </div>
  )
}

export default Push
