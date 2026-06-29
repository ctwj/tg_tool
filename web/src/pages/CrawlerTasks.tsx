import React, { useEffect, useState, useMemo } from 'react'
import {
  Table, Button, Modal, Form, Input, InputNumber, Select, Switch, Space,
  message, Tag, Popconfirm, Drawer, Tooltip, Typography, Alert, Empty,
  Card, Collapse, Progress,
} from 'antd'
import {
  PlusOutlined, EditOutlined, DeleteOutlined, PlayCircleOutlined,
  ExportOutlined, ImportOutlined, SaveOutlined, ReloadOutlined,
  LinkOutlined,
  SettingOutlined, GlobalOutlined, ClockCircleOutlined,
  ControlOutlined, BarChartOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import * as crawlerApi from '../api/crawler'
import type { CrawlerTask, CrawlerTaskInput, CrawlerTemplate, FieldStat } from '../types'

const { Text } = Typography

// 统一 Card 样式：让分组有明显的视觉边界
const CARD_STYLE: React.CSSProperties = {
  marginBottom: 16,
  borderColor: '#e5e7eb',
}
const CARD_HEAD_STYLE: React.CSSProperties = {
  backgroundColor: '#f9fafb',
  borderBottomColor: '#e5e7eb',
  fontWeight: 600,
}
const CARD_BODY_STYLE: React.CSSProperties = { paddingTop: 18 }

const DEFAULT_USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'

function emptyInput(): CrawlerTaskInput {
  return {
    name: '',
    enabled: true,
    list_urls: [''],
    two_stage: true,
    interval_minutes: 30,
    task_concurrency: 1,
    user_agent: DEFAULT_USER_AGENT,
    request_delay_ms: 1000,
    proxy: '',
    auto_link_check: false,
    block_detection_config: '',
    max_consecutive_failures: 3,
    template_source: '',
    max_pagination_depth: 10,
  }
}

const STATUS_META: Record<string, { color: string; text: string }> = {
  active: { color: 'success', text: '运行中' },
  paused: { color: 'default', text: '已暂停' },
  auto_blocked: { color: 'error', text: '已自动停用' },
  deleted: { color: 'default', text: '已删除' },
}

// ---------- 主组件 ----------
const CrawlerTasks: React.FC = () => {
  const navigate = useNavigate()
  const [tasks, setTasks] = useState<CrawlerTask[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(false)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [keyword, setKeyword] = useState('')
  const [statusFilter, setStatusFilter] = useState<string>('')

  const [editorOpen, setEditorOpen] = useState(false)
  const [editing, setEditing] = useState<CrawlerTask | null>(null)
  const [form] = Form.useForm<CrawlerTaskInput>()

  const [templates, setTemplates] = useState<CrawlerTemplate[]>([])
  const [templatePickerOpen, setTemplatePickerOpen] = useState(false)

  const [submitLoading, setSubmitLoading] = useState(false)

  // 字段命中率统计 Modal（FR-027 / T059）
  const [statsOpen, setStatsOpen] = useState(false)
  const [statsTask, setStatsTask] = useState<CrawlerTask | null>(null)
  const [statsRows, setStatsRows] = useState<FieldStat[]>([])
  const [statsDays, setStatsDays] = useState(30)
  const [statsLoading, setStatsLoading] = useState(false)

  const { containerRef: tableContainerRef, scrollY: tableScrollY } = useTableScrollY()

  const fetchTasks = async () => {
    setLoading(true)
    try {
      const res = await crawlerApi.listTasks({
        page, page_size: pageSize,
        keyword: keyword || undefined,
        status: statusFilter || undefined,
      })
      setTasks(res.data?.list ?? [])
      setTotal(res.data?.pagination?.total ?? 0)
    } catch (e: any) {
      message.error('获取任务列表失败: ' + (e.message ?? ''))
    } finally {
      setLoading(false)
    }
  }

  const fetchTemplates = async () => {
    try {
      const res = await crawlerApi.listTemplates()
      setTemplates(res.data ?? [])
    } catch { /* 静默 */ }
  }

  useEffect(() => { fetchTasks() }, [page, pageSize, statusFilter])
  useEffect(() => { fetchTemplates() }, [])

  // ---------- 编辑器 ----------
  const openCreate = () => {
    setEditing(null)
    form.resetFields()
    const init = emptyInput()
    form.setFieldsValue(init as any)
    setEditorOpen(true)
  }

  const openEdit = (task: CrawlerTask) => {
    setEditing(task)
    // API 返回的 list_urls 是 JSON 字符串（DB 原样），塞进 form 前需解析
    let parsedListUrls: string[] = ['']
    const rawListUrls = (task as any).list_urls
    if (typeof rawListUrls === 'string') {
      try {
        const arr = JSON.parse(rawListUrls)
        if (Array.isArray(arr) && arr.length > 0) parsedListUrls = arr.filter((s: any) => typeof s === 'string')
      } catch { /* 用默认 */ }
    } else if (Array.isArray(rawListUrls) && rawListUrls.length > 0) {
      parsedListUrls = rawListUrls
    }
    form.setFieldsValue({
      ...task,
      list_urls: parsedListUrls,
      proxy: task.proxy ?? '',
      user_agent: task.user_agent ?? DEFAULT_USER_AGENT,
      block_detection_config: task.block_detection_config ?? '',
      template_source: task.template_source ?? '',
      max_pagination_depth: task.max_pagination_depth ?? 10,
    } as any)
    setEditorOpen(true)
  }

  // onSubmitSuccess: 创建/更新成功后的回调（用于"保存并进入字段配置器"场景跳转）
  const handleSubmit = async (onSuccess?: (task: CrawlerTask) => void) => {
    let values: CrawlerTaskInput
    try {
      const raw = await form.validateFields()
      // 清洗 list_urls（去空白 + 过滤空）
      values = {
        ...raw,
        list_urls: (raw.list_urls || []).map((s: string) => (s || '').trim()).filter(Boolean),
        proxy: raw.proxy?.trim() || null,
        user_agent: raw.user_agent?.trim() || null,
        block_detection_config: raw.block_detection_config?.trim() || null,
        template_source: raw.template_source?.trim() || null,
        max_pagination_depth: raw.max_pagination_depth ?? 10,
      }
      if (values.list_urls.length === 0) {
        message.warning('请至少填写一个列表页 URL')
        return
      }
    } catch {
      return
    }
    setSubmitLoading(true)
    try {
      let saved: CrawlerTask
      if (editing) {
        const res = await crawlerApi.updateTask(editing.id, values)
        if (!res.success || !res.data) throw new Error(res.message ?? '更新失败')
        saved = res.data
        message.success('任务已更新')
      } else {
        const res = await crawlerApi.createTask(values)
        if (!res.success || !res.data) throw new Error(res.message ?? '创建失败')
        saved = res.data
        message.success('任务已创建')
      }
      setEditorOpen(false)
      fetchTasks()
      if (onSuccess) onSuccess(saved)
    } catch (e: any) {
      message.error('保存失败: ' + (e.message ?? ''))
    } finally {
      setSubmitLoading(false)
    }
  }

  /** 跳转到字段配置器（带 taskId 与首个 list_url） */
  const goToConfigurator = (task: CrawlerTask) => {
    const firstUrl = parseListUrls((task as any).list_urls)[0] ?? ''
    const qs = new URLSearchParams({
      taskId: String(task.id),
      ...(firstUrl ? { listUrl: firstUrl } : {}),
    })
    navigate(`/crawler/configurator?${qs.toString()}`)
  }

  // ---------- 操作 ----------
  const handleToggle = async (task: CrawlerTask) => {
    try {
      await crawlerApi.toggleTask(task.id, !task.enabled)
      message.success(task.enabled ? '已停用' : '已启用')
      fetchTasks()
    } catch (e: any) { message.error('切换失败: ' + (e.message ?? '')) }
  }

  const handleDelete = async (task: CrawlerTask, cascade: boolean) => {
    try {
      await crawlerApi.deleteTask(task.id, cascade)
      message.success(cascade ? '任务及文章已删除' : '任务已删除（文章保留）')
      fetchTasks()
    } catch (e: any) { message.error('删除失败: ' + (e.message ?? '')) }
  }

  const handleRun = async (task: CrawlerTask) => {
    try {
      await crawlerApi.runTask(task.id)
      message.success(`任务「${task.name}」已触发，请稍后查看历史记录`)
    } catch (e: any) { message.error('触发失败: ' + (e.message ?? '')) }
  }

  const handleExport = async (task: CrawlerTask) => {
    try {
      const blob = await crawlerApi.exportTask(task.id)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `crawler-task-${task.name}-${task.id}.json`
      document.body.appendChild(a)
      a.click()
      a.remove()
      URL.revokeObjectURL(url)
    } catch (e: any) { message.error('导出失败: ' + (e.message ?? '')) }
  }

  const handleImport = () => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = 'application/json'
    input.onchange = async () => {
      const f = input.files?.[0]
      if (!f) return
      try {
        const text = await f.text()
        const data = JSON.parse(text) as CrawlerTaskInput
        // 直接走 import 接口
        await crawlerApi.importTask(data)
        message.success('任务已导入')
        fetchTasks()
      } catch (e: any) {
        message.error('导入失败: ' + (e.message ?? '文件格式不正确'))
      }
    }
    input.click()
  }

  const applyTemplate = (tpl: CrawlerTemplate) => {
    setEditing(null)
    const cfg = tpl.config
    form.setFieldsValue({
      ...cfg,
      list_urls: cfg.list_urls.length > 0 ? cfg.list_urls : [''],
      proxy: cfg.proxy ?? '',
      user_agent: cfg.user_agent ?? DEFAULT_USER_AGENT,
      block_detection_config: cfg.block_detection_config ?? '',
      template_source: cfg.template_source ?? tpl.key,
      max_pagination_depth: cfg.max_pagination_depth ?? 10,
    } as any)
    setTemplatePickerOpen(false)
    setEditorOpen(true)
    message.info(`已应用模板「${tpl.name}」，请调整 list_urls 与字段配置`)
  }

  // ---------- 字段命中率统计（FR-027） ----------
  const openStats = async (task: CrawlerTask) => {
    setStatsTask(task)
    setStatsOpen(true)
    setStatsDays(30)
    await fetchStats(task.id, 30)
  }

  const fetchStats = async (taskId: number, days: number) => {
    setStatsLoading(true)
    try {
      const res = await crawlerApi.getTaskFieldStats(taskId, days)
      setStatsRows(res.data?.stats ?? [])
    } catch (e: any) {
      message.error('获取字段命中率失败: ' + (e.message ?? ''))
      setStatsRows([])
    } finally {
      setStatsLoading(false)
    }
  }

  const onStatsDaysChange = async (days: number | null) => {
    if (!statsTask || !days) return
    setStatsDays(days)
    await fetchStats(statsTask.id, days)
  }

  // ---------- 表格列 ----------
  const columns = useMemo(() => [
    {
      title: 'ID', dataIndex: 'id', width: 70,
    },
    {
      title: '任务名', dataIndex: 'name', ellipsis: true,
      render: (v: string, r: CrawlerTask) => (
        <Space direction="vertical" size={0}>
          <Text strong>{v}</Text>
          {r.template_source && <Text type="secondary" style={{ fontSize: 12 }}>模板: {r.template_source}</Text>}
        </Space>
      ),
    },
    {
      title: '状态', dataIndex: 'status', width: 120,
      render: (v: string) => {
        const m = STATUS_META[v] ?? { color: 'default', text: v }
        return <Tag color={m.color}>{m.text}</Tag>
      },
    },
    {
      title: '启用', dataIndex: 'enabled', width: 70,
      render: (v: boolean, r: CrawlerTask) => (
        <Switch checked={v} size="small" onChange={() => handleToggle(r)} />
      ),
    },
    {
      title: '间隔(分)', dataIndex: 'interval_minutes', width: 90, align: 'center' as const,
    },
    {
      title: '上次运行', dataIndex: 'last_run_at', width: 160,
      render: (v: string | null) => v ? dayjs(v).format('MM-DD HH:mm:ss') : <Text type="secondary">—</Text>,
    },
    {
      title: '下次运行', dataIndex: 'next_run_at', width: 160,
      render: (v: string | null) => v ? dayjs(v).format('MM-DD HH:mm:ss') : <Text type="secondary">—</Text>,
    },
    {
      title: '连续失败', dataIndex: 'consecutive_failures', width: 90, align: 'center' as const,
      render: (v: number, r: CrawlerTask) => v > 0
        ? <Tag color={v >= r.max_consecutive_failures ? 'error' : 'warning'}>{v}</Tag>
        : <Text type="secondary">0</Text>,
    },
    {
      title: '操作', key: 'actions', width: 340, fixed: 'right' as const,
      render: (_: any, r: CrawlerTask) => (
        <Space size={4} wrap>
          <Button
            type="primary"
            ghost
            size="small"
            icon={<ControlOutlined />}
            onClick={() => goToConfigurator(r)}
          >
            字段配置
          </Button>
          <Tooltip title="编辑任务元信息">
            <Button type="text" size="small" icon={<EditOutlined />} onClick={() => openEdit(r)} />
          </Tooltip>
          <Tooltip title="立即运行">
            <Button type="text" size="small" icon={<PlayCircleOutlined />} onClick={() => handleRun(r)} />
          </Tooltip>
          <Tooltip title="字段命中率">
            <Button type="text" size="small" icon={<BarChartOutlined />} onClick={() => openStats(r)} />
          </Tooltip>
          <Tooltip title="导出 JSON 配置">
            <Button type="text" size="small" icon={<ExportOutlined />} onClick={() => handleExport(r)} />
          </Tooltip>
          <Popconfirm
            title="删除任务"
            description={
              <Space direction="vertical" size={2}>
                <span>是否同时删除已采集的文章？</span>
              </Space>
            }
            onConfirm={() => handleDelete(r, false)}
            okText="保留文章"
            cancelText="取消"
          >
            <Button type="text" size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ], [])

  // ---------- 渲染 ----------
  return (
    <div style={{ padding: 24 }}>
      <PageHeader title="爬虫任务" description="配置驱动的多站点采集，独立于 Telegram 采集系统" />

      <Space style={{ marginBottom: 16 }} wrap>
        <Input.Search
          placeholder="搜索任务名/模板"
          allowClear
          style={{ width: 240 }}
          onSearch={(v) => { setKeyword(v); setPage(1) }}
        />
        <Select
          placeholder="状态筛选"
          allowClear
          style={{ width: 150 }}
          value={statusFilter || undefined}
          onChange={(v) => { setStatusFilter(v || ''); setPage(1) }}
          options={[
            { value: 'active', label: '运行中' },
            { value: 'paused', label: '已暂停' },
            { value: 'auto_blocked', label: '已自动停用' },
          ]}
        />
        <Button icon={<ReloadOutlined />} onClick={fetchTasks}>刷新</Button>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>新建任务</Button>
        <Button icon={<SaveOutlined />} onClick={() => setTemplatePickerOpen(true)}>从模板创建</Button>
        <Button icon={<ImportOutlined />} onClick={handleImport}>导入配置</Button>
      </Space>

      <div ref={tableContainerRef} style={{ flex: 1, minHeight: 300 }}>
      <Table
        rowKey="id"
        loading={loading}
        dataSource={tasks}
        columns={columns as any}
        scroll={{ x: 1200, y: tableScrollY }}
        size="middle"
        pagination={{
          current: page, pageSize, total,
          showSizeChanger: true, showTotal: (t) => `共 ${t} 条`,
          onChange: (p, ps) => { setPage(p); setPageSize(ps) },
        }}
      />
      </div>

      {/* 编辑抽屉 */}
      <Drawer
        title={editing ? `编辑任务 #${editing.id}` : '新建任务'}
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        width={920}
        extra={(
          <Space size={8}>
            <Button onClick={() => setEditorOpen(false)}>取消</Button>
            <Button
              type="primary"
              loading={submitLoading}
              onClick={() => handleSubmit()}
            >
              保存
            </Button>
          </Space>
        )}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          initialValues={emptyInput() as any}
        >
          {/* Section 1: 基本信息 */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span><SettingOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />① 基本信息</span>}
          >
            <Space align="start" style={{ display: 'flex' }}>
              <Form.Item
                label="任务名"
                name="name"
                rules={[{ required: true, message: '请输入任务名' }]}
                style={{ flex: 1, marginRight: 16, minWidth: 280 }}
                tooltip="任务名会作为文章的 source_type 写入数据库，是后续推送接入的关键标识。建议用英文/拼音，避免空格"
                extra="英文或拼音，如：discuz-res、wp-blog-a"
              >
                <Input placeholder="如：example-resource-site" />
              </Form.Item>
              <Form.Item
                label="启用"
                name="enabled"
                valuePropName="checked"
                tooltip="关闭后调度器不会再拉起此任务，但保留所有已采集文章"
              >
                <Switch />
              </Form.Item>
            </Space>
          </Card>

          {/* Section 2: 列表页 URL */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span><LinkOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />② 列表页 URL</span>}
          >
            <Alert
              type="info" showIcon
              style={{ marginBottom: 12 }}
              message="要抓取的入口页面，可填多个（如分页 page=1,2,3）"
              description={(
                <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12, color: '#6b7280', lineHeight: 1.7 }}>
                  <li>每个 URL 都会被解析为多条详情链接</li>
                  <li>无需分页：把每一页的完整 URL 都列出来即可</li>
                  <li><b>需要自动翻页</b>：只需填第一页 URL，再到字段配置器新增 <code>pagination</code> 字段配分页规则（见下方 ②.b）</li>
                </ul>
              )}
            />
            <Form.Item label="URL 列表" required>
              <Form.List name="list_urls">
                {(fields, { add, remove }) => (
                  <>
                    {fields.map((f) => (
                      <Space key={f.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                        <Form.Item name={f.name} noStyle rules={[{ required: true, message: 'URL 不能为空' }]}>
                          <Input
                            placeholder="https://example.com/list?page=1"
                            style={{ width: 720 }}
                            prefix={<LinkOutlined style={{ color: '#bfbfbf' }} />}
                          />
                        </Form.Item>
                        <Button type="text" danger size="small" icon={<DeleteOutlined />} onClick={() => remove(f.name)} />
                      </Space>
                    ))}
                    <Button type="dashed" icon={<PlusOutlined />} onClick={() => add('')}>添加 URL</Button>
                  </>
                )}
              </Form.List>
            </Form.Item>
          </Card>

          {/* Section 2.b: 自动翻页（可选） */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span>②.b 自动翻页（可选）</span>}
          >
            <Alert
              type="info" showIcon
              style={{ marginBottom: 12 }}
              message="分页规则在「字段配置器」里配置，不在这里填"
              description={(
                <div style={{ fontSize: 12, lineHeight: 1.7, color: '#6b7280' }}>
                  自动翻页是一项需要验证的<b>数据</b>，应在抓取列表页源码后再配置：<br />
                  ① 任务保存后进入「字段配置器」 → 输入列表 URL → 点继续抓源码<br />
                  ② 在右侧字段树「列表页字段」下新增字段，name 选 <code>pagination</code>（自动联动为分页指针类型）<br />
                  ③ 填 CSS 选择器（如 <code>.pagination a</code>、<code>.pg a</code>）匹配分页链接，点「验证」确认命中<br />
                  ④ 验证无误后保存字段树，引擎即按此规则链式翻页（命中值作为下一页 URL，去重后扩散抓取）
                </div>
              )}
            />
            <Form.Item
              label="翻页深度上限"
              name="max_pagination_depth"
              tooltip="字段配置器中 field_type=pagination 字段触发链式翻页时的最大页数（含种子页）。0=不限，默认 10。测试期建议填 3-5 防失控"
              extra="页（0=不限，默认 10；为安全阀，防止抓取失控）"
            >
              <InputNumber min={0} max={10000} style={{ width: 200 }} />
            </Form.Item>
          </Card>

          {/* Section 3: 字段配置（提示 + 入口） */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span>③ 字段配置</span>}
          >
            <Alert
              type="info" showIcon
              style={{ marginBottom: 12 }}
              message="可视化字段配置器：4 tab 源码预览 + 字段树（list/detail 双作用域）+ 20+ 预置字段库 + 6 种匹配模式"
              description={(
                <div style={{ fontSize: 12, lineHeight: 1.7, color: '#6b7280' }}>
                  043 重构：原 042 内联 CSS 选择器表单已下线。任务保存后，进入「字段配置」可视化编辑器，
                  支持 4 tab 源码预览（header/html/script/meta）+ 字段树（list/detail 双作用域，父子嵌套链接卡片）+
                  20+ 预置字段库 + 6 种匹配模式（CSS / 正则 / 前后缀 / JSON Path / meta 属性 / 响应头）。
                </div>
              )}
            />
            {editing ? (
              <Button
                type="primary"
                icon={<ControlOutlined />}
                onClick={() => goToConfigurator(editing)}
              >
                进入字段配置器
              </Button>
            ) : (
              <Button
                type="primary"
                ghost
                icon={<ControlOutlined />}
                onClick={() => handleSubmit((saved) => goToConfigurator(saved))}
                loading={submitLoading}
              >
                保存并进入字段配置器
              </Button>
            )}
          </Card>

          {/* Section 4: 调度与并发 */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span><ClockCircleOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />④ 调度与并发</span>}
          >
            <Space wrap size="middle">
              <Form.Item
                label="抓取间隔"
                name="interval_minutes"
                rules={[{ required: true }]}
                tooltip="每隔多少分钟自动拉起此任务一次。短间隔会加重目标站点压力，建议 ≥ 15 分钟"
                extra="分钟"
              >
                <InputNumber min={1} max={1440} style={{ width: 150 }} />
              </Form.Item>
              <Form.Item
                label="任务内并发"
                name="task_concurrency"
                rules={[{ required: true }]}
                tooltip="单个任务内同时抓取多个详情页的并发数。任务级上限受全局 crawler_global_concurrency 约束"
                extra="详情页并发"
              >
                <InputNumber min={1} max={10} style={{ width: 150 }} />
              </Form.Item>
              <Form.Item
                label="请求间隔"
                name="request_delay_ms"
                tooltip="每次 HTTP 请求之间的等待毫秒数，避免压垮目标站点"
                extra="毫秒"
              >
                <InputNumber min={0} max={60000} style={{ width: 150 }} />
              </Form.Item>
              <Form.Item
                label="最大连续失败"
                name="max_consecutive_failures"
                tooltip="连续失败多少次后自动停用任务（status=auto_blocked），需要手动恢复"
                extra="次后自动停用"
              >
                <InputNumber min={1} max={100} style={{ width: 150 }} />
              </Form.Item>
            </Space>
          </Card>

          {/* Section 5: 网络 */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={<span><GlobalOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />⑤ 网络与识别</span>}
          >
            <Form.Item
              label="User-Agent"
              name="user_agent"
              tooltip="HTTP 请求头 User-Agent。留空使用默认（Chrome 桌面版）。部分站点会对 UA 做指纹识别"
            >
              <Input.TextArea rows={2} placeholder={DEFAULT_USER_AGENT} />
            </Form.Item>
            <Form.Item
              label="代理"
              name="proxy"
              tooltip="仅对当前任务生效的代理 URL，覆盖系统 http_proxy_url。支持 http/https/socks5"
              extra="格式：http://host:port 或 socks5://host:port；留空走系统代理"
            >
              <Input placeholder="http://127.0.0.1:7890 或留空" />
            </Form.Item>
            <Form.Item
              label="自动链接检测"
              name="auto_link_check"
              valuePropName="checked"
              tooltip={(
                <div>
                  抓取完成后自动调用 PanCheck 服务校验网盘链接有效性<br />
                  需要 pancheck_host 已配置（系统设置），否则全部置 unknown
                </div>
              )}
            >
              <Switch />
            </Form.Item>
          </Card>

          {/* Section 6: 高级（折叠） */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
          >
            <Collapse ghost items={[{
              key: 'advanced',
              label: <Space><ControlOutlined style={{ color: '#0ea5e9' }} /> 高级（拦截检测覆盖 / 模板标记）</Space>,
              children: (
                <>
                  <Alert
                    type="info" showIcon
                    style={{ marginBottom: 12 }}
                    message="以下选项仅用于高级调优，普通场景保持默认即可"
                  />
                  <Form.Item
                    label="拦截检测覆盖（JSON）"
                    name="block_detection_config"
                    tooltip="覆盖默认的反爬检测阈值。JSON 格式。例如：{&quot;empty_threshold_chars&quot;: 200}"
                  >
                    <Input.TextArea
                      rows={3}
                      placeholder='留空走默认；示例：{"empty_threshold_chars": 200, "block_keywords": ["登录", "验证码"]}'
                    />
                  </Form.Item>
                  <Form.Item
                    label="模板来源"
                    name="template_source"
                    tooltip="此任务是从哪个模板创建的（仅用于标记，不影响行为）"
                  >
                    <Input placeholder="generic_resource_site 或自定义" disabled={!!editing} />
                  </Form.Item>
                </>
              ),
            }]} />
          </Card>
        </Form>
      </Drawer>

      {/* 字段命中率统计 Modal（FR-027） */}
      <Modal
        title={statsTask ? `字段命中率 — ${statsTask.name}` : '字段命中率'}
        open={statsOpen}
        onCancel={() => setStatsOpen(false)}
        footer={null}
        width={860}
        destroyOnClose
      >
        <Space style={{ marginBottom: 12, width: '100%', justifyContent: 'space-between' }} wrap>
          <Space>
            <Text type="secondary">统计窗口：</Text>
            <Select<number>
              value={statsDays}
              onChange={onStatsDaysChange}
              size="small"
              style={{ width: 110 }}
              options={[
                { value: 7, label: '近 7 天' },
                { value: 14, label: '近 14 天' },
                { value: 30, label: '近 30 天' },
                { value: 90, label: '近 90 天' },
              ]}
            />
          </Space>
          <Tooltip title="命中率 < 10% 的字段会被标红，提示规则可能过期">
            <Tag color="warning" style={{ margin: 0 }}>规则可能过期 &lt; 10%</Tag>
          </Tooltip>
        </Space>
        <Table
          rowKey={(r) => `${r.field_node_id ?? 'null'}-${r.field_path}`}
          loading={statsLoading}
          dataSource={statsRows}
          size="small"
          pagination={false}
          scroll={{ y: 480 }}
          locale={{ emptyText: <Empty description="暂无字段抓取记录（任务可能未运行过）" /> }}
          columns={[
            {
              title: '字段', dataIndex: 'field_path', ellipsis: true,
              render: (path: string, r: FieldStat) => (
                <Space direction="vertical" size={0} style={{ width: '100%' }}>
                  <Text strong style={{ fontSize: 13 }}>
                    {r.field_display_name ?? r.field_name ?? path.split('/').pop() ?? path}
                  </Text>
                  <Text type="secondary" style={{ fontSize: 11, fontFamily: 'monospace' }}>{path}</Text>
                </Space>
              ),
            },
            {
              title: '命中 / 总数', width: 120, align: 'center' as const,
              render: (_: any, r: FieldStat) => (
                <Text>
                  <Text strong>{r.hit_articles}</Text>
                  <Text type="secondary"> / {r.total_articles}</Text>
                </Text>
              ),
            },
            {
              title: '命中率', width: 180,
              render: (_: any, r: FieldStat) => {
                const pct = Math.round(r.hit_rate * 100)
                const color = r.status === 'healthy' ? '#10b981'
                  : r.status === 'degraded' ? '#f59e0b' : '#ef4444'
                return (
                  <Progress
                    percent={pct}
                    size="small"
                    strokeColor={color}
                    format={(p) => `${p}%`}
                  />
                )
              },
            },
            {
              title: '状态', width: 130,
              render: (_: any, r: FieldStat) => {
                if (r.status === 'healthy') return <Tag color="success">健康</Tag>
                if (r.status === 'degraded') return <Tag color="warning">退化</Tag>
                return (
                  <Tooltip title="命中率 &lt; 10%，建议复核 CSS 选择器 / 来源层是否变更">
                    <Tag color="error" style={{ cursor: 'help' }}>规则可能过期</Tag>
                  </Tooltip>
                )
              },
            },
          ]}
        />
      </Modal>

      {/* 模板选择 */}
      <Modal
        title="从模板创建"
        open={templatePickerOpen}
        onCancel={() => setTemplatePickerOpen(false)}
        footer={null}
        width={640}
      >
        {templates.length === 0 ? (
          <Empty description="暂无可用模板（043 字段树模板将在 US1 T037 重建）" />
        ) : (
          (() => {
            const builtin = templates.filter(t => !t.key.startsWith('custom_'))
            const custom = templates.filter(t => t.key.startsWith('custom_'))
            const renderGroup = (title: string, list: CrawlerTemplate[]) => (
              list.length === 0 ? null : (
                <div style={{ marginBottom: 16 }}>
                  <div style={{
                    fontSize: 12, color: '#6b7280', marginBottom: 8,
                    borderBottom: '1px solid #f0f0f0', paddingBottom: 4,
                  }}>
                    {title} <Tag style={{ marginInlineStart: 4 }}>{list.length}</Tag>
                  </div>
                  <Space direction="vertical" style={{ width: '100%' }} size={8}>
                    {list.map((tpl) => (
                      <div
                        key={tpl.key}
                        style={{
                          border: '1px solid #e5e7eb', borderRadius: 8, padding: 12,
                          cursor: 'pointer', transition: 'all .2s',
                        }}
                        onClick={() => applyTemplate(tpl)}
                        onMouseEnter={(e) => (e.currentTarget.style.borderColor = '#0ea5e9')}
                        onMouseLeave={(e) => (e.currentTarget.style.borderColor = '#e5e7eb')}
                      >
                        <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                          <Space direction="vertical" size={0}>
                            <Text strong>{tpl.name}</Text>
                            <Space size={4}>
                              <Tag color={tpl.site_type === 'forum' ? 'purple' : tpl.site_type === 'blog' ? 'blue' : 'default'}>
                                {tpl.site_type}
                              </Tag>
                              <Text type="secondary" style={{ fontSize: 11 }}>{tpl.key}</Text>
                            </Space>
                          </Space>
                          <Button type="primary" size="small">使用</Button>
                        </Space>
                        {tpl.description && (
                          <Typography.Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0, fontSize: 12 }}>
                            {tpl.description}
                          </Typography.Paragraph>
                        )}
                      </div>
                    ))}
                  </Space>
                </div>
              )
            )
            return (
              <>
                {renderGroup('内置模板', builtin)}
                {renderGroup('自定义模板', custom)}
              </>
            )
          })()
        )}
      </Modal>
    </div>
  )
}

/** 从任务的 list_urls 字段（可能是 JSON 字符串、字符串数组或换行分隔字符串）解析出 URL 列表 */
function parseListUrls(raw: unknown): string[] {
  if (Array.isArray(raw)) return raw.filter((s): s is string => typeof s === 'string')
  if (typeof raw === 'string') {
    try {
      const v = JSON.parse(raw)
      if (Array.isArray(v)) return v.filter((s): s is string => typeof s === 'string')
    } catch {
      return raw.split('\n').map((s) => s.trim()).filter(Boolean)
    }
  }
  return []
}

export default CrawlerTasks
