import React, { useEffect, useState } from 'react'
import { Card, Form, Input, Button, message, Space, Alert, Tabs, Modal, Tag, Switch, InputNumber, Select, Spin } from 'antd'
import { CheckCircleOutlined, CloseCircleOutlined, ThunderboltOutlined, PlusOutlined, ExperimentOutlined, EditOutlined, DeleteOutlined, ApiOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { AiEndpoint, Client, Chat } from '../types'
import PageHeader from '../components/PageHeader'


interface ProxyTestResult {
  success: boolean
  message: string
  latency_ms?: number
}

// AI 类型预设配置
const aiTypePresets: Record<string, { label: string; color: string; url: string; model: string; delay: number }> = {
  openai: { label: 'OpenAI 兼容', color: 'green', url: 'https://api.openai.com/v1', model: 'gpt-4o-mini', delay: 500 },
  nvidia: { label: 'NVIDIA NIM', color: 'orangered', url: 'https://integrate.api.nvidia.com/v1', model: 'abacusai/dracarys-llama-3.1-70b-instruct', delay: 2000 },
  zhipu: { label: '智普 GLM', color: 'blue', url: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash', delay: 1000 },
}

const generateId = () => 'cfg_' + Date.now() + '_' + Math.random().toString(36).substring(2, 9)

const Settings: React.FC = () => {
  // ─── 基本设置 ───
  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)
  const [testLoading, setTestLoading] = useState(false)
  const [testResult, setTestResult] = useState<ProxyTestResult | null>(null)
  const [envDefaults, setEnvDefaults] = useState<Record<string, string>>({})

  // ─── 图床群组选择 ───
  const [clients, setClients] = useState<Client[]>([])
  const [chats, setChats] = useState<Chat[]>([])
  const [chatsLoading, setChatsLoading] = useState(false)
  const [selectedClientId, setSelectedClientId] = useState<string>('')

  // ─── 大模型配置 ───
  const [endpoints, setEndpoints] = useState<AiEndpoint[]>([])
  const [aiSaving, setAiSaving] = useState(false)
  const [editModalOpen, setEditModalOpen] = useState(false)
  const [editingIndex, setEditingIndex] = useState<number>(-1)
  const [editForm] = Form.useForm()
  const [testingIndex, setTestingIndex] = useState<number>(-1)
  const [testResults, setTestResults] = useState<Record<number, ProxyTestResult>>({})

  // ─── 基本设置：加载/保存 ───

  const fetchOptions = async () => {
    try {
      const res = await apiClient.get('/options')
      const data = res.data.data ?? {}
      form.setFieldsValue(data)
      if (res.data.env_defaults) setEnvDefaults(res.data.env_defaults)

      // 大模型配置
      const endpointsStr = data.push_ai_endpoints || '[]'
      try {
        setEndpoints(JSON.parse(endpointsStr))
      } catch {
        setEndpoints([])
      }
    } catch {
      /* ignore */
    }
  }

  const fetchClients = async () => {
    try {
      const res = await apiClient.get('/clients')
      const list: Client[] = res.data.data?.list ?? []
      setClients(list.filter(c => c.status === 'active'))
    } catch {
      /* ignore */
    }
  }

  const fetchChats = async (clientId: string) => {
    if (!clientId) { setChats([]); return }
    setChatsLoading(true)
    setChats([])
    try {
      const res = await apiClient.get(`/clients/${clientId}/chats`)
      const list: Chat[] = res.data.data?.chats ?? []
      setChats(list)
    } catch {
      message.error('获取聊天列表失败')
    } finally {
      setChatsLoading(false)
    }
  }

  useEffect(() => {
    fetchOptions()
    fetchClients()
  }, [form])

  const saveOptions = async (values: Record<string, string>) => {
    setLoading(true)
    try {
      await apiClient.put('/options', values)
      message.success('设置已保存')
      setTestResult(null)
    } catch {
      message.error('保存失败')
    } finally {
      setLoading(false)
    }
  }

  const testProxy = async () => {
    setTestLoading(true)
    setTestResult(null)
    try {
      const values = form.getFieldsValue()
      await apiClient.put('/options', values)

      const res = await apiClient.post('/options/test-proxy')
      setTestResult(res.data)
      if (res.data.success) {
        message.success(res.data.message)
      } else {
        message.warning(res.data.message)
      }
    } catch (e: any) {
      const msg = e.response?.data?.message || e.message || '测试失败'
      setTestResult({ success: false, message: msg })
      message.error(msg)
    } finally {
      setTestLoading(false)
    }
  }

  const getPlaceholder = (key: string) => {
    const val = envDefaults[key]
    if (!val) return undefined
    return `当前使用环境变量: ${val}`
  }

  // ─── 图床群组：级联选择 ───

  const onImageGroupClientChange = (clientId: string) => {
    setSelectedClientId(clientId)
    form.setFieldsValue({ image_group: undefined })
    fetchChats(clientId)
  }

  const onImageGroupChatChange = (chatId: number) => {
    form.setFieldsValue({ image_group: String(chatId) })
  }

  // ─── 大模型配置：端点管理 ───

  const saveAiConfig = async () => {
    setAiSaving(true)
    try {
      await apiClient.put('/push/extract-config', {
        ai_endpoints: JSON.stringify(endpoints),
      })
      message.success('大模型配置已保存')
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setAiSaving(false)
    }
  }

  const openAddModal = () => {
    setEditingIndex(-1)
    const preset = aiTypePresets.openai
    editForm.resetFields()
    editForm.setFieldsValue({
      id: generateId(),
      name: '',
      ai_type: 'openai',
      url: preset.url,
      key: '',
      model: preset.model,
      enable: true,
      request_delay: preset.delay,
    })
    setEditModalOpen(true)
  }

  const openEditModal = (index: number) => {
    setEditingIndex(index)
    editForm.setFieldsValue({ ...endpoints[index] })
    setEditModalOpen(true)
  }

  const handleAiTypeChange = (value: string) => {
    const preset = aiTypePresets[value]
    if (preset) {
      editForm.setFieldsValue({
        url: preset.url,
        model: preset.model,
        request_delay: preset.delay,
      })
    }
  }

  const saveEndpoint = async () => {
    try {
      const values = await editForm.validateFields()
      if (!values.id) values.id = generateId()
      if (!values.name) {
        const preset = aiTypePresets[values.ai_type]
        values.name = `${preset?.label || values.ai_type} 配置`
      }

      const newEndpoints = [...endpoints]
      if (editingIndex >= 0) {
        newEndpoints[editingIndex] = values
      } else {
        newEndpoints.push(values)
      }
      setEndpoints(newEndpoints)
      setEditModalOpen(false)
    } catch {
      /* validation failed */
    }
  }

  const toggleEndpoint = (index: number, enable: boolean) => {
    const newEndpoints = [...endpoints]
    newEndpoints[index] = { ...newEndpoints[index], enable }
    setEndpoints(newEndpoints)
  }

  const deleteEndpoint = (index: number) => {
    const newEndpoints = endpoints.filter((_, i) => i !== index)
    setEndpoints(newEndpoints)
    const newResults = { ...testResults }
    delete newResults[index]
    setTestResults(newResults)
  }

  const testEndpoint = async (index: number) => {
    const ep = endpoints[index]
    if (!ep) return
    setTestingIndex(index)
    try {
      const res = await apiClient.post('/options/ai-test', {
        url: ep.url,
        key: ep.key,
        model: ep.model,
      })
      setTestResults(prev => ({
        ...prev,
        [index]: {
          success: res.data.success,
          message: res.data.message,
          latency_ms: res.data.latency_ms,
        },
      }))
      if (res.data.success) {
        message.success(res.data.message)
      } else {
        message.warning(res.data.message)
      }
    } catch (e: any) {
      const msg = e.response?.data?.message || e.message || '测试失败'
      setTestResults(prev => ({
        ...prev,
        [index]: { success: false, message: msg },
      }))
      message.error(msg)
    } finally {
      setTestingIndex(-1)
    }
  }

  const maskKey = (key: string) => {
    if (!key) return ''
    if (key.length <= 8) return '***'
    return key.slice(0, 4) + '***' + key.slice(-4)
  }

  // ─── 渲染 ───

  return (
    <div>
      <PageHeader title="系统设置" description="管理系统配置、代理和 AI 大模型" />

      <Tabs
        defaultActiveKey="basic"
        items={[
          {
            key: 'basic',
            label: '基本设置',
            children: (
              <Card style={{ borderRadius: 12 }}>
                <Alert
                  message="配置优先级说明"
                  description="系统配置页面填写的值优先于环境变量（.env 文件）中的配置。留空则使用环境变量中的值。"
                  type="info"
                  showIcon
                  style={{ marginBottom: 24, borderRadius: 8 }}
                />

                <Form form={form} onFinish={saveOptions} layout="vertical">
                  {/* 代理配置 */}
                  <Form.Item
                    name="proxy_url"
                    label="代理地址"
                    help="支持 HTTP/SOCKS5 代理，如 socks5://127.0.0.1:1080 或 http://proxy:8080"
                  >
                    <Input placeholder={getPlaceholder('proxy_url') || 'socks5://127.0.0.1:1080'} />
                  </Form.Item>

                  {testResult && (
                    <div style={{ marginBottom: 16 }}>
                      {testResult.success ? (
                        <Alert
                          message={
                            <Space>
                              <CheckCircleOutlined style={{ color: '#52c41a' }} />
                              <span>{testResult.message}</span>
                            </Space>
                          }
                          type="success"
                          showIcon={false}
                          style={{ borderRadius: 8 }}
                        />
                      ) : (
                        <Alert
                          message={
                            <Space>
                              <CloseCircleOutlined style={{ color: '#ff4d4f' }} />
                              <span>{testResult.message}</span>
                            </Space>
                          }
                          type="error"
                          showIcon={false}
                          style={{ borderRadius: 8 }}
                        />
                      )}
                    </div>
                  )}

                  <Form.Item>
                    <Space>
                      <Button type="primary" htmlType="submit" loading={loading}>
                        保存设置
                      </Button>
                      <Button
                        icon={<ThunderboltOutlined />}
                        onClick={testProxy}
                        loading={testLoading}
                      >
                        测试代理连接
                      </Button>
                    </Space>
                  </Form.Item>

                  {/* Telegram 配置 */}
                  <Card
                    title="Telegram 配置"
                    size="small"
                    style={{ marginBottom: 16, borderRadius: 10 }}
                    type="inner"
                  >
                    <Form.Item
                      name="tg_app_id"
                      label="Telegram APP ID"
                      help="留空则使用环境变量中的值，填写后将覆盖环境变量"
                    >
                      <Input placeholder={getPlaceholder('tg_app_id') || '从 my.telegram.org 获取'} />
                    </Form.Item>
                    <Form.Item
                      name="tg_app_hash"
                      label="Telegram APP Hash"
                      help="留空则使用环境变量中的值，填写后将覆盖环境变量"
                    >
                      <Input placeholder={getPlaceholder('tg_app_hash') || '从 my.telegram.org 获取'} />
                    </Form.Item>
                  </Card>

                  {/* 图床配置 */}
                  <Card title="图床配置" size="small" type="inner" style={{ borderRadius: 10 }}>
                    <Form.Item label="选择客户端" help="选择一个已连接的客户端来加载群组列表">
                      <Select
                        placeholder="请先选择客户端"
                        value={selectedClientId || undefined}
                        onChange={onImageGroupClientChange}
                        notFoundContent={clients.length === 0 ? '没有活跃的客户端，请先启动客户端' : undefined}
                      >
                        {clients.map(c => (
                          <Select.Option key={c.id} value={c.id}>
                            {c.client_type === 'Bot' ? 'Bot ' : ''}
                            {c.phone || c.id.substring(0, 8)}...
                          </Select.Option>
                        ))}
                      </Select>
                    </Form.Item>
                    <Form.Item label="图床群组" help="选择用于图床上传的群组">
                      <Space direction="vertical" style={{ width: '100%' }}>
                        <Select
                          placeholder={selectedClientId ? '加载中...' : '请先选择客户端'}
                          onChange={onImageGroupChatChange}
                          loading={chatsLoading}
                          disabled={!selectedClientId || chatsLoading}
                          showSearch
                          optionFilterProp="label"
                          notFoundContent={
                            !selectedClientId ? '请先选择客户端' :
                            chatsLoading ? <Spin size="small" /> :
                            chats.length === 0 ? '没有可用的群组' : undefined
                          }
                          value={form.getFieldValue('image_group') ? Number(form.getFieldValue('image_group')) : undefined}
                        >
                          {chats.filter(c => c.type === 'channel' || c.type === 'group').map(c => (
                            <Select.Option key={c.id} value={c.id} label={c.name}>
                              {c.type === 'channel' ? '频道' : '群组'} {c.name}
                              <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                            </Select.Option>
                          ))}
                        </Select>
                      </Space>
                    </Form.Item>
                    <Form.Item name="image_group" label="群组 ID" help="也可直接输入群组 ID">
                      <Input placeholder="-100xxxxxxxxxx" />
                    </Form.Item>
                  </Card>
                </Form>
              </Card>
            ),
          },
          {
            key: 'ai',
            label: (
              <span>
                <ApiOutlined /> 大模型配置
              </span>
            ),
            children: (
              <Card style={{ borderRadius: 12 }}>
                <Alert
                  message="配置 AI 大模型端点，支持 OpenAI 兼容格式。API 地址需包含版本路径（如 /v1）。多个端点将轮询使用。"
                  type="info"
                  showIcon
                  style={{ marginBottom: 24, borderRadius: 8 }}
                />

                {/* 端点列表 */}
                <div style={{ marginBottom: 24 }}>
                  <div style={{ marginBottom: 12, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <span style={{ fontWeight: 500 }}>API 配置列表</span>
                    <Button
                      type="dashed"
                      icon={<PlusOutlined />}
                      onClick={openAddModal}
                    >
                      添加配置
                    </Button>
                  </div>

                  {endpoints.length === 0 ? (
                    <div style={{
                      textAlign: 'center',
                      padding: '40px 0',
                      color: '#999',
                      border: '1px dashed #d9d9d9',
                      borderRadius: 10,
                    }}>
                      <ApiOutlined style={{ fontSize: 32, marginBottom: 8 }} />
                      <div>暂无 API 配置，点击上方按钮添加</div>
                    </div>
                  ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                      {endpoints.map((ep, index) => {
                        const preset = aiTypePresets[ep.ai_type] || aiTypePresets.openai
                        const testResult = testResults[index]
                        return (
                          <div
                            key={ep.id || index}
                            style={{
                              border: '1px solid #f0f0f0',
                              borderRadius: 10,
                              padding: '12px 16px',
                              transition: 'border-color 0.2s',
                            }}
                          >
                            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                <Switch
                                  size="small"
                                  checked={ep.enable}
                                  onChange={(checked) => toggleEndpoint(index, checked)}
                                />
                                <span style={{ fontWeight: 500, fontSize: 14 }}>
                                  {ep.name || '未命名配置'}
                                </span>
                                <Tag color={preset.color} style={{ margin: 0 }}>
                                  {preset.label}
                                </Tag>
                                {!ep.enable && (
                                  <span style={{ fontSize: 12, color: '#bbb' }}>已禁用</span>
                                )}
                                {testResult && (
                                  testResult.success
                                    ? <Tag color="green" style={{ margin: 0 }}>{testResult.latency_ms}ms</Tag>
                                    : <Tag color="red" style={{ margin: 0 }}>失败</Tag>
                                )}
                              </div>
                              <Space size="small">
                                <Button
                                  type="link"
                                  size="small"
                                  icon={<ExperimentOutlined />}
                                  loading={testingIndex === index}
                                  onClick={() => testEndpoint(index)}
                                >
                                  测试
                                </Button>
                                <Button
                                  type="link"
                                  size="small"
                                  icon={<EditOutlined />}
                                  onClick={() => openEditModal(index)}
                                />
                                <Button
                                  type="link"
                                  size="small"
                                  danger
                                  icon={<DeleteOutlined />}
                                  onClick={() => {
                                    Modal.confirm({
                                      title: '确认删除',
                                      content: `确定要删除「${ep.name || '该配置'}」吗？`,
                                      onOk: () => deleteEndpoint(index),
                                    })
                                  }}
                                />
                              </Space>
                            </div>
                            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
                              {ep.url} | {ep.model} | 密钥: {maskKey(ep.key)} | 延迟: {ep.request_delay || 0}ms
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  )}
                </div>

                <Button
                  type="primary"
                  onClick={saveAiConfig}
                  loading={aiSaving}
                >
                  保存大模型配置
                </Button>
              </Card>
            ),
          },
        ]}
      />

      {/* 添加/编辑端点弹窗 */}
      <Modal
        title={editingIndex >= 0 ? '编辑 API 配置' : '添加 API 配置'}
        open={editModalOpen}
        onCancel={() => setEditModalOpen(false)}
        onOk={saveEndpoint}
        okText="确定"
        width={520}
      >
        <Form form={editForm} layout="vertical">
          <Form.Item name="id" hidden>
            <Input />
          </Form.Item>

          <Form.Item
            name="name"
            label="配置名称"
            rules={[{ required: true, message: '请填写配置名称' }]}
          >
            <Input placeholder="如：OpenAI 主力、DeepSeek 备用" />
          </Form.Item>

          <Form.Item
            name="ai_type"
            label="AI 类型"
            rules={[{ required: true, message: '请选择 AI 类型' }]}
          >
            <Select onChange={handleAiTypeChange}>
              {Object.entries(aiTypePresets).map(([key, cfg]) => (
                <Select.Option key={key} value={key}>
                  <Space>
                    <Tag color={cfg.color} style={{ margin: 0 }}>{cfg.label}</Tag>
                  </Space>
                  {cfg.label}
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item
            name="url"
            label="API 地址"
            rules={[{ required: true, message: '请填写 API 地址' }]}
            extra="需包含完整路径，如 https://api.openai.com/v1"
          >
            <Input placeholder="https://api.openai.com/v1" />
          </Form.Item>

          <Form.Item
            name="key"
            label="API 密钥"
            rules={[{ required: true, message: '请填写 API 密钥' }]}
          >
            <Input.Password placeholder="sk-xxxxxxxx" />
          </Form.Item>

          <Form.Item
            name="model"
            label="模型名称"
            rules={[{ required: true, message: '请填写模型名称' }]}
            extra="如 gpt-4o、deepseek-chat、qwen-plus"
          >
            <Input placeholder="gpt-4o" />
          </Form.Item>

          <Form.Item
            name="request_delay"
            label="请求延迟 (ms)"
            help="每次请求后的等待时间，避免 API 限流。NVIDIA 建议 2000ms 以上"
          >
            <InputNumber min={0} max={10000} step={100} style={{ width: '100%' }} placeholder="1000" />
          </Form.Item>

          <Form.Item
            name="enable"
            label="启用"
            valuePropName="checked"
          >
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Settings
