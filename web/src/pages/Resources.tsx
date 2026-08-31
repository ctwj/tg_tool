import React, { useEffect, useState } from 'react'
import { Table, Button, Modal, Form, Input, Select, Space, message, Tag, Popconfirm, Typography, Pagination, Tooltip, Statistic, Row, Col, Card, Switch, InputNumber, Divider, Alert, Spin } from 'antd'
import { ThunderboltOutlined, EditOutlined, DeleteOutlined, ReloadOutlined, BarChartOutlined, SettingOutlined, EyeOutlined, SendOutlined, SafetyCertificateOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { ExtractedResource, ResourceStats, ExtractionResult, ResourceDetailResponse } from '../types'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import { normalizeImageDomain } from '../utils/imageDomain'

const { Text } = Typography

// HTTP 方法颜色（与推送配置页保持一致）
const METHOD_COLORS: Record<string, string> = {
  POST: 'blue',
  PUT: 'orange',
  PATCH: 'purple',
}

// JSON 语法高亮（直接复用自 Push.tsx）
const renderJsonHighlight = (json: string) => {
  return json.replace(/("(?:\\.|[^"\\])*")\s*:/g, '<span style="color:#7dd3fc">$1</span>:')
    .replace(/:\s*("(?:\\.|[^"\\])*")/g, ': <span style="color:#7dd3fc">$1</span>')
    .replace(/:\s*(\d+)/g, ': <span style="color:#fbbf24">$1</span>')
    .replace(/:\s*(true|false|null)/g, ': <span style="color:#c084fc">$1</span>')
}

const DEFAULT_AI_PROMPT = `从消息中提取网盘资源，每个链接一条记录，返回JSON数组。

格式: {"t":"标题","u":"链接","d":"描述","tags":"标签"}

提取规则:
- t(标题): 资源的真实名称。
  - 必须去掉"名称："、"标题："等前缀关键词，只取后面的内容
  - 必须去掉"通过百度网盘分享的文件："等分享模板前缀，只取实际资源名
  - 优先从"名称："、"标题："后面提取；若没有，取消息第一行(去掉emoji和#标签)
  - 绝不要把"资源介绍"、"描述"、"亮点"等关键词或其后面的内容当标题
- u(链接): 完整网盘链接，如果有提取码，拼接到URL的pwd参数中，如 https://pan.baidu.com/s/x?pwd=ab
  - 同一资源的多个网盘链接，每条链接一条记录(相同标题)
  - 没有提取码的链接保持原样
- d(描述): "描述："、"资源介绍："、"简介："、"亮点："等关键词后面的完整段落，无则填""
  - 不要把"来自百度网盘超级会员V4的分享"等系统消息当描述
- tags: 3-5个标签逗号分隔(去#前缀)，无则填""
- 忽略t.me链接、广告群、推广链接、hi.keba.host等非网盘链接
- 网盘链接域名包括: pan.quark.cn, pan.baidu.com, pan.xunlei.com, drive.uc.cn, pan.aliyun.com, 115cdn.com, cloud.189.cn, yun.139.com 等

示例:
消息: "名称：电影A [4K]\\n描述：精彩动作片\\n链接：\\n夸克：https://pan.quark.cn/s/abc\\n百度：https://pan.baidu.com/s/def?pwd=1234"
结果: [{"t":"电影A [4K]","u":"https://pan.quark.cn/s/abc","d":"精彩动作片","tags":"电影,4K"},{"t":"电影A [4K]","u":"https://pan.baidu.com/s/def?pwd=1234","d":"精彩动作片","tags":"电影,4K"}]

消息: "用Gemini生成写真教程\\n📝 资源介绍：详细教程\\n🔗 下载：https://pan.quark.cn/s/xyz"
结果: [{"t":"用Gemini生成写真教程","u":"https://pan.quark.cn/s/xyz","d":"详细教程","tags":"AI,教程"}]

消息: "通过百度网盘分享的文件：大新哥《教你玩转本地生活》\\n链接：https://pan.baidu.com/s/xxx?pwd=ab12\\n提取码：ab12"
结果: [{"t":"大新哥《教你玩转本地生活》","u":"https://pan.baidu.com/s/xxx?pwd=ab12","d":"","tags":"教程,本地生活"}]

消息:
`

const Resources: React.FC = () => {
  const [resources, setResources] = useState<ExtractedResource[]>([])
  const [loading, setLoading] = useState(false)
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined)
  const [categoryFilter, setCategoryFilter] = useState<string | undefined>(undefined)
  const [linkStatusFilter, setLinkStatusFilter] = useState<string | undefined>(undefined)
  const [checkingIds, setCheckingIds] = useState<Set<number>>(new Set())

  // 编辑弹窗
  const [editModalOpen, setEditModalOpen] = useState(false)
  const [editForm] = Form.useForm()
  const [editingId, setEditingId] = useState<number | null>(null)

  // 统计
  const [stats, setStats] = useState<ResourceStats | null>(null)
  const [statsVisible, setStatsVisible] = useState(false)

  // 提取中
  const [extracting, setExtracting] = useState(false)

  // 提取配置
  const [extractConfig, setExtractConfig] = useState({
    extract_mode: 'rule',
    auto_extract: false,
    extract_interval: 30,
    ai_prompt: '',
    ai_use_proxy: false,
  })
  const [nextRunAt, setNextRunAt] = useState<string | null>(null)
  const [extractSaving, setExtractSaving] = useState(false)
  const [configVisible, setConfigVisible] = useState(false)
  const [imageDomain, setImageDomain] = useState('')

  // 查看弹窗（提取对比）
  const [viewModalOpen, setViewModalOpen] = useState(false)
  const [viewDetail, setViewDetail] = useState<ResourceDetailResponse | null>(null)
  const [viewLoading, setViewLoading] = useState(false)

  // 推送
  const [pushingIds, setPushingIds] = useState<Set<number>>(new Set())
  const [pushResultOpen, setPushResultOpen] = useState(false)
  const [pushResultData, setPushResultData] = useState<{
    success: boolean
    request?: {
      method: string
      url: string
      headers: Array<{ key: string; value: string; is_auth?: boolean; location?: string }>
      body: string
    }
    http_status?: number
    response_body?: string
    batch_id?: string
    message?: string
  } | null>(null)

  // 网盘类型选项
  const categoryOptions = [
    { label: '夸克网盘', value: 'quark' },
    { label: '阿里云盘', value: 'aliyun' },
    { label: '百度网盘', value: 'baidu' },
    { label: 'UC网盘', value: 'uc' },
    { label: '115网盘', value: '115' },
    { label: '123网盘', value: '123pan' },
    { label: '天翼网盘', value: 'tianyi' },
    { label: '迅雷网盘', value: 'xunlei' },
  ]

  const categoryLabel = (cat?: string) => {
    const found = categoryOptions.find(o => o.value === cat)
    return found?.label || cat || '未知'
  }

  // 加载资源列表
  const fetchResources = async () => {
    setLoading(true)
    try {
      const params = new URLSearchParams()
      params.set('page', String(page))
      params.set('page_size', String(pageSize))
      if (statusFilter) params.set('status', statusFilter)
      if (categoryFilter) params.set('category', categoryFilter)
      if (linkStatusFilter) params.set('link_status', linkStatusFilter)

      const resp = await apiClient.get(`/resources?${params}`)
      if (resp.data?.success) {
        setResources(resp.data.data?.list || [])
        setTotal(resp.data.data?.pagination?.total || 0)
      }
    } catch {
      message.error('加载资源列表失败')
    } finally {
      setLoading(false)
    }
  }

  // 加载统计
  const fetchStats = async () => {
    try {
      const resp = await apiClient.get('/resources/stats')
      if (resp.data?.success) {
        setStats(resp.data.data)
      }
    } catch {
      // ignore
    }
  }

  useEffect(() => {
    fetchResources()
  }, [page, pageSize, statusFilter, categoryFilter, linkStatusFilter])

  // 加载提取配置
  useEffect(() => {
    const fetchExtractConfig = async () => {
      try {
        const res = await apiClient.get('/options')
        const data = res.data.data ?? {}
        setExtractConfig({
          extract_mode: data.push_extract_mode || 'rule',
          auto_extract: data.push_auto_extract === '1' || data.push_auto_extract === 'true',
          extract_interval: parseInt(data.push_extract_interval || '30', 10),
          ai_prompt: data.push_ai_prompt || '',
          ai_use_proxy: data.push_ai_use_proxy === '1' || data.push_ai_use_proxy === 'true',
        })
        setImageDomain(data.TelegramImageDomain || '')
      } catch { /* ignore */ }
    }
    fetchExtractConfig()
  }, [])

  // 触发提取
  const handleExtract = async () => {
    setExtracting(true)
    try {
      const resp = await apiClient.post('/resources/extract', { batch_size: 1000 }, { timeout: 300000 })
      if (resp.data?.success) {
        const result: ExtractionResult = resp.data.data
        message.success(`提取完成：扫描 ${result.total_scanned} 条，提取 ${result.extracted} 条，跳过 ${result.skipped} 条`)
        fetchResources()
        fetchStats()
      } else {
        message.error(resp.data?.message || '提取失败')
      }
    } catch {
      message.error('提取请求失败')
    } finally {
      setExtracting(false)
    }
  }

  // 保存提取配置
  const saveExtractConfig = async () => {
    setExtractSaving(true)
    try {
      const resp = await apiClient.put('/push/extract-config', {
        extract_mode: extractConfig.extract_mode,
        auto_extract: extractConfig.auto_extract ? '1' : '0',
        extract_interval: String(extractConfig.extract_interval),
        ai_prompt: extractConfig.ai_prompt,
        ai_use_proxy: extractConfig.ai_use_proxy ? '1' : '0',
      })
      setNextRunAt(resp.data?.next_run_at || null)
      message.success('提取配置已保存')
      setConfigVisible(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setExtractSaving(false)
    }
  }

  // 编辑资源
  const handleEdit = (record: ExtractedResource) => {
    setEditingId(record.id)
    editForm.setFieldsValue({
      title: record.title,
      description: record.description,
      tags: record.tags,
      category: record.category,
    })
    setEditModalOpen(true)
  }

  const handleEditSubmit = async () => {
    if (!editingId) return
    try {
      const values = await editForm.validateFields()
      const resp = await apiClient.put(`/resources/${editingId}`, values)
      if (resp.data?.success) {
        message.success('资源已更新')
        setEditModalOpen(false)
        fetchResources()
      }
    } catch {
      message.error('更新失败')
    }
  }

  // 删除资源
  const handleDelete = async (id: number) => {
    try {
      const resp = await apiClient.delete(`/resources/${id}`)
      if (resp.data?.success) {
        message.success('资源已删除')
        fetchResources()
        fetchStats()
      }
    } catch {
      message.error('删除失败')
    }
  }

  // 查看提取对比
  const openViewModal = async (record: ExtractedResource) => {
    setViewModalOpen(true)
    setViewLoading(true)
    setViewDetail(null)
    try {
      const resp = await apiClient.get(`/resources/${record.id}/detail`)
      if (resp.data?.success) {
        setViewDetail(resp.data.data)
      } else {
        message.error(resp.data?.error || '获取详情失败')
      }
    } catch {
      message.error('获取详情失败')
    } finally {
      setViewLoading(false)
    }
  }

  // 推送 — 复用与"推送管理→触发推送"同一套配置，实际推送并标记 is_pushed=true
  const handlePush = async (record: ExtractedResource) => {
    setPushingIds(prev => new Set(prev).add(record.id))
    try {
      const resp = await apiClient.post(`/resources/${record.id}/push`, {}, { timeout: 60000 })
      const data = resp.data?.data
      if (resp.data?.success) {
        message.success(`推送成功 (HTTP ${data?.http_status ?? ''})`)
        setPushResultData({
          success: true,
          request: data?.request,
          http_status: data?.http_status,
          response_body: data?.response_body,
          batch_id: data?.batch_id,
        })
        setPushResultOpen(true)
        // 推送成功后刷新列表，让 is_pushed 状态更新
        fetchResources()
      } else if (data?.missing && Array.isArray(data.missing) && data.missing.length > 0) {
        // 配置缺失
        Modal.warning({
          title: '推送配置不完整',
          content: (
            <div>
              <p>请先在"推送管理"页面配置以下项：</p>
              <ul>{data.missing.map((k: string) => <li key={k}>{k}</li>)}</ul>
            </div>
          ),
        })
      } else if (data?.request || data?.response_body) {
        // 推送发出但 API 报错 — 同样展示左右分栏让用户排查
        setPushResultData({
          success: false,
          request: data?.request,
          http_status: data?.http_status,
          response_body: data?.response_body,
          batch_id: data?.batch_id,
          message: resp.data?.message,
        })
        setPushResultOpen(true)
      } else {
        message.error(resp.data?.message || '推送失败')
      }
    } catch (e: any) {
      message.error(e?.response?.data?.message || e?.message || '推送请求失败')
    } finally {
      setPushingIds(prev => {
        const s = new Set(prev)
        s.delete(record.id)
        return s
      })
    }
  }

  // 检测链接有效性（单条，调用 PanCheck，不改 is_pushed）
  const handleCheck = async (record: ExtractedResource) => {
    setCheckingIds(prev => new Set(prev).add(record.id))
    try {
      const resp = await apiClient.post(`/resources/${record.id}/check-link`, {}, { timeout: 60000 })
      if (resp.data?.success) {
        const ls = resp.data.data?.link_status
        const label = ls === 'valid' ? '有效' : ls === 'invalid' ? '失效' : '未检测'
        message.success(`检测完成：${label}`)
        fetchResources()
      } else {
        message.error(resp.data?.message || '检测失败')
      }
    } catch (e: any) {
      message.error(e?.response?.data?.message || e?.message || '检测请求失败')
    } finally {
      setCheckingIds(prev => { const s = new Set(prev); s.delete(record.id); return s })
    }
  }

  const columns = [
    {
      title: '标题',
      dataIndex: 'title',
      key: 'title',
      width: 250,
      render: (text: string) => (
        <Tooltip title={text}>
          <Text strong className="clamp-2" style={{ color: '#0c4a6e' }}>{text}</Text>
        </Tooltip>
      ),
    },
    {
      title: '封面ID',
      dataIndex: 'img',
      key: 'img',
      width: 180,
      render: (img: string, record: ExtractedResource) => {
        if (!img) return '-'

        const fwdStatus = record.img_forward_status
        const msgId = record.image_message_id
        const fileId = record.file_id

        // 状态色：失败红 / 已转发绿 / 待转发蓝 / 未转发黄
        const color = fwdStatus === 'failed' ? '#ef4444'
          : fwdStatus === 'forwarded' ? '#10b981'
          : fwdStatus === 'pending' ? '#0ea5e9'
          : '#f59e0b'

        const statusText = fwdStatus === 'failed' ? '封面转发失败'
          : fwdStatus === 'forwarded' ? '封面已转发'
          : fwdStatus === 'pending' ? '封面待转发'
          : '封面未转发'

        // 点击 URL：基于 Bot file_id 的图片代理（无 file_id 则不可点击）
        // 配置了图床域名 → {domain}/{fileId}（后端 /api/images/{id} 智能路由识别 file_id）；
        // 未配置 → 走主站相对路径 /api/images/{fileId}
        const domain = normalizeImageDomain(imageDomain)
        const fileUrl = fileId
          ? (domain ? `${domain}/${fileId}` : `/api/images/${fileId}`)
          : null

        const msgTooltip = msgId != null
          ? `群组A 消息ID: ${msgId}`
          : '群组A 消息ID: (历史数据缺失)'

        const fileTooltip = fileId
          ? `Bot file_id（点击打开）:\n${fileId}`
          : 'Bot file_id: 尚未获取'

        // 单行省略号样式
        const ellipsisStyle: React.CSSProperties = {
          fontSize: 12,
          color,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        }

        return (
          <div style={{ lineHeight: '18px', width: '100%' }}>
            {/* msg 行 — 不可点击，单行省略 */}
            <Tooltip title={`${msgTooltip}  ·  ${statusText}`}>
              <div style={ellipsisStyle}>
                <span style={{ color: '#9ca3af', marginRight: 4 }}>msg</span>
                {msgId != null ? msgId : '-'}
              </div>
            </Tooltip>
            {/* file 行 — 可点击，单行省略 */}
            <Tooltip title={fileTooltip}>
              {fileUrl ? (
                <a href={fileUrl} target="_blank" rel="noreferrer"
                   style={{ ...ellipsisStyle, display: 'block', textDecoration: 'none' }}>
                  <span style={{ color: '#9ca3af', marginRight: 4 }}>file</span>
                  {fileId}
                </a>
              ) : (
                <div style={ellipsisStyle}>
                  <span style={{ color: '#9ca3af', marginRight: 4 }}>file</span>
                  -
                </div>
              )}
            </Tooltip>
          </div>
        )
      },
    },
    {
      title: '资源链接',
      dataIndex: 'url',
      key: 'url',
      width: 220,
      render: (url: string) => {
        if (!url) return '-'
        const links = url.split(',').map(s => s.trim()).filter(Boolean)
        return (
          <Space direction="vertical" size={2} style={{ width: '100%' }}>
            {links.map((link, i) => (
              <Tooltip key={i} title={link}>
                <a href={link} target="_blank" rel="noreferrer"
                   style={{
                     fontSize: 12, color: '#0ea5e9',
                     display: 'block', maxWidth: '100%',
                     whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                   }}>
                  {link.replace(/^https?:\/\//, '')}
                </a>
              </Tooltip>
            ))}
          </Space>
        )
      },
    },
    {
      title: '描述',
      dataIndex: 'description',
      key: 'description',
      width: 180,
      render: (text: string) => text ? (
        <Tooltip title={text}>
          <Text className="clamp-2" style={{ fontSize: 12 }}>{text}</Text>
        </Tooltip>
      ) : '-',
    },
    {
      title: '网盘类型',
      dataIndex: 'category',
      key: 'category',
      width: 110,
      render: (cat: string) => (
        <Tag color="#0ea5e9" style={{ margin: 0 }}>{categoryLabel(cat)}</Tag>
      ),
    },
    {
      title: '提取模式',
      dataIndex: 'extract_mode',
      key: 'extract_mode',
      width: 90,
      render: (mode: string) => (
        <Tag color={mode === 'ai' ? '#8b5cf6' : '#10b981'} style={{ margin: 0 }}>{mode === 'ai' ? 'AI' : '规则'}</Tag>
      ),
    },
    {
      title: '标签',
      dataIndex: 'tags',
      key: 'tags',
      width: 150,
      ellipsis: true,
      render: (tags: string) =>
        tags ? tags.split(',').filter(Boolean).map((t, i) => <Tag key={i}>{t}</Tag>) : '-',
    },
    {
      title: '推送状态',
      dataIndex: 'is_pushed',
      key: 'is_pushed',
      width: 100,
      render: (pushed: boolean) => pushed ? <Tag color="green" style={{ margin: 0 }}>已推送</Tag> : <Tag color="orange" style={{ margin: 0 }}>未推送</Tag>,
    },
    {
      title: '链接',
      dataIndex: 'link_status',
      key: 'link_status',
      width: 80,
      render: (ls?: string) => {
        if (ls === 'valid') return <Tooltip title="网盘链接有效"><Tag color="green" style={{ margin: 0 }}>有效</Tag></Tooltip>
        if (ls === 'invalid') return <Tooltip title="网盘链接已失效"><Tag color="red" style={{ margin: 0 }}>失效</Tag></Tooltip>
        return <Tooltip title="未检测/无链接"><Tag color="default" style={{ margin: 0 }}>未检</Tag></Tooltip>
      },
    },
    {
      title: '已编辑',
      dataIndex: 'is_edited',
      key: 'is_edited',
      width: 80,
      render: (edited: boolean) => edited ? <Tag color="cyan" style={{ margin: 0 }}>是</Tag> : '-',
    },
    {
      title: '时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 170,
      render: (t: string) => t?.replace('T', ' ')?.substring(0, 19) || '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 220,
      fixed: 'right' as const,
      render: (_: unknown, record: ExtractedResource) => (
        <Space size={4}>
          <Tooltip title="查看">
            <Button size="small" type="text" icon={<EyeOutlined />} onClick={() => openViewModal(record)} />
          </Tooltip>
          <Tooltip title="编辑">
            <Button size="small" type="text" icon={<EditOutlined />} onClick={() => handleEdit(record)} />
          </Tooltip>
          <Tooltip title="推送">
            <Button
              size="small"
              type="text"
              icon={<SendOutlined />}
              loading={pushingIds.has(record.id)}
              onClick={() => handlePush(record)}
            />
          </Tooltip>
          <Tooltip title="检测链接">
            <Button
              size="small"
              type="text"
              icon={<SafetyCertificateOutlined />}
              loading={checkingIds.has(record.id)}
              onClick={() => handleCheck(record)}
            />
          </Tooltip>
          <Popconfirm title="确定删除此资源？" onConfirm={() => handleDelete(record.id)}>
            <Tooltip title="删除">
              <Button size="small" type="text" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  const { containerRef, scrollY } = useTableScrollY(false) // 外部 Pagination

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title="资源管理"
        description="管理从 Telegram 消息中提取的资源"
        extra={
          <Space>
            <Select
              placeholder="推送状态"
              allowClear
              style={{ width: 120 }}
              value={statusFilter}
              onChange={setStatusFilter}
              options={[
                { label: '未推送', value: 'unpushed' },
                { label: '已推送', value: 'pushed' },
                { label: '全部', value: 'all' },
              ]}
            />
            <Select
              placeholder="网盘类型"
              allowClear
              style={{ width: 130 }}
              value={categoryFilter}
              onChange={setCategoryFilter}
              options={categoryOptions}
            />
            <Select
              placeholder="链接状态"
              allowClear
              style={{ width: 120 }}
              value={linkStatusFilter}
              onChange={setLinkStatusFilter}
              options={[
                { label: '有效', value: 'valid' },
                { label: '失效', value: 'invalid' },
                { label: '未检测', value: 'unknown' },
              ]}
            />
            <Tooltip title="统计">
              <Button icon={<BarChartOutlined />} onClick={() => { setStatsVisible(!statsVisible); fetchStats() }} />
            </Tooltip>
            <Button icon={<SettingOutlined />} onClick={() => setConfigVisible(true)}>提取配置</Button>
            <Button icon={<ReloadOutlined />} onClick={fetchResources}>刷新</Button>
            <Button type="primary" icon={<ThunderboltOutlined />} loading={extracting} onClick={handleExtract}>
              触发提取
            </Button>
          </Space>
        }
      />

      {statsVisible && stats && (
        <Card size="small" style={{ marginBottom: 12, borderRadius: 12, flexShrink: 0 }}>
          <Row gutter={16}>
            <Col span={6}><Statistic title="总资源" value={stats.total} /></Col>
            <Col span={6}><Statistic title="已推送" value={stats.pushed} valueStyle={{ color: '#10b981' }} /></Col>
            <Col span={6}><Statistic title="未推送" value={stats.unpushed} valueStyle={{ color: '#f59e0b' }} /></Col>
            <Col span={6}>
              <div>
                <Text type="secondary">按类型</Text>
                <div>{Object.entries(stats.by_category || {}).map(([k, v]) => (
                  <Tag key={k} style={{ margin: 2 }}>{categoryLabel(k)}: {v}</Tag>
                ))}</div>
              </div>
            </Col>
          </Row>
        </Card>
      )}

      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={resources}
          columns={columns}
          rowKey="id"
          loading={loading}
          pagination={false}
          size="middle"
          scroll={{ x: 1780, y: scrollY }}
          style={{ background: '#fff', borderRadius: 12 }}
          className="resource-table"
        />
      </div>

      <div style={{ flexShrink: 0, padding: '12px 0 0', textAlign: 'right' }}>
        <Pagination
          current={page}
          pageSize={pageSize}
          total={total}
          showTotal={(t) => `共 ${t} 条`}
          showSizeChanger
          onChange={(p, ps) => { setPage(p); setPageSize(ps) }}
        />
      </div>

      <Modal
        title="编辑资源"
        open={editModalOpen}
        onOk={handleEditSubmit}
        onCancel={() => setEditModalOpen(false)}
        okText="保存"
      >
        <Form form={editForm} layout="vertical">
          <Form.Item name="title" label="标题" rules={[{ required: true, message: '标题不能为空' }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="tags" label="标签（逗号分隔）">
            <Input />
          </Form.Item>
          <Form.Item name="category" label="网盘类型">
            <Select options={categoryOptions} allowClear />
          </Form.Item>
        </Form>
      </Modal>

      {/* 提取配置弹窗 */}
      <Modal
        title="提取配置"
        open={configVisible}
        onCancel={() => setConfigVisible(false)}
        footer={null}
        width={560}
      >
        <div style={{ marginTop: 16 }}>
          <div style={{ marginBottom: 20 }}>
            <div style={{ marginBottom: 8, fontWeight: 500 }}>提取模式</div>
            <Space>
              <Select
                value={extractConfig.extract_mode}
                onChange={v => setExtractConfig({ ...extractConfig, extract_mode: v })}
                style={{ width: 200 }}
                options={[
                  { label: '规则提取（推荐）', value: 'rule' },
                  { label: 'AI 增强', value: 'ai' },
                ]}
              />
              {extractConfig.extract_mode === 'ai' && (
                <Tag color="purple">AI 模式已启用</Tag>
              )}
            </Space>
            <div style={{ fontSize: 12, color: '#666', marginTop: 8, lineHeight: '20px' }}>
              <div style={{ marginBottom: 4 }}>
                <Tag color="green" style={{ margin: 0, fontSize: 11 }}>规则提取</Tag>
                速度即时 · 无外部依赖 · 识别 8 种网盘链接 + 广告清洗 + 关键词提取 · 适合格式规范的资源消息
              </div>
              <div>
                <Tag color="purple" style={{ margin: 0, fontSize: 11 }}>AI 增强</Tag>
                调用大模型（约 30s）· 需配置 API 端点 · 在规则基础上语义增强标题/描述/分类 · 适合格式复杂或非标准的消息
              </div>
            </div>
          </div>

          <Divider style={{ margin: '16px 0' }} />

          <div style={{ marginBottom: 20 }}>
            <div style={{ marginBottom: 8, fontWeight: 500 }}>自动提取</div>
            <Space>
              <Switch
                checked={extractConfig.auto_extract}
                onChange={v => setExtractConfig({ ...extractConfig, auto_extract: v })}
              />
              {extractConfig.auto_extract && <Tag color="green">已启用</Tag>}
            </Space>
            <div style={{ fontSize: 12, color: '#999', marginTop: 4 }}>
              启用后将按设定间隔自动触发资源提取
            </div>
          </div>

          {extractConfig.auto_extract && (
            <div style={{ marginBottom: 20 }}>
              <div style={{ marginBottom: 8, fontWeight: 500 }}>提取间隔（分钟）</div>
              <Space>
                <InputNumber
                  min={5}
                  max={1440}
                  value={extractConfig.extract_interval}
                  onChange={v => setExtractConfig({ ...extractConfig, extract_interval: v || 30 })}
                  style={{ width: 200 }}
                />
                {nextRunAt && (
                  <Tag color="blue" style={{ fontSize: 12 }}>
                    下次执行: {nextRunAt}
                  </Tag>
                )}
              </Space>
            </div>
          )}

          {extractConfig.extract_mode === 'ai' && (
            <>
              <Divider style={{ margin: '16px 0' }} />
              <div style={{ marginBottom: 20 }}>
                <div style={{ marginBottom: 8, fontWeight: 500 }}>
                  AI 提示词模板
                  <span style={{ fontWeight: 'normal', color: '#999', marginLeft: 8 }}>
                    （留空使用默认提示词）
                  </span>
                </div>
                <Input.TextArea
                  value={extractConfig.ai_prompt || DEFAULT_AI_PROMPT}
                  onChange={e => setExtractConfig({ ...extractConfig, ai_prompt: e.target.value })}
                  rows={6}
                  placeholder={DEFAULT_AI_PROMPT}
                />
              </div>

              <div style={{ marginBottom: 20 }}>
                <div style={{ marginBottom: 8, fontWeight: 500 }}>通过 HTTP 代理访问 AI 接口</div>
                <Space>
                  <Switch
                    checked={extractConfig.ai_use_proxy}
                    onChange={v => setExtractConfig({ ...extractConfig, ai_use_proxy: v })}
                  />
                  {extractConfig.ai_use_proxy && <Tag color="green">已启用</Tag>}
                </Space>
                <div style={{ fontSize: 12, color: '#999', marginTop: 4 }}>
                  使用系统配置中的代理地址访问大模型 API
                </div>
              </div>
            </>
          )}

          <div style={{ textAlign: 'right' }}>
            <Button
              type="primary"
              onClick={saveExtractConfig}
              loading={extractSaving}
            >
              保存配置
            </Button>
          </div>
        </div>
      </Modal>

      {/* 查看提取对比弹窗 — 左右分栏：原始消息 vs 提取结果 */}
      <Modal
        title={null}
        open={viewModalOpen}
        onCancel={() => setViewModalOpen(false)}
        footer={null}
        width={960}
        destroyOnClose
      >
        {viewLoading ? (
          <div style={{ textAlign: 'center', padding: '60px 0' }}>
            <Spin size="large" tip="加载中..." />
          </div>
        ) : viewDetail ? (
          <div>
            {/* 标题栏 */}
            <div style={{ display: 'flex', alignItems: 'center', marginBottom: 16 }}>
              <div style={{
                width: 36, height: 36, borderRadius: 8,
                background: 'linear-gradient(135deg, #0369a1, #0ea5e9)',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: '#fff', fontSize: 18, marginRight: 12, flexShrink: 0,
              }}>
                <EyeOutlined />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 16, color: '#1f2937' }}>
                  资源查看
                </div>
                <div style={{ fontSize: 12, color: '#9ca3af' }}>
                  {viewDetail.channel_name && (
                    <span style={{ color: '#0ea5e9', marginRight: 6 }}>
                      📡 {viewDetail.channel_name}
                    </span>
                  )}
                  ID: {viewDetail.resource.id} · {viewDetail.resource.created_at?.replace('T', ' ').substring(0, 19)}
                  {viewDetail.resource.extract_mode && (
                    <Tag color={viewDetail.resource.extract_mode === 'ai' ? 'purple' : 'blue'} style={{ marginLeft: 8, marginRight: 0 }}>
                      {viewDetail.resource.extract_mode === 'ai' ? 'AI 提取' : '规则提取'}
                    </Tag>
                  )}
                </div>
              </div>
            </div>

            {/* 左右分栏 */}
            <div style={{ display: 'flex', gap: 16, minHeight: 300 }}>
              {/* 左侧 — 原始消息 */}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 13, color: '#6b7280', marginBottom: 8 }}>
                  📄 原始消息
                </div>
                {!viewDetail.has_history ? (
                  <Alert
                    message="原始消息不可用"
                    description="关联的采集历史记录已被删除"
                    type="warning"
                    showIcon
                    style={{ margin: 0 }}
                  />
                ) : viewDetail.raw_text ? (
                  <div style={{
                    background: '#f9fafb',
                    borderRadius: 8,
                    padding: 16,
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-all',
                    fontSize: 13,
                    lineHeight: 1.8,
                    color: '#374151',
                    maxHeight: 500,
                    overflowY: 'auto',
                    border: '1px solid #e5e7eb',
                  }}>
                    {viewDetail.raw_text}
                  </div>
                ) : (
                  <div style={{
                    background: '#f9fafb',
                    borderRadius: 8,
                    padding: 40,
                    textAlign: 'center',
                    color: '#9ca3af',
                    border: '1px solid #e5e7eb',
                  }}>
                    消息内容为空
                  </div>
                )}
              </div>

              {/* 分隔线 */}
              <div style={{ width: 1, background: '#e5e7eb', flexShrink: 0 }} />

              {/* 右侧 — 提取结果 */}
              <div style={{ width: 400, flexShrink: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 13, color: '#6b7280', marginBottom: 8 }}>
                  ✅ 提取结果
                </div>
                <div style={{ background: '#f9fafb', borderRadius: 8, padding: 16, border: '1px solid #e5e7eb' }}>
                  {/* 标题 */}
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 2 }}>标题</div>
                    <div style={{ fontWeight: 600, color: '#1f2937', fontSize: 14 }}>{viewDetail.resource.title || '-'}</div>
                  </div>

                  {/* 链接 */}
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 2 }}>链接</div>
                    {viewDetail.resource.url ? (
                      <div>
                        {viewDetail.resource.url.split(',').map((u, i) => (
                          <div key={i} style={{ marginBottom: i < viewDetail.resource.url!.split(',').length - 1 ? 4 : 0 }}>
                            <a href={u.trim()} target="_blank" rel="noopener noreferrer" style={{ fontSize: 12, wordBreak: 'break-all' }}>
                              {u.trim()}
                            </a>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <span style={{ color: '#9ca3af', fontSize: 13 }}>无链接</span>
                    )}
                  </div>

                  {/* 描述 */}
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 2 }}>描述</div>
                    <div style={{ color: '#374151', fontSize: 13, whiteSpace: 'pre-wrap' }}>
                      {viewDetail.resource.description || <span style={{ color: '#9ca3af' }}>无</span>}
                    </div>
                  </div>

                  {/* 分类 */}
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 2 }}>网盘类型</div>
                    {viewDetail.resource.category ? (
                      <Tag color="#0ea5e9">{viewDetail.resource.category}</Tag>
                    ) : (
                      <span style={{ color: '#9ca3af', fontSize: 13 }}>无</span>
                    )}
                  </div>

                  {/* 标签 */}
                  {viewDetail.resource.tags && (
                    <div>
                      <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 2 }}>标签</div>
                      <div>
                        {viewDetail.resource.tags.split(',').filter(Boolean).map((tag, i) => (
                          <Tag key={i} style={{ marginBottom: 4 }}>{tag.trim()}</Tag>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        ) : null}
      </Modal>

      {/* ====== 推送结果弹窗 — 左右分栏（请求 / 响应）终端风格 ====== */}
      <Modal
        title="推送结果"
        width={960}
        open={pushResultOpen}
        footer={null}
        onCancel={() => setPushResultOpen(false)}
        destroyOnClose
      >
        {pushResultData ? (
          <div>
            <Row gutter={16}>
              {/* ===== 左侧：Request Preview ===== */}
              <Col span={12}>
                <div style={{
                  background: '#0f172a',
                  borderRadius: 8,
                  overflow: 'hidden',
                  border: '1px solid #334155',
                }}>
                  <div style={{
                    display: 'flex', alignItems: 'center', gap: 6,
                    padding: '10px 16px',
                    background: '#1e293b',
                    borderBottom: '1px solid #334155',
                  }}>
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#ef4444' }} />
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#fbbf24' }} />
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#0ea5e9' }} />
                    <Text style={{ color: '#94a3b8', fontSize: 12, marginLeft: 8, fontFamily: 'monospace' }}>
                      Request Preview
                    </Text>
                  </div>

                  <div style={{ padding: 16 }}>
                    {/* 请求行 */}
                    <div style={{
                      display: 'flex', alignItems: 'center', gap: 8,
                      marginBottom: 16, padding: '8px 12px',
                      background: '#1e293b', borderRadius: 6,
                    }}>
                      <Tag color={METHOD_COLORS[pushResultData.request?.method || 'POST'] || 'blue'} style={{
                        fontWeight: 700, fontSize: 12, minWidth: 52, textAlign: 'center',
                        margin: 0, borderRadius: 4,
                      }}>
                        {pushResultData.request?.method || 'POST'}
                      </Tag>
                      <span style={{
                        color: '#e2e8f0', fontSize: 12, fontFamily: 'monospace',
                        wordBreak: 'break-all', lineHeight: '18px',
                      }}>
                        {pushResultData.request?.url || '(未配置 API 地址)'}
                      </span>
                    </div>

                    {/* 请求头 */}
                    {pushResultData.request?.headers && pushResultData.request.headers.length > 0 ? (
                      <div style={{ marginBottom: 16 }}>
                        <div style={{ color: '#94a3b8', fontSize: 11, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                          Request Headers
                        </div>
                        <div style={{
                          background: '#1e293b', borderRadius: 6, padding: '8px 12px',
                          borderLeft: '2px solid #3b82f6',
                        }}>
                          {pushResultData.request.headers.map((h, i) => (
                            <div key={i} style={{
                              fontSize: 12, fontFamily: "'SFMono-Regular', Consolas, monospace",
                              lineHeight: '22px', display: 'flex', gap: 4,
                            }}>
                              <span style={{ color: h.is_auth ? '#fbbf24' : '#7dd3fc', flexShrink: 0 }}>
                                {h.key}:
                              </span>
                              <span style={{
                                color: h.is_auth ? '#fde68a' : '#7dd3fc',
                                wordBreak: 'break-all',
                              }}>
                                {h.value}
                              </span>
                              {h.is_auth && (
                                <Tag color="gold" style={{ fontSize: 9, lineHeight: '16px', margin: '0 0 0 4px', padding: '0 4px' }}>
                                  AUTH
                                </Tag>
                              )}
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null}

                    {/* 请求体 */}
                    <div>
                      <div style={{ color: '#94a3b8', fontSize: 11, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                        Request Body
                      </div>
                      {pushResultData.request?.body ? (
                        <div style={{
                          background: '#1e293b', borderRadius: 6, padding: '10px 12px',
                          borderLeft: '2px solid #a855f7',
                          maxHeight: 280, overflow: 'auto',
                        }}>
                          <pre
                            style={{
                              fontSize: 11.5, lineHeight: '17px', margin: 0,
                              fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
                              color: '#e2e8f0', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                            }}
                            dangerouslySetInnerHTML={{
                              __html: renderJsonHighlight(pushResultData.request.body),
                            }}
                          />
                        </div>
                      ) : (
                        <div style={{ background: '#1e293b', borderRadius: 6, padding: 12, color: '#64748b', fontSize: 12, fontStyle: 'italic' }}>
                          无请求体
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              </Col>

              {/* ===== 右侧：Response Preview ===== */}
              <Col span={12}>
                <div style={{
                  background: '#0f172a',
                  borderRadius: 8,
                  overflow: 'hidden',
                  border: '1px solid #334155',
                }}>
                  <div style={{
                    display: 'flex', alignItems: 'center', gap: 6,
                    padding: '10px 16px',
                    background: '#1e293b',
                    borderBottom: '1px solid #334155',
                  }}>
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#ef4444' }} />
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#fbbf24' }} />
                    <span style={{ width: 8, height: 8, borderRadius: '50%', background: '#0ea5e9' }} />
                    <Text style={{ color: '#94a3b8', fontSize: 12, marginLeft: 8, fontFamily: 'monospace' }}>
                      Response Preview
                    </Text>
                  </div>

                  <div style={{ padding: 16 }}>
                    {/* 状态行 */}
                    <div style={{
                      display: 'flex', alignItems: 'center', gap: 8,
                      marginBottom: 16, padding: '8px 12px',
                      background: '#1e293b', borderRadius: 6,
                    }}>
                      <Tag color={pushResultData.success ? 'green' : 'red'} style={{
                        fontWeight: 700, fontSize: 12, minWidth: 52, textAlign: 'center',
                        margin: 0, borderRadius: 4,
                      }}>
                        {pushResultData.http_status ?? '---'}
                      </Tag>
                      <span style={{
                        color: pushResultData.success ? '#7dd3fc' : '#fca5a5',
                        fontSize: 12, fontFamily: 'monospace',
                      }}>
                        {pushResultData.success ? 'OK' : 'Error'}
                      </span>
                    </div>

                    {/* 响应体 */}
                    <div>
                      <div style={{ color: '#94a3b8', fontSize: 11, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                        Response Body
                      </div>
                      {pushResultData.response_body ? (
                        <div style={{
                          background: '#1e293b', borderRadius: 6, padding: '10px 12px',
                          borderLeft: `2px solid ${pushResultData.success ? '#0ea5e9' : '#ef4444'}`,
                          maxHeight: 320, overflow: 'auto',
                        }}>
                          <pre
                            style={{
                              fontSize: 11.5, lineHeight: '17px', margin: 0,
                              fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
                              color: '#e2e8f0', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                            }}
                            dangerouslySetInnerHTML={{
                              __html: renderJsonHighlight(pushResultData.response_body),
                            }}
                          />
                        </div>
                      ) : (
                        <div style={{ background: '#1e293b', borderRadius: 6, padding: 12, color: '#64748b', fontSize: 12, fontStyle: 'italic' }}>
                          无响应内容
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              </Col>
            </Row>

            {pushResultData.batch_id ? (
              <div style={{ marginTop: 12, color: '#64748b', fontSize: 12, fontFamily: 'monospace' }}>
                Batch ID: {pushResultData.batch_id}
              </div>
            ) : null}
          </div>
        ) : null}
      </Modal>
    </div>
  )
}

export default Resources
