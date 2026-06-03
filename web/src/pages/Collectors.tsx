import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Switch, Space, message, Tag, Popconfirm, Spin } from 'antd'
import { PlusOutlined, DeleteOutlined, CloudDownloadOutlined, FileSearchOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import apiClient from '../api/client'
import type { Collector, Client, Chat } from '../types'
import PageHeader from '../components/PageHeader'

const Collectors: React.FC = () => {
  const [collectors, setCollectors] = useState<Collector[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()
  const navigate = useNavigate()

  // 客户端和频道列表
  const [clients, setClients] = useState<Client[]>([])
  const [chats, setChats] = useState<Chat[]>([])
  const [chatsLoading, setChatsLoading] = useState(false)
  const [selectedClientId, setSelectedClientId] = useState<string>('')

  const fetchCollectors = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/collectors')
      setCollectors(res.data.data?.list ?? [])
    } catch { message.error('获取采集器失败') }
    finally { setLoading(false) }
  }

  const fetchClients = async () => {
    try {
      const res = await apiClient.get('/clients')
      const list: Client[] = res.data.data?.list ?? []
      setClients(list.filter(c => c.status === 'active'))
    } catch { /* ignore */ }
  }

  const fetchChats = async (clientId: string) => {
    if (!clientId) { setChats([]); return }
    setChatsLoading(true)
    setChats([])
    try {
      const res = await apiClient.get(`/clients/${clientId}/chats`)
      const list: Chat[] = res.data.data?.chats ?? []
      setChats(list.filter(c => c.type === 'channel' || c.type === 'group'))
    } catch (e: any) {
      message.error('获取频道列表失败：' + (e.response?.data?.error || e.message))
    } finally {
      setChatsLoading(false)
    }
  }

  useEffect(() => { fetchCollectors(); fetchClients() }, [])

  const onClientChange = (clientId: string) => {
    setSelectedClientId(clientId)
    form.setFieldsValue({ channel_id: undefined, channel_name: undefined })
    fetchChats(clientId)
  }

  const onChannelChange = (channelId: number) => {
    const chat = chats.find(c => c.id === channelId)
    if (chat) {
      form.setFieldsValue({ channel_name: chat.name })
    }
  }

  const createCollector = async (values: any) => {
    try {
      await apiClient.post('/collectors', {
        client_id: selectedClientId,
        channel_id: values.channel_id,
        channel_name: values.channel_name,
        collector_type: 'origin',
        remark: values.remark || '',
      })
      message.success('采集器已创建')
      setModalOpen(false)
      form.resetFields()
      setSelectedClientId('')
      setChats([])
      fetchCollectors()
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '创建失败')
    }
  }

  const toggleCollector = async (id: number) => {
    try { await apiClient.put(`/collectors/${id}/toggle`); fetchCollectors() }
    catch (e: any) { message.error(e.message || '切换失败') }
  }

  const deleteCollector = async (id: number) => {
    try { await apiClient.delete(`/collectors/${id}`); message.success('已删除'); fetchCollectors() }
    catch (e: any) { message.error(e.message || '删除失败') }
  }

  // 采集弹窗
  const [fetchOpen, setFetchOpen] = useState(false)
  const [fetchCollectorId, setFetchCollectorId] = useState<number>(0)
  const [fetchLimit, setFetchLimit] = useState<number>(1000)
  const [fetching, setFetching] = useState(false)

  const openFetchDialog = (collector: Collector) => {
    setFetchCollectorId(collector.id)
    setFetchLimit(1000)
    setFetchOpen(true)
  }

  const triggerFetch = async () => {
    setFetching(true)
    try {
      const res = await apiClient.post(`/collectors/${fetchCollectorId}/fetch`, { limit: fetchLimit })
      message.success(res.data?.data?.message || '采集完成')
      setFetchOpen(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '采集失败')
    } finally {
      setFetching(false)
    }
  }

  // 找到客户端名称
  const getClientName = (clientId?: string) => {
    if (!clientId) return '-'
    const c = clients.find(c => c.id === clientId)
    return c ? (c.phone || c.id.substring(0, 8) + '...') : clientId.substring(0, 8) + '...'
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    {
      title: '客户端', dataIndex: 'client_id', key: 'client_id', width: 140,
      render: (v: string) => <Tag color="purple" style={{ margin: 0 }}>{getClientName(v)}</Tag>,
    },
    { title: '频道', dataIndex: 'channel_name', key: 'channel_name' },
    {
      title: '频道ID', dataIndex: 'channel_id', key: 'channel_id', width: 140,
      render: (v: number) => <code style={{ fontSize: 12, color: '#6366f1', background: '#eef2ff', padding: '2px 8px', borderRadius: 4 }}>{v}</code>,
    },
    {
      title: '激活', dataIndex: 'is_active', key: 'is_active', width: 80,
      render: (v: boolean, r: Collector) => <Switch checked={v} onChange={() => toggleCollector(r.id)} size="small" />,
    },
    {
      title: '操作', key: 'actions', width: 220,
      render: (_: any, r: Collector) => (
        <Space size={4}>
          <Button size="small" type="text" icon={<FileSearchOutlined />} onClick={() => navigate(`/collectors/${r.id}/history`, { state: { channel_name: r.channel_name, channel_id: r.channel_id } })}>
            记录
          </Button>
          <Button size="small" type="text" icon={<CloudDownloadOutlined />} onClick={() => openFetchDialog(r)} style={{ color: '#6366f1' }}>采集</Button>
          <Popconfirm title="确定删除？" onConfirm={() => deleteCollector(r.id)}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title="采集器管理"
        description="管理频道消息采集任务"
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => { setModalOpen(true); fetchClients() }}>
            创建采集器
          </Button>
        }
      />
      <Table
        dataSource={collectors}
        columns={columns}
        rowKey="id"
        loading={loading}
        style={{ background: '#fff', borderRadius: 12 }}
      />

      {/* 创建采集器弹窗 */}
      <Modal
        title="创建采集器"
        open={modalOpen}
        onCancel={() => { setModalOpen(false); form.resetFields(); setSelectedClientId(''); setChats([]) }}
        onOk={() => form.submit()}
        width={560}
      >
        <Form form={form} onFinish={createCollector} layout="vertical">
          <Form.Item name="client_id" label="选择客户端" rules={[{ required: true, message: '请选择客户端' }]}>
            <Select
              placeholder="请先选择一个已连接的客户端"
              onChange={onClientChange}
              notFoundContent={clients.length === 0 ? '没有活跃的客户端，请先添加并启动客户端' : undefined}
            >
              {clients.map(c => (
                <Select.Option key={c.id} value={c.id}>
                  {c.client_type === 'Bot' ? 'Bot ' : ''}
                  {c.phone || c.id.substring(0, 8)}...
                  <Tag color={c.status === 'active' ? 'green' : 'default'} style={{ marginLeft: 8 }}>
                    {c.status}
                  </Tag>
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item name="channel_id" label="频道/群组" rules={[{ required: true, message: '请选择频道或群组' }]}>
            <Select
              placeholder={selectedClientId ? '加载中...' : '请先选择客户端'}
              onChange={onChannelChange}
              loading={chatsLoading}
              disabled={!selectedClientId || chatsLoading}
              showSearch
              optionFilterProp="label"
              notFoundContent={
                !selectedClientId ? '请先选择客户端' :
                chatsLoading ? <Spin size="small" /> :
                chats.length === 0 ? '该客户端没有可采集的频道或群组' : undefined
              }
            >
              {chats.map(c => (
                <Select.Option key={c.id} value={c.id} label={c.name}>
                  {c.type === 'channel' ? '频道' : '群组'} {c.name}
                  <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>({c.id})</span>
                </Select.Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item name="channel_name" label="频道名称">
            <Input placeholder="自动填入" disabled />
          </Form.Item>

          <Form.Item name="remark" label="备注">
            <Input placeholder="可选备注" />
          </Form.Item>
        </Form>
      </Modal>

      {/* 采集数量选择弹窗 */}
      <Modal
        title="批量采集"
        open={fetchOpen}
        onCancel={() => setFetchOpen(false)}
        onOk={triggerFetch}
        confirmLoading={fetching}
        okText="开始采集"
        width={420}
      >
        <div style={{ marginBottom: 16 }}>
          选择要采集的消息数量。已采集过的消息不会重复写入。
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <span>采集数量：</span>
          <Select
            value={fetchLimit}
            onChange={setFetchLimit}
            style={{ width: 180 }}
            options={[
              { value: 100, label: '最近 100 条' },
              { value: 500, label: '最近 500 条' },
              { value: 1000, label: '最近 1,000 条（默认）' },
              { value: 3000, label: '最近 3,000 条' },
              { value: 5000, label: '最近 5,000 条' },
              { value: 10000, label: '最近 10,000 条（上限）' },
            ]}
          />
        </div>
      </Modal>
    </div>
  )
}

export default Collectors
