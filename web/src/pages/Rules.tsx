import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Switch, message, Tag, Popconfirm, Spin } from 'antd'
import { PlusOutlined, DeleteOutlined, SwapRightOutlined, ImportOutlined, ExportOutlined, FilterOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { Rule, Client, Chat } from '../types'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

/* ═══════════════════ 分栏面板样式 ═══════════════════ */
const panelStyle: React.CSSProperties = {
  flex: 1,
  padding: '20px 24px 16px',
  display: 'flex',
  flexDirection: 'column',
  gap: 0,
  minWidth: 0,
}

const panelTitleStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  fontSize: 15,
  fontWeight: 600,
  color: '#0c4a6e',
  marginBottom: 16,
  paddingBottom: 10,
  borderBottom: '1.5px solid #bae6fd',
}

const iconBox = (color: string): React.CSSProperties => ({
  width: 28,
  height: 28,
  borderRadius: 6,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: color,
  color: '#fff',
  fontSize: 14,
  flexShrink: 0,
})

const arrowBox: React.CSSProperties = {
  width: 32,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  color: '#0ea5e9',
  fontSize: 18,
}

/* ═══════════════════ 主组件 ═══════════════════ */
const Rules: React.FC = () => {
  const [rules, setRules] = useState<Rule[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [editRule, setEditRule] = useState<Rule | null>(null)
  const [form] = Form.useForm()

  // 客户端和频道列表
  const [clients, setClients] = useState<Client[]>([])
  const [chats, setChats] = useState<Chat[]>([])
  const [selectedClientId, setSelectedClientId] = useState<string>('')
  const [chatsLoading, setChatsLoading] = useState(false)

  const forwardMethod = Form.useWatch('forward_method', form)
  const filterMode = Form.useWatch('filter_mode', form)

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

  const fetchChats = async (clientId: string): Promise<Chat[]> => {
    if (!clientId) return []
    const client = clients.find(c => c.id === clientId)
    const endpoint = client?.client_type === 'Bot' ? `/clients/${clientId}/bot-chats` : `/clients/${clientId}/chats`
    const res = await apiClient.get(endpoint)
    const list: Chat[] = res.data.data?.chats ?? []
    return list.filter(c => c.type === 'channel' || c.type === 'group')
  }

  useEffect(() => { fetchRules(); fetchClients() }, [])

  // 选择客户端 → 加载频道（源和目标共用）
  const onClientChange = async (clientId: string) => {
    setSelectedClientId(clientId)
    form.setFieldsValue({
      source_chat_id: undefined,
      source_chat_name: undefined,
      forward_target: undefined,
      forward_client_id: clientId || undefined,
      source_client_id: clientId || undefined,
    })
    setChats([])
    if (!clientId) return
    setChatsLoading(true)
    try {
      const list = await fetchChats(clientId)
      setChats(list)
    } catch (e: any) {
      message.error('获取频道列表失败：' + (e.response?.data?.error || e.message))
    } finally {
      setChatsLoading(false)
    }
  }

  const onSourceChatChange = (chatId: number) => {
    const chat = chats.find(c => c.id === chatId)
    if (chat) {
      form.setFieldsValue({ source_chat_name: chat.name })
    }
  }

  const resetModalState = () => {
    setModalOpen(false)
    form.resetFields()
    setEditRule(null)
    setSelectedClientId('')
    setChats([])
  }

  const openCreateModal = () => {
    setEditRule(null)
    form.resetFields()
    setSelectedClientId('')
    setChats([])
    setModalOpen(true)
  }

  const openEditModal = async (rule: Rule) => {
    setEditRule(rule)
    form.setFieldsValue({
      source_chat_id: rule.source_chat_id,
      source_chat_name: rule.source_chat_name,
      forward_method: rule.forward_method,
      forward_target: rule.forward_target,
      forward_config: rule.forward_config,
      remark: rule.remark,
      forward_client_id: rule.forward_client_id || rule.source_client_id,
      source_client_id: rule.source_client_id || rule.forward_client_id,
      filter_mode: rule.filter_mode || 'none',
      keywords: rule.keywords,
      media_filter: rule.media_filter || 'all',
    })
    // 加载该客户端的频道列表
    const clientId = rule.source_client_id || rule.forward_client_id || ''
    setSelectedClientId(clientId)
    if (clientId) {
      setChatsLoading(true)
      try {
        const list = await fetchChats(clientId)
        setChats(list)
      } catch { /* ignore */ } finally {
        setChatsLoading(false)
      }
    } else {
      setChats([])
    }
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
        forward_client_id: selectedClientId || undefined,
        source_client_id: selectedClientId || undefined,
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
      resetModalState()
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
    if (rule.source_client_id || rule.forward_client_id) {
      tags.push(<Tag key="fc" color="purple" style={{ margin: 0 }}>客户端: {getClientName(rule.source_client_id || rule.forward_client_id)}</Tag>)
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

  // 从 chats 中排除已选为源频道的，用于目标选择
  const sourceChatId = Form.useWatch('source_chat_id', form)

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

      {/* ══════════ 左右分栏弹窗 ══════════ */}
      <Modal
        title={
          <div style={{ fontSize: 16, fontWeight: 600, color: '#0c4a6e' }}>
            {editRule ? '编辑转发规则' : '创建转发规则'}
          </div>
        }
        open={modalOpen}
        onCancel={resetModalState}
        onOk={() => form.submit()}
        width={920}
        okText={editRule ? '保存' : '创建'}
        destroyOnClose
        styles={{
          body: { padding: 0 },
        }}
      >
        <Form form={form} onFinish={submitRule} layout="vertical" initialValues={{ forward_method: 'Chat', filter_mode: 'none', media_filter: 'all' }}>
          <div style={{ display: 'flex', padding: '16px 0 0' }}>
            {/* ──── 左栏：数据源 + 过滤 ──── */}
            <div style={panelStyle}>
              <div style={panelTitleStyle}>
                <span style={iconBox('#0ea5e9')}><ImportOutlined style={{ fontSize: 14 }} /></span>
                <span>数据源</span>
              </div>

              <Form.Item label="客户端" required>
                <Select
                  placeholder="选择客户端以加载频道列表"
                  value={selectedClientId || undefined}
                  onChange={onClientChange}
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
                  placeholder={selectedClientId ? '加载中...' : '请先选择客户端'}
                  onChange={onSourceChatChange}
                  loading={chatsLoading}
                  disabled={!selectedClientId || chatsLoading}
                  showSearch
                  optionFilterProp="label"
                  notFoundContent={
                    !selectedClientId ? '请先选择客户端' :
                    chatsLoading ? <Spin size="small" /> :
                    chats.length === 0 ? '没有可用的频道或群组' : undefined
                  }
                >
                  {chats.map(c => (
                    <Select.Option key={c.id} value={c.id} label={c.name}>
                      [{c.type === 'channel' ? '频道' : '群组'}] {c.name}
                      <span style={{ color: '#9ca3af', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                    </Select.Option>
                  ))}
                </Select>
              </Form.Item>

              <Form.Item name="source_chat_name" label="频道名称">
                <Input placeholder="自动填入" disabled />
              </Form.Item>

              {/* 过滤条件 */}
              <div style={{ marginTop: 8, borderTop: '1px dashed #e5e7eb', paddingTop: 12 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: '#0c4a6e', marginBottom: 12, display: 'flex', alignItems: 'center', gap: 6 }}>
                  <FilterOutlined style={{ color: '#0ea5e9', fontSize: 14 }} /> 过滤条件
                </div>

                <div style={{ display: 'flex', gap: 12 }}>
                  <Form.Item name="filter_mode" label="关键词过滤" style={{ flex: 1 }}>
                    <Select
                      options={[
                        { value: 'none', label: '不过滤' },
                        { value: 'include', label: '白名单' },
                        { value: 'exclude', label: '黑名单' },
                      ]}
                    />
                  </Form.Item>

                  <Form.Item name="media_filter" label="媒体类型" style={{ flex: 1 }}>
                    <Select
                      options={[
                        { value: 'all', label: '全部' },
                        { value: 'photo', label: '仅图片' },
                        { value: 'document', label: '仅文档' },
                        { value: 'text', label: '仅文本' },
                      ]}
                    />
                  </Form.Item>
                </div>

                {filterMode && filterMode !== 'none' && (
                  <Form.Item name="keywords" label="关键词" extra="多个关键词用英文逗号分隔，任一匹配即生效">
                    <Input placeholder={filterMode === 'exclude' ? '广告,推广,加微信' : '资源,分享'} />
                  </Form.Item>
                )}
              </div>
            </div>

            {/* ──── 中间箭头 ──── */}
            <div style={arrowBox}>
              <SwapRightOutlined />
            </div>

            {/* ──── 右栏：转发目标 ──── */}
            <div style={{ ...panelStyle, borderLeft: '1px solid #e5e7eb' }}>
              <div style={panelTitleStyle}>
                <span style={iconBox('#8b5cf6')}><ExportOutlined style={{ fontSize: 14 }} /></span>
                <span>转发目标</span>
              </div>

              <Form.Item name="forward_method" label="转发方式" rules={[{ required: true }]}>
                <Select options={[{ value: 'Chat', label: '转发到聊天' }, { value: 'Webhook', label: 'Webhook' }]} />
              </Form.Item>

              {forwardMethod === 'Chat' && (
                <Form.Item name="forward_target" label="目标频道/群组" rules={[{ required: true, message: '请选择目标' }]}>
                  <Select
                    placeholder={chatsLoading ? '加载中...' : selectedClientId ? '选择目标频道或群组' : '请先选择客户端'}
                    loading={chatsLoading}
                    disabled={!selectedClientId || chatsLoading}
                    showSearch
                    optionFilterProp="label"
                    notFoundContent={
                      !selectedClientId ? '请先选择客户端' :
                      chatsLoading ? <Spin size="small" /> :
                      chats.length === 0 ? '没有可用的频道或群组' : undefined
                    }
                  >
                    {chats
                      .filter(c => c.id !== sourceChatId) // 排除源频道
                      .map(c => (
                        <Select.Option key={c.id} value={String(c.id)} label={c.name}>
                          [{c.type === 'channel' ? '频道' : '群组'}] {c.name}
                          <span style={{ color: '#9ca3af', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                        </Select.Option>
                      ))}
                  </Select>
                </Form.Item>
              )}

              {forwardMethod === 'Webhook' && (
                <>
                  <Form.Item name="forward_target" label="Webhook URL" rules={[{ required: true, message: '请输入 Webhook URL' }]}>
                    <Input placeholder="https://example.com/webhook" />
                  </Form.Item>
                  <Form.Item name="forward_config" label="Webhook 配置">
                    <Input.TextArea rows={3} placeholder='{"webhook_url": "...", "method": "POST"}' />
                  </Form.Item>
                </>
              )}

              {/* 隐藏字段 */}
              <Form.Item name="forward_client_id" hidden><Input /></Form.Item>
              <Form.Item name="source_client_id" hidden><Input /></Form.Item>

              {/* 备注 */}
              <Form.Item name="remark" label="备注" style={{ marginTop: 'auto' }}>
                <Input placeholder="可选备注" />
              </Form.Item>
            </div>
          </div>
        </Form>
      </Modal>
    </div>
  )
}

export default Rules
