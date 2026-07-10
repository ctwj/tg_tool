import React, { useEffect, useState } from 'react'
import {
  Table,
  Button,
  Modal,
  Form,
  Input,
  Select,
  Tag,
  Space,
  Popconfirm,
  message,
  Card,
} from 'antd'
import {
  PlusOutlined,
  ReloadOutlined,
  DeleteOutlined,
  CheckCircleOutlined,
  EditOutlined,
} from '@ant-design/icons'
import {
  listPanAccounts,
  createPanAccount,
  updatePanAccount,
  deletePanAccount,
  checkPanAccount,
  type PanAccount,
  type CreatePanAccount,
} from '../api/pan'

const PLATFORMS = [
  { value: 'quark', label: '夸克' },
  { value: 'uc', label: 'UC' },
  { value: 'baidu', label: '百度' },
]

const statusTag = (status: string) => {
  const map: Record<string, { color: string; text: string }> = {
    active: { color: 'success', text: '可用' },
    disabled: { color: 'default', text: '未启用' },
    expired: { color: 'error', text: '失效' },
  }
  const s = map[status] || { color: 'default', text: status }
  return <Tag color={s.color}>{s.text}</Tag>
}

const formatBytes = (b: number | null) => {
  if (!b) return '-'
  const gb = b / 1024 / 1024 / 1024
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(b / 1024 / 1024).toFixed(0)} MB`
}

const PanAccounts: React.FC = () => {
  const [data, setData] = useState<PanAccount[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<PanAccount | null>(null)
  const [form] = Form.useForm()

  const load = async () => {
    setLoading(true)
    try {
      setData(await listPanAccounts())
    } catch (e: any) {
      message.error(e.message || '加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
  }, [])

  const onAdd = () => {
    setEditing(null)
    form.resetFields()
    setModalOpen(true)
  }

  const onEdit = (r: PanAccount) => {
    setEditing(r)
    form.setFieldsValue({
      platform: r.platform,
      display_name: r.display_name,
      target_dir: r.target_dir,
    })
    setModalOpen(true)
  }

  const onSubmit = async () => {
    const v = await form.validateFields()
    try {
      if (editing) {
        const upd: Record<string, string> = {
          display_name: v.display_name,
          target_dir: v.target_dir,
        }
        if (v.credential) upd.credential = v.credential
        await updatePanAccount(editing.id, upd)
        message.success('已更新')
      } else {
        await createPanAccount(v as CreatePanAccount)
        message.success('已添加')
      }
      setModalOpen(false)
      load()
    } catch (e: any) {
      message.error(e.message || '保存失败')
    }
  }

  const onDelete = async (id: number) => {
    await deletePanAccount(id)
    message.success('已删除')
    load()
  }

  const onCheck = async (id: number) => {
    try {
      await checkPanAccount(id)
      message.success('校验完成')
      load()
    } catch (e: any) {
      message.error(e.message || '校验失败')
    }
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', width: 60 },
    {
      title: '平台',
      dataIndex: 'platform',
      width: 90,
      render: (p: string) => PLATFORMS.find((x) => x.value === p)?.label || p,
    },
    { title: '名称', dataIndex: 'display_name' },
    { title: '状态', dataIndex: 'status', width: 90, render: statusTag },
    { title: '目标目录', dataIndex: 'target_dir' },
    {
      title: '容量',
      dataIndex: 'capacity_bytes',
      width: 110,
      render: formatBytes,
    },
    {
      title: '最后校验',
      dataIndex: 'last_checked_at',
      width: 170,
      render: (t: string | null) => (t ? new Date(t).toLocaleString() : '-'),
    },
    {
      title: '操作',
      width: 220,
      render: (_: unknown, r: PanAccount) => (
        <Space>
          <Button
            size="small"
            icon={<CheckCircleOutlined />}
            onClick={() => onCheck(r.id)}
            disabled={r.status === 'disabled'}
          >
            校验
          </Button>
          <Button size="small" icon={<EditOutlined />} onClick={() => onEdit(r)} />
          <Popconfirm
            title="确认删除该账号？凭据将一并移除"
            onConfirm={() => onDelete(r.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <Card
      title="网盘账号管理"
      extra={
        <Space>
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={onAdd}>
            添加账号
          </Button>
        </Space>
      }
    >
      <Table
        rowKey="id"
        columns={columns as any}
        dataSource={data}
        loading={loading}
        pagination={{ pageSize: 20 }}
      />
      <Modal
        title={editing ? '编辑账号' : '添加账号'}
        open={modalOpen}
        onOk={onSubmit}
        onCancel={() => setModalOpen(false)}
        okText="保存"
        cancelText="取消"
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          initialValues={{ platform: 'quark', target_dir: '/tgtool/转存' }}
        >
          <Form.Item name="platform" label="平台" rules={[{ required: true }]}>
            <Select options={PLATFORMS} disabled={!!editing} />
          </Form.Item>
          <Form.Item name="display_name" label="名称" rules={[{ required: true }]}>
            <Input placeholder="如：我的夸克-1" />
          </Form.Item>
          <Form.Item
            name="credential"
            label={editing ? '凭据（留空则不修改）' : '凭据（Cookie/Token）'}
            rules={editing ? [] : [{ required: true }]}
          >
            <Input.TextArea rows={3} placeholder="从浏览器抓取的完整 Cookie" />
          </Form.Item>
          <Form.Item name="target_dir" label="目标目录（平铺）" rules={[{ required: true }]}>
            <Input placeholder="/tgtool/转存" />
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  )
}

export default PanAccounts
