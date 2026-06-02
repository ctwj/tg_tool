import React, { useEffect, useState } from 'react'
import { Card, Form, Input, Button, message, Space, Alert, Tabs, Table, Modal, Select, Tag } from 'antd'
import { CheckCircleOutlined, CloseCircleOutlined, ThunderboltOutlined, PlusOutlined, ExperimentOutlined, EditOutlined, DeleteOutlined, ApiOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { AiEndpoint } from '../types'

interface ProxyTestResult {
  success: boolean
  message: string
  latency_ms?: number
}

const Settings: React.FC = () => {
  // ─── 基本设置 ───
  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)
  const [testLoading, setTestLoading] = useState(false)
  const [testResult, setTestResult] = useState<ProxyTestResult | null>(null)
  const [envDefaults, setEnvDefaults] = useState<Record<string, string>>({})

  // ─── 大模型配置 ───
  const [endpoints, setEndpoints] = useState<AiEndpoint[]>([])
  const [extractMode, setExtractMode] = useState<string>('rule')
  const [aiPrompt, setAiPrompt] = useState<string>('')
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
      setExtractMode(data.push_extract_mode || 'rule')
      setAiPrompt(data.push_ai_prompt || '')
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

  useEffect(() => {
    fetchOptions()
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

  // ─── 大模型配置：端点管理 ───

  const saveAiConfig = async () => {
    setAiSaving(true)
    try {
      await apiClient.put('/push/extract-config', {
        extract_mode: extractMode,
        ai_endpoints: JSON.stringify(endpoints),
        ai_prompt: aiPrompt,
      })
      message.success('大模型配置已保存')
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setAiSaving(false)
    }
  }

  const openEditModal = (index: number = -1) => {
    setEditingIndex(index)
    if (index >= 0 && endpoints[index]) {
      editForm.setFieldsValue({ ...endpoints[index] })
    } else {
      editForm.resetFields()
    }
    setEditModalOpen(true)
  }

  const saveEndpoint = async () => {
    try {
      const values = await editForm.validateFields()
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

  const deleteEndpoint = (index: number) => {
    const newEndpoints = endpoints.filter((_, i) => i !== index)
    setEndpoints(newEndpoints)
    // 清除该索引的测试结果
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

  // 脱敏密钥显示
  const maskKey = (key: string) => {
    if (!key) return ''
    if (key.length <= 8) return '***'
    return key.slice(0, 4) + '***' + key.slice(-4)
  }

  // ─── 端点表格列 ───

  const endpointColumns = [
    {
      title: 'API 地址',
      dataIndex: 'url',
      key: 'url',
      ellipsis: true,
    },
    {
      title: '模型',
      dataIndex: 'model',
      key: 'model',
      width: 150,
    },
    {
      title: '密钥',
      dataIndex: 'key',
      key: 'key',
      width: 140,
      render: (v: string) => <span style={{ color: '#999' }}>{maskKey(v)}</span>,
    },
    {
      title: '状态',
      key: 'status',
      width: 80,
      render: (_: any, __: AiEndpoint, index: number) => {
        const r = testResults[index]
        if (!r) return <Tag>未测试</Tag>
        return r.success
          ? <Tag color="green">{r.latency_ms}ms</Tag>
          : <Tag color="red">失败</Tag>
      },
    },
    {
      title: '操作',
      key: 'action',
      width: 180,
      render: (_: any, __: AiEndpoint, index: number) => (
        <Space size="small">
          <Button
            type="link"
            size="small"
            icon={<EditOutlined />}
            onClick={() => openEditModal(index)}
          >
            编辑
          </Button>
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
            danger
            icon={<DeleteOutlined />}
            onClick={() => {
              Modal.confirm({
                title: '确认删除',
                content: `确定要删除 ${endpoints[index]?.model || '该端点'} 吗？`,
                onOk: () => deleteEndpoint(index),
              })
            }}
          />
        </Space>
      ),
    },
  ]

  // ─── 渲染 ───

  return (
    <div>
      <h2>系统设置</h2>

      <Tabs
        defaultActiveKey="basic"
        items={[
          {
            key: 'basic',
            label: '基本设置',
            children: (
              <Card>
                <Alert
                  message="配置优先级说明"
                  description="系统配置页面填写的值优先于环境变量（.env 文件）中的配置。留空则使用环境变量中的值。"
                  type="info"
                  showIcon
                  style={{ marginBottom: 24 }}
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
                    style={{ marginBottom: 16 }}
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

                  {/* 其他配置 */}
                  <Card title="其他配置" size="small" type="inner">
                    <Form.Item name="image_group" label="图床群组 ID">
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
              <Card>
                <Alert
                  message="配置 AI 大模型端点后，可在资源提取时启用 AI 增强模式，提升提取质量。支持 OpenAI 兼容格式（OpenAI、DeepSeek、通义千问、Ollama 等）。"
                  type="info"
                  showIcon
                  style={{ marginBottom: 24 }}
                />

                {/* 提取模式 */}
                <div style={{ marginBottom: 24 }}>
                  <div style={{ marginBottom: 8, fontWeight: 500 }}>提取模式</div>
                  <Space>
                    <Select
                      value={extractMode}
                      onChange={setExtractMode}
                      style={{ width: 200 }}
                      options={[
                        { label: '规则提取（推荐）', value: 'rule' },
                        { label: 'AI 增强', value: 'ai' },
                      ]}
                    />
                    {extractMode === 'ai' && (
                      <Tag color="blue">AI 模式已启用</Tag>
                    )}
                  </Space>
                </div>

                {/* 端点列表 */}
                <div style={{ marginBottom: 24 }}>
                  <div style={{ marginBottom: 8, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <span style={{ fontWeight: 500 }}>API 端点列表</span>
                    <Button
                      type="dashed"
                      icon={<PlusOutlined />}
                      onClick={() => openEditModal(-1)}
                    >
                      添加端点
                    </Button>
                  </div>

                  {endpoints.length === 0 ? (
                    <div style={{
                      textAlign: 'center',
                      padding: '40px 0',
                      color: '#999',
                      border: '1px dashed #d9d9d9',
                      borderRadius: 8,
                    }}>
                      <ApiOutlined style={{ fontSize: 32, marginBottom: 8 }} />
                      <div>暂无 AI 端点，点击上方按钮添加</div>
                    </div>
                  ) : (
                    <Table
                      dataSource={endpoints.map((ep, i) => ({ ...ep, key: i }))}
                      columns={endpointColumns}
                      pagination={false}
                      size="small"
                    />
                  )}
                </div>

                {/* 默认提示词 */}
                <div style={{ marginBottom: 24 }}>
                  <div style={{ marginBottom: 8, fontWeight: 500 }}>
                    AI 提示词模板
                    <span style={{ fontWeight: 'normal', color: '#999', marginLeft: 8 }}>
                      （可选，留空使用默认提示词）
                    </span>
                  </div>
                  <Input.TextArea
                    value={aiPrompt}
                    onChange={e => setAiPrompt(e.target.value)}
                    rows={3}
                    placeholder="从以下 Telegram 消息中提取结构化资源信息..."
                  />
                </div>

                {/* 保存 */}
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
        title={editingIndex >= 0 ? '编辑 AI 端点' : '添加 AI 端点'}
        open={editModalOpen}
        onCancel={() => setEditModalOpen(false)}
        onOk={saveEndpoint}
        okText="确定"
        width={520}
      >
        <Form form={editForm} layout="vertical">
          <Form.Item
            name="url"
            label="API 地址"
            rules={[{ required: true, message: '请填写 API 地址' }]}
            extra="如 https://api.openai.com 或 https://api.deepseek.com"
          >
            <Input placeholder="https://api.openai.com" />
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
        </Form>
      </Modal>
    </div>
  )
}

export default Settings
