import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Tag, Popconfirm } from 'antd'
import { PlusOutlined, PlayCircleOutlined, PauseCircleOutlined, DeleteOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { Client } from '../types'

const Clients: React.FC = () => {
  const [clients, setClients] = useState<Client[]>([])
  const [loading, setLoading] = useState(false)
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

  const statusColor: Record<string, string> = {
    active: 'green', new: 'blue', wait_code: 'orange', wait_password: 'orange', offline: 'red',
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 120 },
    { title: '类型', dataIndex: 'client_type', key: 'client_type', width: 80 },
    { title: '手机号', dataIndex: 'phone', key: 'phone' },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 100,
      render: (status: string) => <Tag color={statusColor[status] || 'default'}>{status}</Tag>,
    },
    {
      title: '操作', key: 'actions', width: 200,
      render: (_: any, record: Client) => (
        <Space>
          <Button size="small" icon={<PlayCircleOutlined />} onClick={() => startClient(record.id)}>启动</Button>
          <Button size="small" icon={<PauseCircleOutlined />} onClick={() => stopClient(record.id)}>停止</Button>
          <Popconfirm title="确定删除？" onConfirm={() => removeClient(record.id)}>
            <Button size="small" danger icon={<DeleteOutlined />}>删除</Button>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <h2>客户端管理</h2>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>添加客户端</Button>
      </div>
      <Table dataSource={clients} columns={columns} rowKey="id" loading={loading} />
      <Modal title="添加客户端" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()}>
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
