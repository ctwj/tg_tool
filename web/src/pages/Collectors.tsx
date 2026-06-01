import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Switch, Space, message, Tag, Popconfirm } from 'antd'
import { PlusOutlined, DeleteOutlined, CloudDownloadOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { Collector } from '../types'

const Collectors: React.FC = () => {
  const [collectors, setCollectors] = useState<Collector[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [form] = Form.useForm()

  const fetchCollectors = async () => {
    setLoading(true)
    try {
      const res = await apiClient.get('/api/collectors')
      setCollectors(res.data.data?.list ?? [])
    } catch { message.error('获取采集器失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchCollectors() }, [])

  const createCollector = async (values: any) => {
    try {
      await apiClient.post('/api/collectors', values)
      message.success('采集器已创建')
      setModalOpen(false)
      form.resetFields()
      fetchCollectors()
    } catch (e: any) { message.error(e.message || '创建失败') }
  }

  const toggleCollector = async (id: number) => {
    try { await apiClient.put(`/api/collectors/${id}/toggle`); fetchCollectors() }
    catch (e: any) { message.error(e.message || '切换失败') }
  }

  const deleteCollector = async (id: number) => {
    try { await apiClient.delete(`/api/collectors/${id}`); message.success('已删除'); fetchCollectors() }
    catch (e: any) { message.error(e.message || '删除失败') }
  }

  const fetchHistory = async (id: number) => {
    try { await apiClient.post(`/api/collectors/${id}/fetch`); message.success('采集已触发') }
    catch (e: any) { message.error(e.message || '触发失败') }
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '频道', dataIndex: 'channel_name', key: 'channel_name' },
    { title: '频道ID', dataIndex: 'channel_id', key: 'channel_id' },
    { title: '类型', dataIndex: 'collector_type', key: 'collector_type', width: 100,
      render: (v: string) => <Tag>{v}</Tag> },
    { title: '激活', dataIndex: 'is_active', key: 'is_active', width: 80,
      render: (v: boolean, r: Collector) => <Switch checked={v} onChange={() => toggleCollector(r.id)} size="small" /> },
    { title: '操作', key: 'actions', width: 160,
      render: (_: any, r: Collector) => (
        <Space>
          <Button size="small" icon={<CloudDownloadOutlined />} onClick={() => fetchHistory(r.id)}>采集</Button>
          <Popconfirm title="确定删除？" onConfirm={() => deleteCollector(r.id)}>
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <h2>采集器管理</h2>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>创建采集器</Button>
      </div>
      <Table dataSource={collectors} columns={columns} rowKey="id" loading={loading} />
      <Modal title="创建采集器" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()}>
        <Form form={form} onFinish={createCollector} layout="vertical">
          <Form.Item name="channel_id" label="频道 ID" rules={[{ required: true }]}>
            <Input placeholder="-100xxxxxxxxxx" />
          </Form.Item>
          <Form.Item name="channel_name" label="频道名称">
            <Input />
          </Form.Item>
          <Form.Item name="collector_type" label="采集类型" rules={[{ required: true }]}>
            <Select options={[{ value: 'origin', label: '原始' }, { value: 'fullnetwork', label: '全网络' }]} />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Collectors
