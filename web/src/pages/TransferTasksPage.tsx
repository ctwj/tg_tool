import React, { useEffect, useState } from 'react'
import {
  Table,
  Button,
  Tag,
  Space,
  Select,
  message,
  Card,
  Typography,
  Modal,
  Input,
} from 'antd'
import { ReloadOutlined, ShareAltOutlined } from '@ant-design/icons'
import {
  listTransferTasks,
  retryTransfer,
  getTransferTask,
  type TransferTask,
} from '../api/pan'

const statusTag = (s: string) => {
  const map: Record<string, { color: string; text: string }> = {
    pending: { color: 'default', text: '待处理' },
    processing: { color: 'processing', text: '进行中' },
    succeeded: { color: 'success', text: '成功' },
    failed: { color: 'error', text: '失败' },
  }
  const v = map[s] || { color: 'default', text: s }
  return <Tag color={v.color}>{v.text}</Tag>
}

const TransferTasksPage: React.FC = () => {
  const [data, setData] = useState<TransferTask[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [status, setStatus] = useState<string | undefined>(undefined)
  const [loading, setLoading] = useState(false)
  const [share, setShare] = useState<{ url: string; code: string | null | undefined } | null>(null)

  const load = async () => {
    setLoading(true)
    try {
      const res = await listTransferTasks({ status, page, page_size: 20 })
      setData(res.items)
      setTotal(res.total)
    } catch (e: any) {
      message.error(e.message || '加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page, status])

  const onRetry = async (id: number) => {
    try {
      await retryTransfer(id)
      message.success('已重新入队')
      load()
    } catch (e: any) {
      message.error(e.message || '重试失败')
    }
  }

  const onViewShare = async (id: number) => {
    try {
      const t = await getTransferTask(id)
      if (t.share_url) {
        setShare({ url: t.share_url, code: t.share_extract_code })
      } else {
        message.info('该任务无分享链接')
      }
    } catch (e: any) {
      message.error(e.message || '查询失败')
    }
  }

  const copyShare = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => message.success('已复制'),
      () => message.error('复制失败，请手动选择'),
    )
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', width: 60 },
    {
      title: '来源链接',
      dataIndex: 'source_url',
      ellipsis: true,
      render: (u: string) => (
        <Typography.Text ellipsis style={{ maxWidth: 220 }} title={u}>
          {u}
        </Typography.Text>
      ),
    },
    {
      title: '类型',
      dataIndex: 'source_type',
      width: 90,
      render: (t: string) => (t === 'pan_share' ? '网盘分享' : '直链'),
    },
    { title: '状态', dataIndex: 'status', width: 90, render: statusTag },
    {
      title: '失败原因',
      dataIndex: 'failure_reason',
      ellipsis: true,
      render: (r: string | null) =>
        r ? (
          <Typography.Text type="danger" ellipsis style={{ maxWidth: 200 }} title={r}>
            {r}
          </Typography.Text>
        ) : (
          '-'
        ),
    },
    { title: '重试', dataIndex: 'retry_count', width: 60 },
    {
      title: '创建时间',
      dataIndex: 'created_at',
      width: 160,
      render: (t: string) => new Date(t).toLocaleString(),
    },
    {
      title: '操作',
      width: 200,
      render: (_: unknown, r: TransferTask) => (
        <Space>
          {r.status === 'failed' && (
            <Button size="small" onClick={() => onRetry(r.id)}>
              重试
            </Button>
          )}
          {r.status === 'succeeded' && (
            <Button size="small" icon={<ShareAltOutlined />} onClick={() => onViewShare(r.id)}>
              分享
            </Button>
          )}
        </Space>
      ),
    },
  ]

  return (
    <Card
      title="转存任务历史"
      extra={
        <Space>
          <Select
            allowClear
            placeholder="状态筛选"
            style={{ width: 120 }}
            value={status}
            onChange={(v) => {
              setStatus(v)
              setPage(1)
            }}
            options={[
              { value: 'pending', label: '待处理' },
              { value: 'processing', label: '进行中' },
              { value: 'succeeded', label: '成功' },
              { value: 'failed', label: '失败' },
            ]}
          />
          <Button icon={<ReloadOutlined />} onClick={load}>
            刷新
          </Button>
        </Space>
      }
    >
      <Table
        rowKey="id"
        columns={columns as any}
        dataSource={data}
        loading={loading}
        pagination={{ current: page, total, pageSize: 20, onChange: setPage }}
      />
      <Modal
        title="分享链接"
        open={!!share}
        onCancel={() => setShare(null)}
        footer={[
          <Button key="copy" type="primary" onClick={() => share && copyShare(share.url)}>
            复制链接
          </Button>,
          <Button key="close" onClick={() => setShare(null)}>
            关闭
          </Button>,
        ]}
      >
        {share && (
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input value={share.url} readOnly />
            {share.code && (
              <Input
                value={`提取码：${share.code}`}
                readOnly
                suffix={
                  <Button size="small" onClick={() => copyShare(share.code!)}>
                    复制
                  </Button>
                }
              />
            )}
          </Space>
        )}
      </Modal>
    </Card>
  )
}

export default TransferTasksPage
