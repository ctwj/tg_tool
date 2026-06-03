import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Popconfirm, Tag } from 'antd'
import { PlusOutlined, DeleteOutlined, KeyOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

const Users: React.FC = () => {
  const [users, setUsers] = useState<any[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()

  // 修改密码
  const [pwdModalOpen, setPwdModalOpen] = useState(false)
  const [pwdUser, setPwdUser] = useState<any>(null)
  const [pwdForm] = Form.useForm()
  const [pwdLoading, setPwdLoading] = useState(false)

  const fetchUsers = async () => {
    setLoading(true)
    try { const res = await apiClient.get('/users'); setUsers(res.data.data?.list ?? []) }
    catch { message.error('获取用户列表失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchUsers() }, [])

  const createUser = async (values: any) => {
    try {
      await apiClient.post('/users', values)
      message.success('用户已创建')
      setModalOpen(false)
      form.resetFields()
      fetchUsers()
    } catch (e: any) { message.error(e.message || '创建失败') }
  }

  const deleteUser = async (id: number) => {
    try { await apiClient.delete(`/users/${id}`); message.success('已删除'); fetchUsers() }
    catch (e: any) { message.error(e.message || '删除失败') }
  }

  const openPwdModal = (user: any) => {
    setPwdUser(user)
    pwdForm.resetFields()
    setPwdModalOpen(true)
  }

  const changePassword = async (values: any) => {
    if (!pwdUser) return
    setPwdLoading(true)
    try {
      await apiClient.put(`/users/${pwdUser.id}`, { password: values.new_password })
      message.success('密码已修改')
      setPwdModalOpen(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '修改失败')
    } finally {
      setPwdLoading(false)
    }
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
    { title: '操作', key: 'actions', width: 120,
      render: (_: any, r: any) => (
        <Space size={4}>
          <Button size="small" type="text" icon={<KeyOutlined />} onClick={() => openPwdModal(r)}
            style={{ color: '#f59e0b' }}>
            改密
          </Button>
          {r.id !== 1 && (
            <Popconfirm title="确定删除？" onConfirm={() => deleteUser(r.id)}>
              <Button size="small" type="text" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          )}
        </Space>
      ),
    },
  ]

  const { containerRef, scrollY } = useTableScrollY()

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="用户管理"
        description="管理系统用户账号和权限"
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
            创建用户
          </Button>
        }
      />
      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={users}
          columns={columns}
          rowKey="id"
          loading={loading}
          scroll={{ y: scrollY }}
          style={{ background: '#fff', borderRadius: 12 }}
        />
      </div>
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

      {/* 修改密码弹窗 */}
      <Modal
        title={`修改密码 — ${pwdUser?.username || ''}`}
        open={pwdModalOpen}
        onCancel={() => setPwdModalOpen(false)}
        onOk={() => pwdForm.submit()}
        confirmLoading={pwdLoading}
        okText="确认修改"
      >
        <Form form={pwdForm} onFinish={changePassword} layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item
            name="new_password"
            label="新密码"
            rules={[
              { required: true, message: '请输入新密码' },
              { min: 6, message: '密码至少 6 位' },
            ]}
          >
            <Input.Password placeholder="请输入新密码" />
          </Form.Item>
          <Form.Item
            name="confirm_password"
            label="确认密码"
            dependencies={['new_password']}
            rules={[
              { required: true, message: '请确认密码' },
              ({ getFieldValue }) => ({
                validator(_, value) {
                  if (!value || getFieldValue('new_password') === value) {
                    return Promise.resolve()
                  }
                  return Promise.reject(new Error('两次输入的密码不一致'))
                },
              }),
            ]}
          >
            <Input.Password placeholder="请再次输入新密码" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Users
