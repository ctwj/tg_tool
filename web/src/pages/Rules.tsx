import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Switch, message, Tag, Popconfirm, Spin, Divider } from 'antd'
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { Rule, Client, Chat } from '../types'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

const Rules: React.FC = () => {
  const [rules, setRules] = useState<Rule[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [editRule, setEditRule] = useState<Rule | null>(null)
  const [form] = Form.useForm()

  // 客户端和频道列表
  const [clients, setClients] = useState<Client[]>([])
  const [sourceChats, setSourceChats] = useState<Chat[]>([])
  const [targetChats, setTargetChats] = useState<Chat[]>([])
  const [sourceClientId, setSourceClientId] = useState<string>('')
  const [targetClientId, setTargetClientId] = useState<string>('')
  const [sourceChatsLoading, setSourceChatsLoading] = useState(false)
  const [targetChatsLoading, setTargetChatsLoading] = useState(false)

  const forwardMethod = Form.useWatch('forward_method', form)

  const fetchRules = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/rules')
      setRules(res.data.data?.list ?? [])
    } catch { message.error('获取规则失败') }
    finally { setLoading(false) }
  }

  const fetchClients = async () => {
    try {
      const res = await apiClient.get('/clients')
      const list: Client[] = res.data.data?.list ?? []
      setClients(list.filter(c => c.status === 'active'))
    } catch { /* ignore */ }
  }

  const fetchChats = async (clientId: string, isBot: boolean): Promise<Chat[]> => {
    if (!clientId) return []
    const endpoint = isBot ? `/clients/${clientId}/bot-chats` : `/clients/${clientId}/chats`
    const res = await apiClient.get(endpoint)
    const list: Chat[] = res.data.data?.chats ?? []
    return list.filter(c => c.type === 'channel' || c.type === 'group')
  }

  useEffect(() => { fetchRules(); fetchClients() }, [])

  // 监听客户端 → 加载源频道
  const onSourceClientChange = async (clientId: string) => {
    setSourceClientId(clientId)
    form.setFieldsValue({ source_chat_id: undefined, source_chat_name: undefined })
    setSourceChats([])
    if (!clientId) return
    const client = clients.find(c => c.id === clientId)
    setSourceChatsLoading(true)
    try {
      const chats = await fetchChats(clientId, client?.client_type === 'Bot')
      setSourceChats(chats)
    } catch (e: any) {
      message.error('获取频道列表失败：' + (e.response?.data?.error || e.message))
    } finally {
      setSourceChatsLoading(false)
    }
  }

  const onSourceChatChange = (chatId: number) => {
    const chat = sourceChats.find(c => c.id === chatId)
    if (chat) {
      form.setFieldsValue({ source_chat_name: chat.name })
    }
  }

  // 监听转发客户端 → 加载目标群组
  const onTargetClientChange = async (clientId: string) => {
    setTargetClientId(clientId)
    form.setFieldsValue({ forward_target: undefined, forward_client_id: clientId || undefined })
    setTargetChats([])
    if (!clientId) return
    const client = clients.find(c => c.id === clientId)
    setTargetChatsLoading(true)
    try {
      const chats = await fetchChats(clientId, client?.client_type === 'Bot')
      setTargetChats(chats)
    } catch (e: any) {
      message.error('获取群组列表失败：' + (e.response?.data?.error || e.message))
    } finally {
      setTargetChatsLoading(false)
    }
  }

  const onTargetChatChange = (_chatId: number) => {
    // forward_target is set via the Select value directly
  }

  const openCreateModal = () => {
    setEditRule(null)
    form.resetFields()
    setSourceClientId('')
    setTargetClientId('')
    setSourceChats([])
    setTargetChats([])
    setModalOpen(true)
  }

  const openEditModal = (rule: Rule) => {
    setEditRule(rule)
    setSourceClientId('')
    setTargetClientId(rule.forward_client_id || '')
    setSourceChats([])
    setTargetChats([])
    form.setFieldsValue({
      source_chat_id: rule.source_chat_id,
      source_chat_name: rule.source_chat_name,
      forward_method: rule.forward_method,
      forward_target: rule.forward_target,
      forward_config: rule.forward_config,
      remark: rule.remark,
      forward_client_id: rule.forward_client_id,
      filter_mode: rule.filter_mode || 'none',
      keywords: rule.keywords,
      media_filter: rule.media_filter || 'all',
    })
    setModalOpen(true)
  }

  const submitRule = async (values: any) => {
    try {
      const payload = {
        source_chat_id: values.source_chat_id,
        source_chat_name: values.source_chat_name,
        forward_method: values.forward_method || 'Chat',
        forward_config: values.forward_config,
        forward_target: values.forward_target,
        remark: values.remark,
        forward_client_id: values.forward_client_id || undefined,
        filter_mode: values.filter_mode && values.filter_mode !== 'none' ? values.filter_mode : undefined,
        keywords: values.keywords || undefined,
        media_filter: values.media_filter && values.media_filter !== 'all' ? values.media_filter : undefined,
      }
      if (editRule) {
        await apiClient.put(`/rules/${editRule.id}`, payload)
        message.success('规则已更新')
      } else {
        await apiClient.post('/rules', { ...payload, is_active: true })
        message.success('规则已创建')
      }
      setModalOpen(false)
      form.resetFields()
      setEditRule(null)
      fetchRules()
    } catch (e: any) { message.error(e.response?.data?.error || e.message || '操作失败') }
  }

  const toggleRule = async (id: number) => {
    try {
      await apiClient.put(`/rules/${id}/toggle`)
      fetchRules()
    } catch (e: any) { message.error(e.message || '切换失败') }
  }

  const deleteRule = async (id: number) => {
    try {
      await apiClient.delete(`/rules/${id}`)
      message.success('已删除')
      fetchRules()
    } catch (e: any) { message.error(e.message || '删除失败') }
  }

  // 找到客户端名称
  const getClientName = (clientId?: string) => {
    if (!clientId) return '-'
    const c = clients.find(c => c.id === clientId)
    return c ? (c.phone || c.id.substring(0, 8) + '...') : clientId.substring(0, 8) + '...'
  }

  // 过滤标签展示
  const renderFilterTags = (_: any, rule: Rule) => {
    const tags: React.ReactNode[] = []
    if (rule.filter_mode && rule.filter_mode !== 'none') {
      const modeText = rule.filter_mode === 'exclude' ? '黑名单' : '白名单'
      tags.push(<Tag key="kw" color={rule.filter_mode === 'exclude' ? 'red' : 'green'} style={{ margin: 0 }}>关键词{modeText}</Tag>)
    }
    if (rule.media_filter && rule.media_filter !== 'all') {
      const mediaMap: Record<string, string> = { photo: '仅图片', document: '仅文档', text: '仅文本' }
      tags.push(<Tag key="mf" color="blue" style={{ margin: 0 }}>{mediaMap[rule.media_filter] || rule.media_filter}</Tag>)
    }
    if (rule.forward_client_id) {
      tags.push(<Tag key="fc" color="purple" style={{ margin: 0 }}>客户端: {getClientName(rule.forward_client_id)}</Tag>)
    }
    return tags.length > 0 ? <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>{tags}</div> : <span style={{ color: '#aaa' }}>无</span>
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '源频道', dataIndex: 'source_chat_name', key: 'source_chat_name', ellipsis: true },
    {
      title: '转发方式',
      dataIndex: 'forward_method',
      key: 'forward_method',
      width: 100,
      render: (v: string) => (
        <Tag color={v === 'Chat' ? '#0ea5e9' : '#8b5cf6'} style={{ margin: 0 }}>{v}</Tag>
      ),
    },
    { title: '目标', dataIndex: 'forward_target', key: 'forward_target', ellipsis: true },
    {
      title: '过滤',
      key: 'filters',
      width: 200,
      render: renderFilterTags,
    },
    { title: '备注', dataIndex: 'remark', key: 'remark', ellipsis: true },
    {
      title: '激活',
      dataIndex: 'is_active',
      key: 'is_active',
      width: 80,
      render: (v: boolean, r: Rule) => <Switch checked={v} onChange={() => toggleRule(r.id)} size="small" />,
    },
    {
      title: '操作',
      key: 'actions',
      width: 120,
      render: (_: any, r: Rule) => (
        <div style={{ display: 'flex', gap: 4 }}>
          <Button size="small" type="text" onClick={() => openEditModal(r)}>编辑</Button>
          <Popconfirm title="确定删除？" onConfirm={() => deleteRule(r.id)}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </div>
      ),
    },
  ]

  const { containerRef, scrollY } = useTableScrollY()

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="转发规则"
        description="管理消息转发到聊天或 Webhook 的规则，支持关键词和媒体类型过滤"
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={openCreateModal}>
            创建规则
          </Button>
        }
      />
      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={rules}
          columns={columns}
          rowKey="id"
          loading={loading}
          scroll={{ y: scrollY }}
          style={{ background: '#fff', borderRadius: 12 }}
        />
      </div>
      <Modal
        title={editRule ? '编辑转发规则' : '创建转发规则'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); form.resetFields(); setEditRule(null); setSourceClientId(''); setTargetClientId(''); setSourceChats([]); setTargetChats([]) }}
        onOk={() => form.submit()}
        width={560}
      >
        <Form form={form} onFinish={submitRule} layout="vertical" initialValues={{ forward_method: 'Chat', filter_mode: 'none', media_filter: 'all' }}>
          {/* ---- 源配置 ---- */}
          <Divider orientation="left" plain style={{ margin: '8px 0 16px' }}>消息来源</Divider>

          <Form.Item label="监听客户端">
            <Select
              placeholder="选择监听客户端以加载频道"
              value={sourceClientId || undefined}
              onChange={onSourceClientChange}
              allowClear
              notFoundContent={clients.length === 0 ? '没有活跃的客户端' : undefined}
            >
              {clients.map(c => (
                <Select.Option key={c.id} value={c.id}>
                  {c.client_type === 'Bot' ? 'Bot ' : ''}{c.phone || c.id.substring(0, 8)}...
                  <Tag color={c.status === 'active' ? 'green' : 'default'} style={{ marginLeft: 8 }}>{c.status}</Tag>
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item name="source_chat_id" label="源频道" rules={[{ required: true, message: '请选择源频道' }]}>
            <Select
              placeholder={sourceClientId ? '加载中...' : '请先选择客户端'}
              onChange={onSourceChatChange}
              loading={sourceChatsLoading}
              disabled={!sourceClientId || sourceChatsLoading}
              showSearch
              optionFilterProp="label"
              notFoundContent={
                !sourceClientId ? '请先选择客户端' :
                sourceChatsLoading ? <Spin size="small" /> :
                sourceChats.length === 0 ? '没有可用的频道或群组' : undefined
              }
            >
              {sourceChats.map(c => (
                <Select.Option key={c.id} value={c.id} label={c.name}>
                  {c.type === 'channel' ? '频道' : '群组'} {c.name}
                  <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item name="source_chat_name" label="源频道名称">
            <Input placeholder="自动填入" disabled />
          </Form.Item>

          {/* ---- 转发配置 ---- */}
          <Divider orientation="left" plain style={{ margin: '8px 0 16px' }}>转发目标</Divider>

          <Form.Item name="forward_method" label="转发方式" rules={[{ required: true }]}>
            <Select options={[{ value: 'Chat', label: '转发到聊天' }, { value: 'Webhook', label: 'Webhook' }]} />
          </Form.Item>

          {forwardMethod === 'Chat' && (
            <>
              <Form.Item label="转发客户端">
                <Select
                  placeholder="选择转发客户端（可选，不选则使用任意在线客户端）"
                  value={targetClientId || undefined}
                  onChange={onTargetClientChange}
                  allowClear
                  onClear={() => { setTargetClientId(''); setTargetChats([]); form.setFieldsValue({ forward_client_id: undefined, forward_target: undefined }) }}
                >
                  {clients.map(c => (
                    <Select.Option key={c.id} value={c.id}>
                      {c.client_type === 'Bot' ? 'Bot ' : ''}{c.phone || c.id.substring(0, 8)}...
                      <Tag color={c.status === 'active' ? 'green' : 'default'} style={{ marginLeft: 8 }}>{c.status}</Tag>
                    </Select.Option>
                  ))}
                </Select>
              </Form.Item>

              {targetClientId && (
                <Form.Item name="forward_target" label="目标群组" rules={[{ required: true, message: '请选择目标群组' }]}>
                  <Select
                    placeholder={targetChatsLoading ? '加载中...' : '选择目标群组'}
                    onChange={onTargetChatChange}
                    loading={targetChatsLoading}
                    disabled={targetChatsLoading}
                    showSearch
                    optionFilterProp="label"
                    notFoundContent={
                      targetChatsLoading ? <Spin size="small" /> :
                      targetChats.length === 0 ? '没有可用的群组' : undefined
                    }
                  >
                    {targetChats.map(c => (
                      <Select.Option key={c.id} value={String(c.id)} label={c.name}>
                        {c.type === 'channel' ? '频道' : '群组'} {c.name}
                        <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                      </Select.Option>
                    ))}
                  </Select>
                </Form.Item>
              )}

              {!targetClientId && (
                <Form.Item name="forward_target" label="目标聊天 ID" rules={[{ required: true, message: '请输入目标聊天 ID' }]}>
                  <Input placeholder="-100xxxxxxxxxx（可先选择转发客户端以使用级联选择）" />
                </Form.Item>
              )}

              <Form.Item name="forward_client_id" hidden>
                <Input />
              </Form.Item>
            </>
          )}

          {forwardMethod === 'Webhook' && (
            <>
              <Form.Item name="forward_target" label="Webhook URL" rules={[{ required: true, message: '请输入 Webhook URL' }]}>
                <Input placeholder="https://example.com/webhook" />
              </Form.Item>
              <Form.Item name="forward_config" label="Webhook 配置">
                <Input.TextArea rows={2} placeholder='{"webhook_url": "...", "method": "POST"}' />
              </Form.Item>
            </>
          )}

          {/* ---- 过滤配置 ---- */}
          <Divider orientation="left" plain style={{ margin: '8px 0 16px' }}>过滤条件（可选）</Divider>

          <div style={{ display: 'flex', gap: 16 }}>
            <Form.Item name="filter_mode" label="关键词过滤" style={{ flex: 1 }}>
              <Select
                options={[
                  { value: 'none', label: '不过滤' },
                  { value: 'include', label: '白名单（含关键词才转发）' },
                  { value: 'exclude', label: '黑名单（含关键词不转发）' },
                ]}
              />
            </Form.Item>

            <Form.Item name="media_filter" label="媒体类型" style={{ flex: 1 }}>
              <Select
                options={[
                  { value: 'all', label: '全部' },
                  { value: 'photo', label: '仅图片' },
                  { value: 'document', label: '仅文档/文件' },
                  { value: 'text', label: '仅纯文本' },
                ]}
              />
            </Form.Item>
          </div>

          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.filter_mode !== cur.filter_mode}>
            {({ getFieldValue }) => {
              const mode = getFieldValue('filter_mode')
              if (mode === 'none' || !mode) return null
              return (
                <Form.Item name="keywords" label="关键词" extra="多个关键词用英文逗号分隔，任一匹配即生效">
                  <Input placeholder={mode === 'exclude' ? '广告,推广,加微信' : '资源,分享'} />
                </Form.Item>
              )
            }}
          </Form.Item>

          <Form.Item name="remark" label="备注">
            <Input placeholder="可选备注" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Rules
