import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Tag, Popconfirm, Badge } from 'antd'
import { PlusOutlined, PlayCircleOutlined, PauseCircleOutlined, DeleteOutlined, KeyOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import apiClient from '../api/client'
import type { Client } from '../types'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

const Clients: React.FC = () => {
  const [clients, setClients] = useState<Client[]>([])
  const [loading, setLoading] = useState(false)
  const navigate = useNavigate()
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()

  const fetchClients = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/clients')
      setClients(res.data.data?.list ?? [])
    } catch { message.error('获取客户端列表失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchClients() }, [])

  const addClient = async (values: any) => {
    try {
      await apiClient.post('/clients', values)
      message.success('客户端已添加')
      setModalOpen(false)
      form.resetFields()
      fetchClients()
    } catch (e: any) { message.error(e.message || '添加失败') }
  }

  const removeClient = async (id: string) => {
    try {
      await apiClient.delete(`/clients/${id}`)
      message.success('已删除')
      fetchClients()
    } catch (e: any) { message.error(e.message || '删除失败') }
  }

  const startClient = async (id: string) => {
    try {
      await apiClient.post(`/clients/${id}/start`)
      message.success('启动中...')
      fetchClients()
    } catch (e: any) { message.error(e.message || '启动失败') }
  }

  const stopClient = async (id: string) => {
    try {
      await apiClient.post(`/clients/${id}/stop`)
      message.success('已停止')
      fetchClients()
    } catch (e: any) { message.error(e.message || '停止失败') }
  }

  const statusConfig: Record<string, { color: string; text: string }> = {
    active: { color: '#10b981', text: '在线' },
    new: { color: '#6366f1', text: '新建' },
    wait_code: { color: '#f59e0b', text: '等待验证码' },
    wait_password: { color: '#f59e0b', text: '等待密码' },
    offline: { color: '#ef4444', text: '离线' },
  }

  const columns = [
    {
      title: 'ID',
      dataIndex: 'id',
      key: 'id',
      width: 120,
      render: (id: string) => (
        <code style={{ fontSize: 12, color: '#6366f1', background: '#eef2ff', padding: '2px 8px', borderRadius: 4 }}>
          {id.substring(0, 8)}...
        </code>
      ),
    },
    {
      title: '类型',
      dataIndex: 'client_type',
      key: 'client_type',
      width: 80,
      render: (v: string) => (
        <Tag color={v === 'Bot' ? '#6366f1' : '#06b6d4'} style={{ margin: 0 }}>
          {v === 'Bot' ? 'Bot' : '用户'}
        </Tag>
      ),
    },
    { title: '手机号', dataIndex: 'phone', key: 'phone' },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 120,
      render: (status: string) => {
        const cfg = statusConfig[status] || { color: '#6b7280', text: status }
        return (
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <Badge color={cfg.color} />
            <span style={{ fontSize: 13, color: cfg.color }}>{cfg.text}</span>
          </div>
        )
      },
    },
    {
      title: '操作',
      key: 'actions',
      width: 240,
      render: (_: any, record: Client) => (
        <Space size={4}>
          <Button
            size="small"
            type="text"
            icon={<PlayCircleOutlined />}
            onClick={() => startClient(record.id)}
            disabled={record.status === 'active'}
            style={{ color: record.status === 'active' ? undefined : '#10b981' }}
          >
            启动
          </Button>
          <Button
            size="small"
            type="text"
            icon={<PauseCircleOutlined />}
            onClick={() => stopClient(record.id)}
            disabled={record.status === 'offline' || record.status === 'new'}
          >
            停止
          </Button>
          {(record.status === 'wait_code' || record.status === 'wait_password') && (
            <Button
              size="small"
              type="primary"
              icon={<KeyOutlined />}
              onClick={() => navigate(`/client-auth?id=${record.id}`)}
            >
              认证
            </Button>
          )}
          <Popconfirm title="确定删除？" onConfirm={() => removeClient(record.id)}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  const { containerRef, scrollY } = useTableScrollY()

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="客户端管理"
        description="管理 Telegram 客户端和 Bot 连接"
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
            添加客户端
          </Button>
        }
      />
      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={clients}
          columns={columns}
          rowKey="id"
          loading={loading}
          scroll={{ y: scrollY }}
          style={{ background: '#fff', borderRadius: 12 }}
        />
      </div>
      <Modal
        title="添加客户端"
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        onOk={() => form.submit()}
      >
        <Form form={form} onFinish={addClient} layout="vertical">
          <Form.Item name="client_type" label="类型" rules={[{ required: true }]}>
            <Select options={[{ value: 'Client', label: '用户账号' }, { value: 'Bot', label: 'Bot' }]} />
          </Form.Item>
          <Form.Item name="phone" label="手机号">
            <Input placeholder="用户账号需要手机号" />
          </Form.Item>
          <Form.Item name="token" label="Bot Token">
            <Input placeholder="Bot 需要 Token" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Clients
