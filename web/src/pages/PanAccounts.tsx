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
  Tooltip,
  Descriptions,
  Alert,
  Typography,
  Empty,
} from 'antd'
import {
  PlusOutlined,
  ReloadOutlined,
  DeleteOutlined,
  CheckCircleOutlined,
  EditOutlined,
  MedicineBoxOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons'
import {
  listPanAccounts,
  createPanAccount,
  updatePanAccount,
  deletePanAccount,
  checkPanAccount,
  diagnosePanAccount,
  type PanAccount,
  type CreatePanAccount,
  type AccountDiagnosis,
  type DiagnoseFileItem,
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

const formatBytes = (b: number | null | undefined) => {
  if (!b && b !== 0) return '-'
  const gb = b / 1024 / 1024 / 1024
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${(b / 1024 / 1024).toFixed(0)} MB`
}

/** 渲染容量列：已用 / 总；附使用率提示（无总容量则仅显示已用） */
const renderCapacity = (used: number | null, total: number | null) => {
  if (!total && !used) return '-'
  if (!total) return formatBytes(used)
  const usedStr = formatBytes(used)
  const totalStr = formatBytes(total)
  const pct = total > 0 && used != null ? Math.min(100, (used / total) * 100) : null
  const label = `${usedStr} / ${totalStr}`
  if (pct == null) return label
  return <Tooltip title={`使用率 ${pct.toFixed(1)}%`}>{label}</Tooltip>
}

const PanAccounts: React.FC = () => {
  const [data, setData] = useState<PanAccount[]>([])
  const [loading, setLoading] = useState(false)
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<PanAccount | null>(null)
  const [form] = Form.useForm()
  const [diagOpen, setDiagOpen] = useState(false)
  const [diagLoading, setDiagLoading] = useState(false)
  const [diag, setDiag] = useState<AccountDiagnosis | null>(null)
  const [helpOpen, setHelpOpen] = useState(false)

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

  const onDiagnose = async (r: PanAccount) => {
    setDiagOpen(true)
    setDiagLoading(true)
    setDiag(null)
    try {
      const d = await diagnosePanAccount(r.id)
      setDiag(d)
    } catch (e: any) {
      message.error(e.message || '诊断失败')
      setDiagOpen(false)
    } finally {
      setDiagLoading(false)
    }
  }

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
      title: '容量（已用 / 总）',
      width: 160,
      render: (_: unknown, r: PanAccount) =>
        renderCapacity(r.used_capacity_bytes, r.capacity_bytes),
    },
    {
      title: '最后校验',
      dataIndex: 'last_checked_at',
      width: 170,
      render: (t: string | null) => (t ? new Date(t).toLocaleString() : '-'),
    },
    {
      title: '操作',
      width: 280,
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
          <Button
            size="small"
            icon={<MedicineBoxOutlined />}
            onClick={() => onDiagnose(r)}
            disabled={r.status === 'disabled'}
          >
            诊断
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
          <Button icon={<QuestionCircleOutlined />} onClick={() => setHelpOpen(true)}>
            使用说明
          </Button>
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

      {/* 综合诊断弹窗 */}
      <Modal
        title="账号综合诊断"
        open={diagOpen}
        onCancel={() => setDiagOpen(false)}
        footer={<Button onClick={() => setDiagOpen(false)}>关闭</Button>}
        width={720}
        destroyOnClose
      >
        {diagLoading ? (
          <div style={{ textAlign: 'center', padding: '40px 0' }}>正在诊断中…</div>
        ) : diag ? (
          <DiagnosisContent diag={diag} />
        ) : (
          <Empty description="无诊断数据" />
        )}
      </Modal>

      {/* 使用说明弹窗 */}
      <Modal
        title="使用说明 — 网盘账号能力与触发方式"
        open={helpOpen}
        onCancel={() => setHelpOpen(false)}
        footer={<Button onClick={() => setHelpOpen(false)}>知道了</Button>}
        width={780}
        destroyOnClose
      >
        <HelpContent />
      </Modal>
    </Card>
  )
}

/** 诊断结果展示：基本信息 + 根目录样本 + 能力清单 */
const DiagnosisContent: React.FC<{ diag: AccountDiagnosis }> = ({ diag }) => {
  return (
    <div>
      <Alert
        showIcon
        type={diag.valid ? 'success' : 'error'}
        message={diag.valid ? 'Cookie 有效，可正常使用' : 'Cookie 失效或异常'}
        description={diag.message || undefined}
        style={{ marginBottom: 12 }}
      />
      <Descriptions
        size="small"
        column={2}
        bordered
        style={{ marginBottom: 12 }}
        items={[
          {
            key: 'platform',
            label: '平台',
            children: PLATFORMS.find((x) => x.value === diag.platform)?.label || diag.platform,
          },
          {
            key: 'total',
            label: '总容量',
            children: diag.capacity_bytes ? formatBytes(diag.capacity_bytes) : '-',
          },
          {
            key: 'used',
            label: '已用容量',
            children: diag.used_capacity_bytes ? formatBytes(diag.used_capacity_bytes) : '-',
          },
          {
            key: 'usage',
            label: '使用率',
            children:
              diag.capacity_bytes && diag.used_capacity_bytes != null ? (
                <Tooltip
                  title={`${formatBytes(diag.used_capacity_bytes)} / ${formatBytes(diag.capacity_bytes)}`}
                >
                  {((diag.used_capacity_bytes / diag.capacity_bytes) * 100).toFixed(1)}%
                </Tooltip>
              ) : (
                '-'
              ),
          },
        ]}
      />

      <Typography.Text strong>根目录文件样本（最多 10 项）</Typography.Text>
      {!diag.root_files_ok ? (
        <Alert
          type="warning"
          showIcon
          message={diag.root_files_error || '列文件失败'}
          style={{ marginTop: 6 }}
        />
      ) : diag.root_files_sample.length === 0 ? (
        <Empty description="根目录为空" style={{ margin: '12px 0' }} />
      ) : (
        <Table
          size="small"
          rowKey="fid"
          pagination={false}
          style={{ marginTop: 6 }}
          dataSource={diag.root_files_sample}
          columns={[
            {
              title: '名称',
              dataIndex: 'file_name',
              render: (n: string, r: DiagnoseFileItem) =>
                r.is_dir ? `📁 ${n}/` : `📄 ${n}`,
            },
            { title: '类型', dataIndex: 'is_dir', width: 80, render: (d: boolean) => (d ? '目录' : '文件') },
            {
              title: '大小',
              dataIndex: 'size',
              width: 110,
              render: (s: number, r: DiagnoseFileItem) => (r.is_dir ? '-' : formatBytes(s)),
            },
          ]}
        />
      )}
      {diag.root_files_ok && (
        <Typography.Text type="secondary" style={{ display: 'block', marginTop: 6 }}>
          根目录共 {diag.root_files_total} 项
        </Typography.Text>
      )}

      <Typography.Text strong style={{ display: 'block', marginTop: 16 }}>
        已实现能力（{diag.capabilities.length}）
      </Typography.Text>
      <Space wrap style={{ marginTop: 4 }}>
        {diag.capabilities.length === 0 ? (
          <Typography.Text type="secondary">该平台暂无已实现能力</Typography.Text>
        ) : (
          diag.capabilities.map((c) => (
            <Tag color="green" key={c}>
              {c}
            </Tag>
          ))
        )}
      </Space>

      {diag.unsupported.length > 0 && (
        <>
          <Typography.Text strong style={{ display: 'block', marginTop: 16 }}>
            受限 / 未实现（{diag.unsupported.length}）
          </Typography.Text>
          <div style={{ marginTop: 4 }}>
            {diag.unsupported.map((u) => (
              <Alert
                key={u.capability}
                type="warning"
                showIcon
                style={{ marginBottom: 6 }}
                message={
                  <span>
                    <Typography.Text code>{u.capability}</Typography.Text>{' '}
                    {u.reason}
                  </span>
                }
              />
            ))}
          </div>
        </>
      )}
    </div>
  )
}

/** 使用说明：各能力用途 + 触发方式 + 平台支持 */
const HelpContent: React.FC = () => {
  const capabilities: Array<{
    name: string
    desc: string
    trigger: string
    quark: 'yes' | 'no' | 'partial'
  }> = [
    {
      name: 'health_check',
      desc: '校验 Cookie 是否有效，并读取账号总容量/已用容量。点击列表「校验」按钮即触发，回写状态字段。',
      trigger: '校验按钮 / 创建账号时自动',
      quark: 'yes',
    },
    {
      name: 'transfer_share',
      desc: '将夸克分享链接中的文件转存到自己网盘的目标目录。这是「转存任务」的核心路径。',
      trigger: '创建转存任务（source_url 为夸克分享链接）',
      quark: 'yes',
    },
    {
      name: 'create_share',
      desc: '为转存/上传后的文件生成新的分享链接（含提取码），写入 share_records。',
      trigger: '转存/上传任务成功后自动调用',
      quark: 'yes',
    },
    {
      name: 'upload_file',
      desc: '将直链文件下载到本地中转目录，再分片上传到夸克网盘（含 OSS 签权 + 秒传）。',
      trigger: '创建转存任务（source_url 为直链 http(s)://）',
      quark: 'yes',
    },
    {
      name: 'list_files',
      desc: '列出指定目录的文件清单。用于「诊断」弹窗展示根目录样本，验证 Cookie 是否真的可用。',
      trigger: '诊断按钮（根目录样本）',
      quark: 'yes',
    },
    {
      name: 'check_share_validity',
      desc: '预检分享链接是否有效（含提取码错误/失效检测），不进行实际转存。可用于任务提交前的预校验。',
      trigger: '当前仅后端 API，前端暂无入口',
      quark: 'yes',
    },
    {
      name: 'check_instant_transfer',
      desc: '秒传检测：通过文件 md5+sha1 判断夸克是否已存在相同文件。命中则跳过上传步骤。',
      trigger: 'upload_file 内部自动调用（前端不可见）',
      quark: 'yes',
    },
    {
      name: 'offline_download',
      desc: '磁力链/ed2k 离线下载。夸克网页版已下线原生支持，社区逆向端点需动态签名易失效。',
      trigger: '未实现。如需，建议走 aria2 委托模式（OpenList 路线）。',
      quark: 'no',
    },
  ]

  const tagOf = (s: 'yes' | 'no' | 'partial') =>
    s === 'yes' ? (
      <Tag color="green">已实现</Tag>
    ) : s === 'partial' ? (
      <Tag color="orange">部分</Tag>
    ) : (
      <Tag color="default">未实现</Tag>
    )

  return (
    <div>
      <Typography.Paragraph type="secondary">
        本页用于管理网盘账号的凭据（AES-256-GCM 加密存储）。账号创建后，下表列出的能力会在不同场景被自动调用。
        点击「校验」做轻量 Cookie 检测；点击「诊断」可一次性验证全部已实现能力（含根目录文件样本）。
      </Typography.Paragraph>
      <Table
        size="small"
        rowKey="name"
        pagination={false}
        dataSource={capabilities}
        columns={[
          {
            title: '能力',
            dataIndex: 'name',
            width: 180,
            render: (n: string) => <Typography.Text code>{n}</Typography.Text>,
          },
          {
            title: '夸克',
            dataIndex: 'quark',
            width: 90,
            render: tagOf,
          },
          { title: '说明', dataIndex: 'desc' },
          {
            title: '触发方式',
            dataIndex: 'trigger',
            width: 240,
            render: (t: string) => <Typography.Text type="secondary">{t}</Typography.Text>,
          },
        ]}
      />
      <Alert
        type="info"
        showIcon
        style={{ marginTop: 12 }}
        message="UC / 百度 驱动尚未实现"
        description="UC 网盘 API 与夸克同源（drive-pc.uc.cn），未来可快速接入；百度需适配 OAuth + bdstoken 双重校验。当前这两类账号创建后会标记 disabled，不会被任何任务调用。"
      />
    </div>
  )
}

export default PanAccounts
