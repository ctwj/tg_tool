import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, message, Popconfirm, Tag } from 'antd'
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons'
import apiClient from '../api/client'

const Users: React.FC = () => {
  const [users, setUsers] = useState<any[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()

  const fetchUsers = async () => {
    setLoading(true)
    try { const res = await apiClient.get('/api/users'); setUsers(res.data.data?.list ?? []) }
    catch { message.error('获取用户列表失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchUsers() }, [])

  const createUser = async (values: any) => {
    try {
      await apiClient.post('/api/users', values)
      message.success('用户已创建')
      setModalOpen(false)
      form.resetFields()
      fetchUsers()
    } catch (e: any) { message.error(e.message || '创建失败') }
  }

  const deleteUser = async (id: number) => {
    try { await apiClient.delete(`/api/users/${id}`); message.success('已删除'); fetchUsers() }
    catch (e: any) { message.error(e.message || '删除失败') }
  }

  const roleTag = (role: number) => {
    if (role >= 100) return <Tag color="red">Root</Tag>
    if (role >= 10) return <Tag color="orange">Admin</Tag>
    return <Tag color="blue">User</Tag>
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '用户名', dataIndex: 'username', key: 'username' },
    { title: '显示名', dataIndex: 'display_name', key: 'display_name' },
    { title: '邮箱', dataIndex: 'email', key: 'email' },
    { title: '角色', dataIndex: 'role', key: 'role', width: 80, render: roleTag },
    { title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: number) => v === 1 ? <Tag color="green">启用</Tag> : <Tag color="red">禁用</Tag> },
    { title: '操作', key: 'actions', width: 80,
      render: (_: any, r: any) => r.id !== 1 ? (
        <Popconfirm title="确定删除？" onConfirm={() => deleteUser(r.id)}>
          <Button size="small" danger icon={<DeleteOutlined />} />
        </Popconfirm>
      ) : null,
    },
  ]

  return (
    <div>
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <h2>用户管理</h2>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>创建用户</Button>
      </div>
      <Table dataSource={users} columns={columns} rowKey="id" loading={loading} />
      <Modal title="创建用户" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()}>
        <Form form={form} onFinish={createUser} layout="vertical">
          <Form.Item name="username" label="用户名" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="password" label="密码" rules={[{ required: true }]}>
            <Input.Password />
          </Form.Item>
          <Form.Item name="email" label="邮箱">
            <Input />
          </Form.Item>
          <Form.Item name="display_name" label="显示名">
            <Input />
          </Form.Item>
          <Form.Item name="role" label="角色" initialValue={1}>
            <Select options={[{ value: 1, label: '普通用户' }, { value: 10, label: '管理员' }, { value: 100, label: 'Root' }]} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Users
