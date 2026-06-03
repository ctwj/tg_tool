import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Switch, message, Tag, Popconfirm } from 'antd'
import { PlusOutlined, DeleteOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { Rule } from '../types'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'

const Rules: React.FC = () => {
  const [rules, setRules] = useState<Rule[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()

  const fetchRules = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/rules')
      setRules(res.data.data?.list ?? [])
    } catch { message.error('获取规则失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchRules() }, [])

  const createRule = async (values: any) => {
    try {
      await apiClient.post('/rules', values)
      message.success('规则已创建')
      setModalOpen(false)
      form.resetFields()
      fetchRules()
    } catch (e: any) { message.error(e.message || '创建失败') }
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

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '源频道', dataIndex: 'source_chat_name', key: 'source_chat_name' },
    {
      title: '转发方式',
      dataIndex: 'forward_method',
      key: 'forward_method',
      width: 100,
      render: (v: string) => (
        <Tag color={v === 'Chat' ? '#6366f1' : '#8b5cf6'} style={{ margin: 0 }}>{v}</Tag>
      ),
    },
    { title: '目标', dataIndex: 'forward_target', key: 'forward_target', ellipsis: true },
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
      width: 80,
      render: (_: any, r: Rule) => (
        <Popconfirm title="确定删除？" onConfirm={() => deleteRule(r.id)}>
          <Button size="small" type="text" danger icon={<DeleteOutlined />} />
        </Popconfirm>
      ),
    },
  ]

  const { containerRef, scrollY } = useTableScrollY()

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="转发规则"
        description="管理消息转发到聊天或 Webhook 的规则"
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
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
      <Modal title="创建转发规则" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()} width={500}>
        <Form form={form} onFinish={createRule} layout="vertical">
          <Form.Item name="source_chat_id" label="源频道 ID" rules={[{ required: true }]}>
            <Input placeholder="-100xxxxxxxxxx" />
          </Form.Item>
          <Form.Item name="source_chat_name" label="源频道名称">
            <Input placeholder="频道名称" />
          </Form.Item>
          <Form.Item name="forward_method" label="转发方式" rules={[{ required: true }]}>
            <Select options={[{ value: 'Chat', label: '转发到聊天' }, { value: 'Webhook', label: 'Webhook' }]} />
          </Form.Item>
          <Form.Item name="forward_target" label="目标聊天 ID">
            <Input placeholder="Chat 方式时填写" />
          </Form.Item>
          <Form.Item name="forward_config" label="Webhook 配置">
            <Input.TextArea rows={2} placeholder='{"webhook_url": "...", "method": "POST"}' />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Rules
