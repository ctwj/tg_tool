import React, { useEffect, useState } from 'react'
import { Card, Form, Input, Button, message, Space, Alert, Tabs, Modal, Tag, Switch, InputNumber, Select, Collapse, Typography, AutoComplete } from 'antd'
import { CheckCircleOutlined, CloseCircleOutlined, ThunderboltOutlined, PlusOutlined, ExperimentOutlined, EditOutlined, DeleteOutlined, ApiOutlined, PictureOutlined, CloudOutlined, GlobalOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { AiEndpoint } from '../types'
import PageHeader from '../components/PageHeader'
import { normalizeImageDomain } from '../utils/imageDomain'

const { Text, Paragraph } = Typography

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
  const [httpTestLoading, setHttpTestLoading] = useState(false)
  const [httpTestResult, setHttpTestResult] = useState<ProxyTestResult | null>(null)
  const [envDefaults, setEnvDefaults] = useState<Record<string, string>>({})
  const [botClients, setBotClients] = useState<any[]>([])
  const [botChats, setBotChats] = useState<any[]>([])
  const [botChatsLoading, setBotChatsLoading] = useState(false)
  const [chatIdValue, setChatIdValue] = useState('')
  const [chatValidating, setChatValidating] = useState(false)
  const [chatValidResult, setChatValidResult] = useState<{ success: boolean; msg: string } | null>(null)

  // ─── 图床配置 ───
  const [imageSaving, setImageSaving] = useState(false)
  const [testFileId, setTestFileId] = useState('')

  // ─── 网盘链接检测配置 ───
  const [linkCheckSaving, setLinkCheckSaving] = useState(false)

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
      setChatIdValue(data.ImageGroupChatId || '')
      if (res.data.env_defaults) setEnvDefaults(res.data.env_defaults)

      // 大模型配置
      const endpointsStr = data.push_ai_endpoints || '[]'
      try {
        setEndpoints(JSON.parse(endpointsStr))
      } catch {
        setEndpoints([])
      }

      // 如果已配置图床 Bot，自动加载群组列表（失败提示原因：如 webhook 冲突、网络代理问题）
      if (data.ImageBotId) {
        apiClient.get(`/clients/${data.ImageBotId}/bot-chats`)
          .then(res => setBotChats(res.data.data?.chats ?? []))
          .catch((e: any) => {
            const reason = e?.response?.data?.message || e?.message
            if (reason) message.warning(`获取 Bot 群组列表失败：${reason}`)
          })
      }
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    fetchOptions()
    // 获取 Bot 类型客户端列表
    apiClient.get('/clients').then(res => {
      const list = res.data.data?.list ?? []
      setBotClients(list.filter((c: any) => c.client_type === 'Bot'))
    }).catch(() => {})
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

  const saveImageOptions = async () => {
    setImageSaving(true)
    try {
      const values = form.getFieldsValue(['image_group', 'TelegramImageDomain', 'ImageCacheTTL', 'ImageBotId', 'ImageGroupChatId', 'ImageGroupChatId2', 'delete_bot_forward_message', 'ImageForwardInterval', 'image_storage_enabled'])
      await apiClient.put('/options', values)
      message.success('图床配置已保存')
    } catch {
      message.error('保存失败')
    } finally {
      setImageSaving(false)
    }
  }

  const saveLinkCheckOptions = async () => {
    setLinkCheckSaving(true)
    try {
      const v = form.getFieldsValue(['link_checker_type', 'pancheck_host', 'link_check_concurrency', 'link_check_cache_ttl_hours'])
      await apiClient.put('/options', {
        link_checker_type: v.link_checker_type || 'pancheck',
        pancheck_host: v.pancheck_host || '',
        link_check_concurrency: String(v.link_check_concurrency ?? 5),
        link_check_cache_ttl_hours: String(v.link_check_cache_ttl_hours ?? 24),
      })
      message.success('链接检测配置已保存')
    } catch {
      message.error('保存失败')
    } finally {
      setLinkCheckSaving(false)
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

  const testHttpProxy = async () => {
    setHttpTestLoading(true)
    setHttpTestResult(null)
    try {
      const values = form.getFieldsValue()
      await apiClient.put('/options', values)

      const res = await apiClient.post('/options/test-http-proxy')
      setHttpTestResult(res.data)
      if (res.data.success) {
        message.success(res.data.message)
      } else {
        message.warning(res.data.message)
      }
    } catch (e: any) {
      const msg = e.response?.data?.message || e.message || '测试失败'
      setHttpTestResult({ success: false, message: msg })
      message.error(msg)
    } finally {
      setHttpTestLoading(false)
    }
  }

  const getPlaceholder = (key: string) => {
    const val = envDefaults[key]
    if (!val) return undefined
    return `当前使用环境变量: ${val}`
  }

  // ─── 图床群组：级联选择 ───

  // ─── 大模型配置：端点管理 ───

  const saveAiConfig = async () => {
    setAiSaving(true)
    try {
      const concurrency = form.getFieldValue('ai_concurrency') || '5'
      await apiClient.put('/push/extract-config', {
        ai_endpoints: JSON.stringify(endpoints),
        ai_concurrency: concurrency,
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

  const codeStyle: React.CSSProperties = {
    background: '#f5f5f5',
    border: '1px solid #e8e8e8',
    borderRadius: 8,
    padding: '12px 16px',
    fontFamily: 'monospace',
    fontSize: 13,
    lineHeight: 1.6,
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-all',
    overflowX: 'auto',
  }

  return (
    <div style={{ height: '100%', overflowY: 'auto' }}>
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
                    <Form.Item
                      name="proxy_url"
                      label="Telegram 代理地址"
                      help="Telegram 客户端使用的代理，通常为 SOCKS5 代理，如 socks5://127.0.0.1:1080"
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
                      <Button
                        icon={<ThunderboltOutlined />}
                        onClick={testProxy}
                        loading={testLoading}
                      >
                        测试 Telegram 代理
                      </Button>
                    </Form.Item>
                  </Card>

                  {/* HTTP 代理配置 */}
                  <Card
                    title="HTTP 代理配置"
                    size="small"
                    style={{ marginBottom: 16, borderRadius: 10 }}
                    type="inner"
                  >
                    <Form.Item
                      name="http_proxy_url"
                      label="HTTP 代理地址"
                      help="HTTP API 请求使用的代理（如 AI 提取），如 http://127.0.0.1:7890"
                    >
                      <Input placeholder="http://127.0.0.1:7890" />
                    </Form.Item>

                    {httpTestResult && (
                      <div style={{ marginBottom: 16 }}>
                        {httpTestResult.success ? (
                          <Alert
                            message={
                              <Space>
                                <CheckCircleOutlined style={{ color: '#52c41a' }} />
                                <span>{httpTestResult.message}</span>
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
                                <span>{httpTestResult.message}</span>
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
                      <Button
                        icon={<GlobalOutlined />}
                        onClick={testHttpProxy}
                        loading={httpTestLoading}
                      >
                        测试 HTTP 代理
                      </Button>
                    </Form.Item>
                  </Card>

                  {/* 网盘链接检测配置 */}
                  <Card
                    title="网盘链接检测配置（PanCheck）"
                    size="small"
                    style={{ marginBottom: 16, borderRadius: 10 }}
                    type="inner"
                  >
                    <Alert
                      message="推送前自动跳过网盘链接已失效的资源。配置检测服务地址后启用链接检测；留空则仅按图片转存状态过滤（不检测链接）。"
                      type="info"
                      showIcon
                      style={{ marginBottom: 16, borderRadius: 8 }}
                    />
                    <Form.Item
                      name="link_checker_type"
                      label="检测服务类型"
                      initialValue="pancheck"
                      help="选择链接检测服务（可插拔，新增检测服务后在此切换；当前支持 PanCheck）"
                    >
                      <Select options={[{ label: 'PanCheck', value: 'pancheck' }]} />
                    </Form.Item>
                    <Form.Item
                      name="pancheck_host"
                      label="PanCheck 服务地址"
                      help="PanCheck 服务 Host，如 http://pancheck:6080。系统将调用 {地址}/api/v1/links/check"
                    >
                      <Input placeholder="http://pancheck:6080" />
                    </Form.Item>
                    <Form.Item
                      name="link_check_concurrency"
                      label="检测并发数"
                      initialValue={5}
                      help="同时调用 PanCheck 的并发数（1-20，默认 5）"
                    >
                      <InputNumber min={1} max={20} step={1} style={{ width: '100%' }} placeholder="5" />
                    </Form.Item>
                    <Form.Item
                      name="link_check_cache_ttl_hours"
                      label="结果缓存时长（小时）"
                      initialValue={24}
                      help="链接检测结果缓存有效期，过期后重新检测（默认 24）"
                    >
                      <InputNumber min={1} max={720} step={1} style={{ width: '100%' }} placeholder="24" />
                    </Form.Item>
                    <Form.Item>
                      <Button type="primary" onClick={saveLinkCheckOptions} loading={linkCheckSaving}>
                        保存链接检测配置
                      </Button>
                    </Form.Item>
                  </Card>

                  {/* 安全配置 */}
                  <Card
                    title="安全配置"
                    size="small"
                    style={{ marginBottom: 16, borderRadius: 10 }}
                    type="inner"
                  >
                    <Form.Item
                      name="allow_register"
                      label="允许注册"
                      valuePropName="checked"
                      getValueFromEvent={(checked: boolean) => checked ? 'true' : 'false'}
                      getValueProps={(value: string) => ({ checked: value === 'true' || !value })}
                      help="关闭后，新用户无法注册账号，登录页面将隐藏注册入口"
                    >
                      <Switch checkedChildren="开启" unCheckedChildren="关闭" />
                    </Form.Item>
                  </Card>

                  <Form.Item>
                    <Button type="primary" htmlType="submit" loading={loading} size="large">
                      保存设置
                    </Button>
                  </Form.Item>
                </Form>
              </Card>
            ),
          },
          {
            key: 'image',
            label: (
              <span>
                <PictureOutlined /> 图床配置
              </span>
            ),
            children: (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
                {/* 图床设置 */}
                <Card title="图床设置" style={{ borderRadius: 12 }}>
                  <Form form={form} layout="vertical">
                    <div style={{ marginBottom: 16, padding: 12, background: '#fafafa', borderRadius: 8, border: '1px solid #f0f0f0' }}>
                      <Form.Item
                        name="image_storage_enabled"
                        label="转存功能总开关"
                        valuePropName="checked"
                        getValueFromEvent={(checked: boolean) => checked ? 'true' : 'false'}
                        getValueProps={(value: string) => ({ checked: value !== 'false' })}
                        help="关闭后新图片不再入队（已入队任务会继续处理完）。默认开启"
                        style={{ marginBottom: 0 }}
                      >
                        <Switch checkedChildren="开启" unCheckedChildren="关闭" />
                      </Form.Item>
                    </div>
                    <Form.Item
                      name="TelegramImageDomain"
                      label="图床域名"
                      help="图片访问的域名，如 https://img.example.com，用于拼接资源封面 URL"
                    >
                      <Input placeholder="https://img.example.com" />
                    </Form.Item>
                    <Form.Item
                      name="ImageCacheTTL"
                      label="图片缓存过期天数"
                      help="本地缓存的图片文件过期时间，过期后重新从 Telegram 下载"
                      initialValue={7}
                    >
                      <InputNumber min={1} max={365} step={1} style={{ width: '100%' }} placeholder="7" />
                    </Form.Item>
                    <div style={{ borderTop: '1px solid #f0f0f0', paddingTop: 16, marginTop: 8, marginBottom: 8 }}>
                      <Text strong style={{ fontSize: 14, marginBottom: 12, display: 'block' }}>图片转发配置（双群组两阶段）</Text>
                      <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 16 }}>
                        阶段1：客户端用 copy_media 不下载地把图片转发到「图床群组A」，记录消息 ID。<br />
                        阶段2：Bot 用 forwardMessage 把消息从群组A 转发到「Bot 中转群组B」，同步获取 file_id 写入映射表。<br />
                        客户端和 Bot 均需加入群组A；Bot 单独加入群组B（开启清理时需为管理员）。<br />
                        不知道群组 Chat ID？将 Bot 加入目标群组后在群里发送 /id，Bot 会直接回复该群的 Chat ID（约 10 秒内响应）。
                      </Text>
                    </div>
                    <Form.Item
                      name="ImageBotId"
                      label="图床 Bot"
                      help="选择用于图片转存的 Bot 客户端（需先在客户端管理中添加 Bot 类型客户端）"
                    >
                      <Select
                        placeholder="请选择图床 Bot"
                        allowClear
                        showSearch
                        filterOption={(input, option) =>
                          (option?.label ?? '').toLowerCase().includes(input.toLowerCase())
                        }
                        options={botClients.map((c: any) => {
                          const main = c.name
                            ? (c.username ? `${c.name} (@${c.username})` : c.name)
                            : (c.username ? `@${c.username}` : c.id)
                          return {
                            value: c.id,
                            label: c.status === 'active' ? main : `${main} (离线)`,
                          }
                        })}
                        onChange={(value: string | undefined) => {
                          // 切换 Bot 时清空群组选择并重新加载群组列表
                          setChatIdValue('')
                          form.setFieldValue('ImageGroupChatId', undefined)
                          setBotChats([])
                          if (value) {
                            setBotChatsLoading(true)
                            apiClient.get(`/clients/${value}/bot-chats`)
                              .then(res => {
                                setBotChats(res.data.data?.chats ?? [])
                              })
                              .catch((e: any) => {
                                setBotChats([])
                                const reason = e?.response?.data?.message || e?.message || '网络错误'
                                message.warning(`获取 Bot 群组列表失败：${reason}`)
                              })
                              .finally(() => setBotChatsLoading(false))
                          }
                        }}
                      />
                    </Form.Item>
                    <Form.Item
                      name="ImageGroupChatId"
                      label="图床群组A（客户端转存目标）"
                      help="选择 Bot 后自动加载群组列表，也可手动输入。若列表中没有目标群组，可将 Bot 拉进群组后发送 /id，Bot 会在群内回复该群的 Chat ID（约 10 秒内），复制填入即可"
                    >
                      <Space.Compact style={{ width: '100%' }}>
                        <AutoComplete
                          style={{ flex: 1 }}
                          value={chatIdValue}
                          placeholder={botChatsLoading ? '加载中...' : '请选择或输入 Chat ID（如 -1001234567890）'}
                          options={botChats.map((c: any) => ({
                            value: String(c.id),
                            label: `${c.title} (${c.id})`,
                          }))}
                          filterOption={(input, option) =>
                            (option?.label ?? '').toLowerCase().includes(input.toLowerCase()) ||
                            (option?.value ?? '').toString().includes(input)
                          }
                          onChange={(val) => {
                            setChatIdValue(val)
                            form.setFieldValue('ImageGroupChatId', val)
                            setChatValidResult(null)
                          }}
                        />
                        <Button
                          loading={chatValidating}
                          onClick={async () => {
                            const botId = form.getFieldValue('ImageBotId')
                            const chatId = form.getFieldValue('ImageGroupChatId')
                            if (!botId) { message.warning('请先选择图床 Bot'); return }
                            if (!chatId) { message.warning('请输入群组/频道 Chat ID'); return }
                            setChatValidating(true)
                            setChatValidResult(null)
                            try {
                              const res = await apiClient.post(`/clients/${botId}/validate-chat`, { chat_id: chatId })
                              const data = res.data.data
                              setChatValidResult({ success: true, msg: `验证成功: ${data.title} (${data.type}, ID: ${data.id})` })
                            } catch (e: any) {
                              const msg = e?.message || e?.response?.data?.message || '验证失败，请检查 Chat ID 和 Bot 权限'
                              setChatValidResult({ success: false, msg })
                            } finally {
                              setChatValidating(false)
                            }
                          }}
                        >
                          验证
                        </Button>
                      </Space.Compact>
                    </Form.Item>
                    {chatValidResult && (
                      <Alert
                        message={chatValidResult.msg}
                        type={chatValidResult.success ? 'success' : 'error'}
                        showIcon
                        closable
                        onClose={() => setChatValidResult(null)}
                        style={{ marginBottom: 16, borderRadius: 8 }}
                      />
                    )}
                    <Form.Item
                      name="ImageGroupChatId2"
                      label="Bot 中转群组B（可选）"
                      help="留空则跳过阶段2，任务将停留在 awaiting_bot 状态、API 返回 404。配置后 Bot 从群组A 转发到此群组以获取 file_id；获取 Chat ID 方式同群组A（群里发送 /id）"
                    >
                      <AutoComplete
                        style={{ width: '100%' }}
                        placeholder={botChatsLoading ? '加载中...' : '-1009876543210（Bot 需加入此群组；开启自动清理需为管理员）'}
                        options={botChats.map((c: any) => ({
                          value: String(c.id),
                          label: `${c.title} (${c.id})`,
                        }))}
                        filterOption={(input, option) =>
                          (option?.label ?? '').toLowerCase().includes(input.toLowerCase()) ||
                          (option?.value ?? '').toString().includes(input)
                        }
                      />
                    </Form.Item>
                    <Form.Item
                      name="delete_bot_forward_message"
                      label="自动删除群组B 临时消息"
                      valuePropName="checked"
                      getValueFromEvent={(checked: boolean) => checked ? 'true' : 'false'}
                      getValueProps={(value: string) => ({ checked: value === 'true' })}
                      help="开启后 Bot 提取 file_id 后自动删除群组B 中的转发消息。需 Bot 为群组B 管理员；默认关闭"
                    >
                      <Switch checkedChildren="开启" unCheckedChildren="关闭" />
                    </Form.Item>
                    <Form.Item
                      name="ImageForwardInterval"
                      label="转发间隔（秒）"
                      help="每张图片转发的时间间隔，避免触发 Telegram 频率限制"
                      initialValue={2}
                    >
                      <InputNumber min={1} max={60} step={1} style={{ width: '100%' }} placeholder="2" />
                    </Form.Item>
                    <Form.Item>
                      <Button type="primary" onClick={saveImageOptions} loading={imageSaving}>
                        保存图床配置
                      </Button>
                    </Form.Item>
                  </Form>
                </Card>

                {/* 链接测试 */}
                <Card title="链接测试" style={{ borderRadius: 12 }}>
                  <Space direction="vertical" style={{ width: '100%' }} size={12}>
                    <Input
                      placeholder="输入 Bot file_id"
                      value={testFileId}
                      onChange={e => setTestFileId(e.target.value)}
                      style={{ maxWidth: 400 }}
                    />
                    {testFileId && (() => {
                      const domain = normalizeImageDomain(form.getFieldValue('TelegramImageDomain'))
                      const apiUrl = `${window.location.origin}/api/images/${testFileId}`
                      const imgUrl = domain ? `${domain}/${testFileId}` : null
                      return (
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, padding: '8px 0' }}>
                          <div>
                            <Text type="secondary" style={{ fontSize: 12 }}>API 地址：</Text>
                            <a href={apiUrl} target="_blank" rel="noreferrer" style={{ fontSize: 13, wordBreak: 'break-all' }}>{apiUrl}</a>
                          </div>
                          {imgUrl ? (
                            <div>
                              <Text type="secondary" style={{ fontSize: 12 }}>图床域名：</Text>
                              <a href={imgUrl} target="_blank" rel="noreferrer" style={{ fontSize: 13, color: '#0ea5e9', wordBreak: 'break-all' }}>{imgUrl}</a>
                            </div>
                          ) : (
                            <Alert message="请先配置图床域名" type="warning" showIcon style={{ borderRadius: 8 }} />
                          )}
                        </div>
                      )
                    })()}
                  </Space>
                </Card>

                {/* Nginx 配置说明 */}
                <Card
                  title={<span><GlobalOutlined /> Nginx 反向代理配置</span>}
                  style={{ borderRadius: 12 }}
                >
                  <Paragraph type="secondary" style={{ marginBottom: 16 }}>
                    配置 Nginx 反向代理后，可以通过独立域名直接访问图片，无需暴露 API 端口。
                    推荐格式（Bot file_id 直接下载，资源封面与推送 img 均使用此格式）：
                    <Text code>https://img.example.com/file/{'{'}file_id{'}'}</Text>。
                  </Paragraph>
                  <Collapse
                    ghost
                    items={[
                      {
                        key: 'nginx-basic',
                        label: '基础配置',
                        children: (
                          <div style={codeStyle}>{`server {
    listen 443 ssl http2;
    server_name img.example.com;

    ssl_certificate     /etc/letsencrypt/live/img.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/img.example.com/privkey.pem;

    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location / {
        proxy_pass http://127.0.0.1:3000/api/images/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        proxy_connect_timeout 30s;
        proxy_read_timeout 30s;
    }
}

# HTTP → HTTPS 重定向
server {
    listen 80;
    server_name img.example.com;
    return 301 https://$host$request_uri;
}`}</div>
                        ),
                      },
                      {
                        key: 'nginx-cache',
                        label: 'Nginx 本地缓存（可选）',
                        children: (
                          <>
                            <Paragraph type="secondary">
                              在 Nginx 层增加本地缓存，减少对 API 的请求。在 <Text code>http</Text> 块中添加缓存路径，在 <Text code>server</Text> 块中启用。
                            </Paragraph>
                            <div style={codeStyle}>{`# http 块中：
proxy_cache_path /var/cache/nginx/images levels=1:2
    keys_zone=image_cache:10m max_size=1g inactive=30d;

# server 块的 location / 中：
location / {
    proxy_pass http://127.0.0.1:3000/api/images/;

    proxy_cache image_cache;
    proxy_cache_valid 200 30d;
    proxy_cache_valid 404 1m;
    proxy_cache_valid 503 1m;
    add_header X-Cache-Status $upstream_cache_status;

    # ... 其他 proxy_set_header
}`}</div>
                          </>
                        ),
                      },
                      {
                        key: 'nginx-ssl',
                        label: 'SSL 证书获取',
                        children: (
                          <div style={codeStyle}>{`# 安装 certbot
apt install certbot python3-certbot-nginx

# 获取证书（先确保 DNS 已指向服务器）
certbot --nginx -d img.example.com

# 测试自动续期
certbot renew --dry-run`}</div>
                        ),
                      },
                    ]}
                  />
                </Card>

                {/* Cloudflare 配置说明 */}
                <Card
                  title={<span><CloudOutlined /> Cloudflare CDN 缓存配置</span>}
                  style={{ borderRadius: 12 }}
                >
                  <Paragraph type="secondary" style={{ marginBottom: 16 }}>
                    配置 Cloudflare CDN 缓存后，图片在首次访问后被 CDN 边缘节点缓存，后续访问直接从 CDN 返回，无需回源。
                  </Paragraph>
                  <Collapse
                    ghost
                    items={[
                      {
                        key: 'cf-dns',
                        label: '1. DNS 配置',
                        children: (
                          <>
                            <Paragraph>在 Cloudflare Dashboard → DNS → Records 中添加 A 记录：</Paragraph>
                            <ul style={{ paddingLeft: 20, margin: '8px 0' }}>
                              <li>Type: <Text code>A</Text></li>
                              <li>Name: <Text code>img</Text>（即 img.example.com）</li>
                              <li>IPv4: 你的服务器 IP</li>
                              <li>Proxy status: <Text type="warning">Proxied（橙色云朵 ☁️）</Text> — 必须开启</li>
                            </ul>
                            <Alert
                              message="同时确保 SSL/TLS 加密模式设为 Full (strict)"
                              type="info"
                              showIcon
                              style={{ marginTop: 12, borderRadius: 8 }}
                            />
                          </>
                        ),
                      },
                      {
                        key: 'cf-cache',
                        label: '2. Cache Rule 配置',
                        children: (
                          <>
                            <Paragraph>在 Caching → Configuration → Cache Rules 中创建规则：</Paragraph>
                            <ul style={{ paddingLeft: 20, margin: '8px 0' }}>
                              <li><Text strong>规则名称</Text>：Image Cache</li>
                              <li><Text strong>匹配条件</Text>：Hostname equals <Text code>img.example.com</Text></li>
                              <li><Text strong>Cache eligibility</Text>：Eligible for cache</li>
                              <li><Text strong>Edge TTL</Text>：7 days</li>
                              <li><Text strong>Browser TTL</Text>：30 days</li>
                            </ul>
                            <Paragraph type="secondary" style={{ marginTop: 12 }}>
                              替代方案：也可使用旧版 Page Rule，URL 匹配 <Text code>img.example.com/*</Text>，设置 Cache Level = Cache Everything。
                            </Paragraph>
                          </>
                        ),
                      },
                      {
                        key: 'cf-verify',
                        label: '3. 验证与状态码',
                        children: (
                          <>
                            <Paragraph>配置完成后使用 curl 验证：</Paragraph>
                            <div style={codeStyle}>{`# 首次访问
curl -I https://img.example.com/file/{file_id}
# → CF-Cache-Status: MISS

# 第二次访问
curl -I https://img.example.com/file/{file_id}
# → CF-Cache-Status: HIT`}</div>
                            <Paragraph style={{ marginTop: 16 }}><Text strong>CF-Cache-Status 含义：</Text></Paragraph>
                            <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 16px', fontSize: 13 }}>
                              <Tag color="blue">MISS</Tag><span>未命中缓存，回源获取</span>
                              <Tag color="green">HIT</Tag><span>命中缓存，直接返回</span>
                              <Tag color="orange">EXPIRED</Tag><span>缓存过期，回源刷新</span>
                              <Tag color="default">STALE</Tag><span>返回过期缓存，同时后台刷新</span>
                              <Tag color="default">REVALIDATED</Tag><span>ETag 验证缓存仍有效</span>
                              <Tag color="red">BYPASS</Tag><span>缓存被跳过</span>
                            </div>
                          </>
                        ),
                      },
                      {
                        key: 'cf-purge',
                        label: '4. 缓存清除',
                        children: (
                          <>
                            <Paragraph>在 Cloudflare Dashboard → Caching → Configuration 中：</Paragraph>
                            <ul style={{ paddingLeft: 20, margin: '8px 0' }}>
                              <li><Text strong>Purge Everything</Text>：清除所有缓存</li>
                              <li><Text strong>Custom Purge</Text>：输入完整 URL 清除指定图片缓存</li>
                            </ul>
                          </>
                        ),
                      },
                    ]}
                  />
                </Card>
              </div>
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

                {/* 并发配置 */}
                <div style={{ marginBottom: 24, display: 'flex', alignItems: 'center', gap: 12 }}>
                  <span style={{ fontWeight: 500, whiteSpace: 'nowrap' }}>AI 并发数</span>
                  <InputNumber
                    min={1}
                    max={10}
                    value={parseInt(form.getFieldValue('ai_concurrency') || '5')}
                    onChange={(val) => form.setFieldValue('ai_concurrency', String(val || 5))}
                    style={{ width: 80 }}
                  />
                  <span style={{ color: '#999', fontSize: 13 }}>同时处理的最大记录数（1-10，默认 5）</span>
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
            <InputNumber min={0} max={300000} step={100} style={{ width: '100%' }} placeholder="1000" />
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
