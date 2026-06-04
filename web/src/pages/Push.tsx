import React, { useEffect, useState, useMemo } from 'react'
import { Button, Table, message, Space, Modal, Form, Input, InputNumber, Switch, Statistic, Card, Row, Col, Tag, Typography, Select, Alert, Tooltip } from 'antd'
import { RocketOutlined, ReloadOutlined, SettingOutlined, BarChartOutlined, PlusOutlined, DeleteOutlined, ApiOutlined, SafetyCertificateOutlined, CodeOutlined, SendOutlined, ThunderboltOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

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
  const [histories, setHistories] = useState<any[]>([])
  const [loading, setLoading] = useState(false)
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)

  const [statsOpen, setStatsOpen] = useState(false)
  const [configOpen, setConfigOpen] = useState(false)
  const [configSaving, setConfigSaving] = useState(false)
  const [form] = Form.useForm()

  const [stats, setStats] = useState({ total: 0, success: 0, failed: 0 })
  const [formValues, setFormValues] = useState<Record<string, any>>({})

  const [authState, setAuthState] = useState<Record<string, { token: string; key: string }>>({
    bearer: { token: '', key: '' },
    custom_header: { token: '', key: 'X-API-Token' },
    query: { token: '', key: 'token' },
  })

  const fetchHistories = async (p: number = 1) => {
    setLoading(true)
    try {
      const res = await apiClient.get(`/push/histories?page=${p}&page_size=20`)
      const data = res.data.data
      setHistories(data?.list ?? [])
      setTotal(data?.pagination?.total ?? 0)
      setPage(p)
    } catch { message.error('获取推送历史失败') }
    finally { setLoading(false) }
  }

  const fetchStats = async () => {
    try {
      const res = await apiClient.get('/push/stats')
      setStats(res.data.data ?? { total: 0, success: 0, failed: 0 })
    } catch { /* ignore */ }
  }

  const fetchConfig = async () => {
    try {
      const res = await apiClient.get('/options')
      const data = res.data.data ?? {}
      const fv = {
        api_url: data.push_api_url || '',
        api_token: data.push_api_token || '',
        target: data.push_target || '',
        batch_size: parseInt(data.push_batch_size) || 1000,
        auto_push: data.push_auto_push === '1' || data.push_auto_push === 'true',
        interval: parseInt(data.push_interval) || 30,
        auth_type: data.push_auth_type || 'custom_header',
        auth_key: data.push_auth_key || 'X-API-Token',
        http_method: data.push_http_method || 'POST',
        body_template: data.push_body_template || '',
        custom_headers: parseCustomHeaders(data.push_custom_headers),
      }
      form.setFieldsValue(fv)
      setFormValues(fv)
      if (data.push_api_token) {
        setAuthState(prev => ({
          ...prev,
          [(data.push_auth_type || 'custom_header')]: {
            token: data.push_api_token,
            key: data.push_auth_key || prev[data.push_auth_type || 'custom_header']?.key || '',
          },
        }))
      }
    } catch { /* ignore */ }
  }

  useEffect(() => { fetchHistories(1); fetchStats() }, [])

  const triggerPush = async () => {
    try {
      const checkRes = await apiClient.get('/push/config-check')
      if (checkRes.data?.success) {
        const { is_valid, missing } = checkRes.data.data || {}
        if (!is_valid) {
          const missingLabels: Record<string, string> = {
            push_api_url: '推送 API 地址',
            push_api_token: '认证凭证',
            push_target: '推送目标',
            push_auth_key: '认证字段 Key',
          }
          const items = (missing || []).map((k: string) => missingLabels[k] || k)
          Modal.warning({
            title: '推送配置不完整',
            content: (
              <div>
                <p>请先在推送配置中补充以下项：</p>
                <ul>{items.map((item: string) => <li key={item}>{item}</li>)}</ul>
              </div>
            ),
          })
          return
        }
      }

      const res = await apiClient.post('/push/trigger', {})
      if (res.data?.success) {
        message.success(res.data?.data?.message || `推送完成，处理 ${res.data?.data?.processed_count ?? 0} 条`)
      } else {
        message.warning(res.data?.message || '推送未成功')
      }
      fetchHistories(page); fetchStats()
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '推送失败')
    }
  }

  const retryFailed = async () => {
    try {
      const res = await apiClient.post('/push/retry')
      message.success(res.data?.message || '重试已触发')
      fetchHistories(page); fetchStats()
    } catch (e: any) {
      message.error(e.message || '重试失败')
    }
  }

  const openConfig = () => {
    fetchConfig()
    setConfigOpen(true)
  }

  const saveConfig = async (values: any) => {
    setConfigSaving(true)
    try {
      const headersJson = JSON.stringify((values.custom_headers || []).filter((h: any) => h?.key?.trim()))
      await apiClient.put('/push/scheduler', {
        api_url: values.api_url || '',
        api_token: values.api_token || '',
        batch_size: values.batch_size || 1000,
        auto_push: values.auto_push ? '1' : '0',
        interval: values.interval || 30,
        auth_type: values.auth_type || 'custom_header',
        auth_key: values.auth_key || '',
        http_method: values.http_method || 'POST',
        body_template: values.body_template || '',
        custom_headers: headersJson,
      })
      message.success('配置已保存')
      setConfigOpen(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setConfigSaving(false)
    }
  }

  const openStats = async () => {
    await fetchStats()
    setStatsOpen(true)
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
      .replace(/:\s*("(?:\\.|[^"\\])*")/g, ': <span style="color:#86efac">$1</span>')
      .replace(/:\s*(\d+)/g, ': <span style="color:#fbbf24">$1</span>')
      .replace(/:\s*(true|false|null)/g, ': <span style="color:#c084fc">$1</span>')
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '批次ID', dataIndex: 'batch_id', key: 'batch_id', width: 180, ellipsis: true },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === 'success'
        ? <Tag color="green" style={{ margin: 0 }}>成功</Tag>
        : <Tag color="red" style={{ margin: 0 }}>失败</Tag>,
    },
    { title: '数据量', dataIndex: 'data_count', key: 'data_count', width: 80 },
    { title: '消息', dataIndex: 'message', key: 'message', ellipsis: true },
    {
      title: '错误信息', dataIndex: 'error_msg', key: 'error_msg', ellipsis: true,
      render: (v: string) => v ? <Text type="danger">{v}</Text> : '-',
    },
    {
      title: '推送时间', dataIndex: 'pushed_at', key: 'pushed_at', width: 170,
      render: (v: string) => v ? new Date(v + 'Z').toLocaleString('zh-CN') : '-',
    },
  ]

  const { containerRef, scrollY } = useTableScrollY()

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="推送管理"
        description="管理消息推送和调度配置"
        extra={
          <Space>
            <Button icon={<BarChartOutlined />} onClick={openStats}>推送统计</Button>
            <Button icon={<SettingOutlined />} onClick={openConfig}>推送配置</Button>
          </Space>
        }
      />

      <Space style={{ marginBottom: 12, flexShrink: 0 }}>
        <Button type="primary" icon={<RocketOutlined />} onClick={triggerPush}>手动推送</Button>
        <Button icon={<ReloadOutlined />} onClick={retryFailed}>重试失败</Button>
        <Button onClick={() => { fetchHistories(page); fetchStats() }}>刷新</Button>
      </Space>

      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={histories}
          columns={columns}
          rowKey="id"
          loading={loading}
          scroll={{ y: scrollY }}
          pagination={{
            current: page, total, pageSize: 20,
            onChange: (p) => fetchHistories(p),
            showTotal: (t) => `共 ${t} 条`, size: 'small',
          }}
          style={{ background: '#fff', borderRadius: 12 }}
        />
      </div>

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

      {/* ====== 推送配置弹窗 — 优化版左右分栏 ====== */}
      <Modal
        title={<span><SendOutlined style={{ marginRight: 8 }} />推送配置</span>}
        open={configOpen}
        onCancel={() => setConfigOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={configSaving}
        okText="保存全部配置"
        width={1080}
        styles={{ body: { maxHeight: 'calc(100vh - 200px)', overflowY: 'auto', paddingRight: 4 } }}
      >
        <Row gutter={20}>
          {/* ===== 左侧：配置表单 ===== */}
          <Col span={14}>
            <Form form={form} onFinish={saveConfig} onValuesChange={handleValuesChange} layout="vertical" size="middle">

              {/* 基本连接配置 */}
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
                        extra={<Text type="secondary" style={{ fontSize: 12 }}>自定义认证 Header 的 Key，如 X-API-Token、X-Api-Key</Text>}>
                        <Input placeholder="X-API-Token" />
                      </Form.Item>
                    )
                    if (at === 'query') return (
                      <Form.Item name="auth_key" label="参数名称"
                        extra={<Text type="secondary" style={{ fontSize: 12 }}>URL Query 参数的 Key，如 token、api_key</Text>}>
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

              {/* 定时推送 */}
              <Card
                size="small"
                title={<span><ThunderboltOutlined style={{ marginRight: 6, color: '#eb2f96' }} />定时推送</span>}
                style={{ marginBottom: 0 }}
                styles={{ body: { paddingTop: 16, paddingBottom: 8 } }}
              >
                <Form.Item name="auto_push" label="自动定时推送" valuePropName="checked"
                  extra="开启后按设定间隔自动推送未推送的采集数据">
                  <Switch checkedChildren="开" unCheckedChildren="关" />
                </Form.Item>
                <Form.Item noStyle shouldUpdate={(prev, cur) => prev.auto_push !== cur.auto_push}>
                  {({ getFieldValue }) => getFieldValue('auto_push') ? (
                    <Form.Item name="interval" label="推送间隔（分钟）"
                      extra="每隔多少分钟自动推送一次，最小 1 分钟">
                      <InputNumber min={1} max={1440} style={{ width: 200 }} />
                    </Form.Item>
                  ) : null}
                </Form.Item>
              </Card>

            </Form>
          </Col>

          {/* ===== 号侧：实时预览面板 ===== */}
          <Col span={10} style={{ position: 'sticky', top: 0, alignSelf: 'flex-start' }}>
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
                <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#22c55e' }} />
                <Text style={{ color: '#94a3b8', fontSize: 12, marginLeft: 8, fontFamily: 'monospace' }}>
                  Request Preview
                </Text>
              </div>

              <div style={{ padding: 16 }}>
                {/* 请求行 — 始终显示，URL 为空时用占位 */}
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
                            color: h.isAuth ? '#fde68a' : '#86efac',
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

                {/* 请求体 — 始终显示 */}
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
      </Modal>
    </div>
  )
}

export default Push
