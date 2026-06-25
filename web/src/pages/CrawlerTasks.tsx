import React, { useEffect, useState, useMemo } from 'react'
import {
  Table, Button, Modal, Form, Input, InputNumber, Select, Switch, Space,
  message, Tag, Popconfirm, Drawer, Tooltip, Typography, Alert, Descriptions, Empty, Spin,
  Card, Collapse,
} from 'antd'
import {
  PlusOutlined, EditOutlined, DeleteOutlined, PlayCircleOutlined,
  ThunderboltOutlined, ExportOutlined, ImportOutlined, SaveOutlined, ReloadOutlined,
  BookOutlined, BulbOutlined, LinkOutlined,
  SettingOutlined, GlobalOutlined, ClockCircleOutlined, CodeOutlined, ControlOutlined,
  FastForwardOutlined, RobotOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import * as crawlerApi from '../api/crawler'
import type {
  CrawlerTask, CrawlerTaskInput, CrawlerTemplate, CrawlerTestPreview, FieldSelectors,
} from '../types'

const { TextArea } = Input
const { Text, Paragraph } = Typography

// 统一 Card 样式：让 6 个分组有明显的视觉边界
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

// ---------- 默认值与工具 ----------
const DEFAULT_SELECTORS: FieldSelectors = {
  list_item: '',
  detail_link: '',
  detail_link_attr: 'href',
  title: { css: '', attr: null, regex: null },
  content: { css: '', attr: 'html', regex: null },
  category: { css: '', attr: null, regex: null },
  tags: { css: '', attr: null, regex: null },
  images: { css: '', attr: 'src', regex: null },
  pan_links: { css: '', attr: 'href', regex: null },
  direct_links: { css: '', attr: 'href', regex: null },
}

const DEFAULT_USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'

// AI 辅助提取提示词：用户复制到 Claude / ChatGPT，让其自动 fetch 站点 HTML 产出 crawler config（两阶段模式专用）
const AI_SELECTOR_PROMPT = `你是 CSS 选择器专家。我会给你一个目标站点的列表页 URL（域名或分页地址），请按【两阶段抓取】模型，自行抓取该 URL 与一条样本详情页，分析后返回 crawler task 配置所需的全部 CSS 选择器。

两阶段抓取流程：列表页按 list_item 选择器切成多条记录 → 每条按 detail_link 取详情页 URL → 抓详情页后按字段选择器提取 title/content/images/pan_links 等。

严格只输出以下 JSON（不要 markdown 代码块、不要解释、不要前后缀文字）：
{
  "list_item": "【列表页】每条文章卡片/行的最外层容器选择器，如 .post-list .post-item",
  "detail_link": "【列表页内、list_item 范围内】详情页链接的选择器，通常是 a 标签，如 a.detail-title",
  "detail_link_attr": "取链接用的属性名，默认 href；若链接放在 data-url 等自定义属性则填该属性名",
  "title":          { "css": "【详情页】文章标题选择器" },
  "content":        { "css": "【详情页】正文最外层容器选择器（含段落+图片+链接的整体，不要只取单个 <p>）" },
  "category":       { "css": "【详情页】分类选择器；找不到填 null" },
  "tags":           { "css": "【详情页】标签选择器；找不到填 null" },
  "images":         { "css": "【详情页正文内】图片选择器，如 .content img；找不到填 null" },
  "pan_links":      { "css": "【详情页正文内】网盘链接选择器，如 .content a[href*=\\"pan.baidu\\"]；找不到填 null" },
  "direct_links":   { "css": "【详情页正文内】直链选择器（非网盘的下载链接，如 magnet:/ed2k:/直链）；找不到填 null" },
  "pagination_selector": "【列表页】分页选择器，一次匹配页面所有数字页/上一页/下一页/末页的 <a>，如 .pagination a / .pg a / .nav-links a；未启用翻页填空字符串",
  "max_pages": 0
}

判定规则：
1. list_item 与 detail_link 是必填项；找不到请明确返回 null 并说明原因
2. 网盘链接识别这些域名：pan.baidu.com / www.aliyundrive.com / aliyun.com / pan.quark.cn / 123pan.com / 115.com / cloud.189.cn / pc.qq.com（uc）
3. 选择器优先级：稳定的 class > 结构路径 > 标签；避免用 nth-child / inline-style 等易碎选择器
4. 正文 content 必须取文章主体最外层容器，能一次性覆盖段落+图片+网盘链接，便于下游字段（images/pan_links/direct_links）在其内部查找
5. pagination_selector 引擎会自动去重扩散抓取，所以只需给一个能匹配所有分页 a 标签的选择器即可

执行要求：
1. 你已具备联网/fetch 能力（如 WebFetch / web_reader）：请直接抓取我提供的列表页 URL 取 HTML；若该页有详情链接，请再抓一条详情页样本用于字段校验
2. 若你的运行环境无法联网，请明确告诉用户「请把列表页 view-source 内容贴给我」并暂停
3. 拿到 HTML 后基于实际 DOM 给出最稳定的选择器组合
4. 若站点需要 JS 渲染才能看到正文（HTML 里没有文章节点），请明确告知用户「该站需 JS 渲染，CSS 选择器无法命中」，让用户考虑换站

目标站点列表页 URL：<在此粘贴列表页 URL，如 https://example.com/list 或 https://example.com/forumdisplay.php?fid=1>`

function emptyInput(): CrawlerTaskInput {
  return {
    name: '',
    enabled: true,
    list_urls: [''],
    selectors: { ...DEFAULT_SELECTORS },
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
    pagination_selector: '',
    max_pages: 0,
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
  // T054: "保存为模板"弹窗
  const [saveTplOpen, setSaveTplOpen] = useState(false)
  const [saveTplLoading, setSaveTplLoading] = useState(false)
  const [saveTplForm] = Form.useForm<{ name: string; description?: string }>()

  const [testOpen, setTestOpen] = useState(false)
  const [testLoading, setTestLoading] = useState(false)
  const [testPreview, setTestPreview] = useState<CrawlerTestPreview | null>(null)
  const [testTaskId, setTestTaskId] = useState<number | null>(null)

  // UX: 保存按钮 loading + CSS 选择器速查弹窗
  const [submitLoading, setSubmitLoading] = useState(false)
  const [cssHelpOpen, setCssHelpOpen] = useState(false)
  // AI 结果解析弹窗
  const [aiResultOpen, setAiResultOpen] = useState(false)
  const [aiResultText, setAiResultText] = useState('')


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
    // 把任务字段塞回 form
    form.setFieldsValue({
      ...task,
      proxy: task.proxy ?? '',
      user_agent: task.user_agent ?? DEFAULT_USER_AGENT,
      block_detection_config: task.block_detection_config ?? '',
      template_source: task.template_source ?? '',
      pagination_selector: task.pagination_selector ?? '',
      max_pages: task.max_pages ?? 0,
      list_urls: task.list_urls.length > 0 ? task.list_urls : [''],
    } as any)
    setEditorOpen(true)
  }

  const handleSubmit = async () => {
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
        pagination_selector: raw.pagination_selector?.trim() || null,
        max_pages: raw.max_pages ?? 0,
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
      if (editing) {
        await crawlerApi.updateTask(editing.id, values)
        message.success('任务已更新')
      } else {
        await crawlerApi.createTask(values)
        message.success('任务已创建')
      }
      setEditorOpen(false)
      fetchTasks()
    } catch (e: any) {
      message.error('保存失败: ' + (e.message ?? ''))
    } finally {
      setSubmitLoading(false)
    }
  }

  // AI 结果解析：把 AI 返回的 JSON 字符串解析后填入表单
  // 容忍 markdown ```json 围栏 / 前后多余文字
  const handleParseAiResult = () => {
    const raw = (aiResultText || '').trim()
    if (!raw) {
      message.warning('请先粘贴 AI 返回的 JSON')
      return
    }
    // 抽取最外层 { ... } 块，去掉 ```json 围栏 / 解释性前后缀
    let jsonStr = raw
    const fenceMatch = raw.match(/```(?:json)?\s*([\s\S]*?)```/i)
    if (fenceMatch) {
      jsonStr = fenceMatch[1].trim()
    } else {
      const firstBrace = raw.indexOf('{')
      const lastBrace = raw.lastIndexOf('}')
      if (firstBrace >= 0 && lastBrace > firstBrace) {
        jsonStr = raw.slice(firstBrace, lastBrace + 1)
      }
    }
    let parsed: Record<string, any>
    try {
      parsed = JSON.parse(jsonStr)
    } catch (e: any) {
      message.error('JSON 解析失败：' + (e.message ?? '未知错误') + '。请确认粘贴的是完整 JSON')
      return
    }
    // 兼容嵌套 { css: "..." } / 裸字符串 / null
    const pickCss = (v: any): string => {
      if (v == null) return ''
      if (typeof v === 'string') return v
      if (typeof v === 'object' && typeof v.css === 'string') return v.css
      return ''
    }
    const nextSelectors: FieldSelectors = {
      list_item: typeof parsed.list_item === 'string' ? parsed.list_item : '',
      detail_link: typeof parsed.detail_link === 'string' ? parsed.detail_link : '',
      detail_link_attr: typeof parsed.detail_link_attr === 'string' && parsed.detail_link_attr
        ? parsed.detail_link_attr : 'href',
      title:     { css: pickCss(parsed.title),     attr: null, regex: null },
      content:   { css: pickCss(parsed.content),   attr: pickCss(parsed.content) ? 'html' : null, regex: null },
      category:  { css: pickCss(parsed.category),  attr: null, regex: null },
      tags:      { css: pickCss(parsed.tags),      attr: null, regex: null },
      images:    { css: pickCss(parsed.images),    attr: 'src', regex: null },
      pan_links: { css: pickCss(parsed.pan_links), attr: 'href', regex: null },
      direct_links: { css: pickCss(parsed.direct_links), attr: 'href', regex: null },
    }
    form.setFieldsValue({
      selectors: nextSelectors,
      pagination_selector: typeof parsed.pagination_selector === 'string'
        ? parsed.pagination_selector : '',
      max_pages: typeof parsed.max_pages === 'number' ? parsed.max_pages : 0,
    } as any)
    const hitFields = [
      nextSelectors.list_item && 'list_item',
      nextSelectors.detail_link && 'detail_link',
      nextSelectors.title.css && 'title',
      nextSelectors.content.css && 'content',
      nextSelectors.pan_links.css && 'pan_links',
      parsed.pagination_selector && 'pagination',
    ].filter(Boolean)
    message.success(`已填入 ${hitFields.length} 个字段：${hitFields.join(' / ') || '（未识别到有效字段）'}`)
    setAiResultOpen(false)
    setAiResultText('')
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

  const handleTest = async (task: CrawlerTask) => {
    setTestTaskId(task.id)
    setTestOpen(true)
    setTestPreview(null)
    setTestLoading(true)
    try {
      const res = await crawlerApi.testTask(task.id, 3)
      setTestPreview(res.data ?? null)
    } catch (e: any) {
      message.error('测试失败: ' + (e.message ?? ''))
    } finally { setTestLoading(false) }
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
      pagination_selector: cfg.pagination_selector ?? '',
      max_pages: cfg.max_pages ?? 0,
    } as any)
    setTemplatePickerOpen(false)
    setEditorOpen(true)
    message.info(`已应用模板「${tpl.name}」，请调整 list_urls 与选择器`)
  }

  // T054: 把当前编辑中的任务另存为自定义模板
  const handleSaveAsTemplate = async () => {
    if (!editing) {
      message.warning('请先保存任务再另存为模板')
      return
    }
    try {
      const v = await saveTplForm.validateFields()
      setSaveTplLoading(true)
      await crawlerApi.saveAsTemplate(editing.id, v.name, v.description)
      message.success(`模板「${v.name}」已保存`)
      setSaveTplOpen(false)
      saveTplForm.resetFields()
      fetchTemplates() // 刷新模板列表
    } catch (e: any) {
      if (e?.errorFields) return
      message.error('保存模板失败: ' + (e?.message ?? ''))
    } finally {
      setSaveTplLoading(false)
    }
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
      title: '操作', key: 'actions', width: 280, fixed: 'right' as const,
      render: (_: any, r: CrawlerTask) => (
        <Space size={4} wrap>
          <Tooltip title="编辑">
            <Button type="text" size="small" icon={<EditOutlined />} onClick={() => openEdit(r)} />
          </Tooltip>
          <Tooltip title="测试运行（不落库）">
            <Button type="text" size="small" icon={<ThunderboltOutlined />} onClick={() => handleTest(r)} />
          </Tooltip>
          <Tooltip title="立即运行">
            <Button type="text" size="small" icon={<PlayCircleOutlined />} onClick={() => handleRun(r)} />
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
          onSearch={(v) => { setKeyword(v); setPage(1); }}
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
        scroll={{ x: 1100, y: tableScrollY }}
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
            {editing ? (
              <Tooltip title="将当前任务配置存为可复用的自定义模板">
                <Button icon={<SaveOutlined />} onClick={() => setSaveTplOpen(true)}>
                  另存为模板
                </Button>
              </Tooltip>
            ) : null}
            <Button
              type="primary"
              loading={submitLoading}
              onClick={handleSubmit}
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
          {/* 快速开始引导 */}
          {!editing && (
            <Alert
              type="info" showIcon
              icon={<BulbOutlined />}
              style={{ marginBottom: 16 }}
              message="第一次配置？建议的 5 步流程"
              description={(
                <div style={{ fontSize: 13, lineHeight: 1.8 }}>
                  <ol style={{ margin: 0, paddingLeft: 18 }}>
                    <li>从「从模板创建」选一个相近模板（如：通用资源站 / Discuz 论坛 / WordPress 博客）</li>
                    <li>填入任务名（会作为文章的 <code>source_type</code> 标识）和列表页 URL</li>
                    <li>用「测试运行」验证选择器命中（不写库），看预览效果</li>
                    <li>根据预览调整字段选择器（标题/正文/网盘链接/图片 等）</li>
                    <li>「立即运行」正式抓取 → 进「爬虫资源」查看结果</li>
                  </ol>
                  <Collapse
                    ghost
                    size="small"
                    style={{ marginTop: 8, marginLeft: -8 }}
                    items={[{
                      key: 'ai',
                      label: (
                        <span style={{ fontSize: 13 }}>
                          <RobotOutlined style={{ color: '#7c3aed', marginRight: 6 }} />
                          找不到选择器？用 Claude / ChatGPT 等 AI 工具辅助提取
                          <Text type="secondary" style={{ fontSize: 12, marginLeft: 6 }}>
                            （点开复制提示词）
                          </Text>
                        </span>
                      ),
                      children: (
                        <div>
                          <Paragraph style={{ fontSize: 12, color: '#6b7280', marginBottom: 8 }}>
                            把下面的提示词复制到 Claude / ChatGPT 等 AI 工具，把 <code>目标站点列表页 URL</code> 换成你的实际地址（域名或分页 URL），AI 会自动抓取页面分析并输出本表单所需的 JSON 配置。
                          </Paragraph>
                          <Paragraph
                            copyable={{
                              text: AI_SELECTOR_PROMPT,
                              tooltips: ['复制提示词', '已复制到剪贴板！'],
                            }}
                            style={{ marginBottom: 0 }}
                          >
                            <pre
                              style={{
                                margin: 0, maxHeight: 240, overflow: 'auto',
                                padding: 12, fontSize: 12, lineHeight: 1.6,
                                background: '#f9fafb', border: '1px solid #e5e7eb',
                                borderRadius: 6, whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                              }}
                            >
                              {AI_SELECTOR_PROMPT}
                            </pre>
                          </Paragraph>
                          <Button
                            type="primary"
                            ghost
                            icon={<ImportOutlined />}
                            style={{ marginTop: 8 }}
                            onClick={() => setAiResultOpen(true)}
                          >
                            读取 AI 结果填入表单
                          </Button>
                        </div>
                      ),
                    }]}
                  />
                </div>
              )}
            />
          )}

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
                  <li>每个 URL 都会按 <code>list_item</code> 选择器解析为多条详情链接</li>
                  <li>支持分页：把每一页的完整 URL 都列出来即可</li>
                  <li><b>启用下方「自动翻页」后，只需填第一页 URL</b>，引擎按分页选择器自动抓所有页</li>
                  <li>需要登录的站点建议先用代理 + 自定义 Cookie（v2）</li>
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
            title={<span><FastForwardOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />②.b 自动翻页（可选）</span>}
          >
            <Alert
              type="warning" showIcon
              style={{ marginBottom: 12 }}
              message="启用后，列表页 URL 只需填第一页"
              description={(
                <div style={{ fontSize: 12, lineHeight: 1.7, color: '#6b7280' }}>
                  引擎抓每页时，会按「分页选择器」从 HTML 中找出所有分页链接（数字页 / 上一页 / 下一页 / 末页），
                  <b>去重后批量扩散</b>抓取，直到 <b>选择器失配</b> 或 <b>所有 URL 都已访问过</b>。
                  建议第一次测试时配合「测试运行」验证，并用「最大页数」限制防失控。
                </div>
              )}
            />
            <Space wrap size="middle" style={{ display: 'flex' }}>
              <Form.Item
                label="分页选择器"
                name="pagination_selector"
                style={{ flex: 1, minWidth: 360 }}
                tooltip={(
                  <div>
                    <div>CSS 选择器，一次性匹配页面所有分页链接（含数字页 + 上下页 + 末页）。
                      引擎会去重后批量抓取。常见写法：</div>
                    <ul style={{ margin: '4px 0', paddingLeft: 16 }}>
                      <li><code>.pagination a</code> — 通用分页容器内所有 a（推荐）</li>
                      <li><code>.pg a</code> — Discuz 分页容器</li>
                      <li><code>.nav-links a</code> — WordPress 经典主题分页</li>
                      <li><code>a[rel=next], a[rel=prev]</code> — 只有上下页的站点</li>
                    </ul>
                    <div>留空 = 不启用自动翻页，仅抓 list_urls 列出的 URL</div>
                    <div>多个用英文逗号分隔，会合并匹配</div>
                  </div>
                )}
                extra="留空 = 不启用；匹配页面所有分页 a 标签（引擎自动去重）"
              >
                <Input placeholder="如：.pagination a 或 .pg a" style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item
                label="最大页数"
                name="max_pages"
                tooltip="0 = 不限（靠选择器失配 / URL 去重自然停止）。建议测试期填 3-5 防止失控"
                extra="页（0=不限，含种子页）"
              >
                <InputNumber min={0} max={10000} style={{ width: 150 }} />
              </Form.Item>
            </Space>
          </Card>

          {/* Section 3: 字段选择器（CSS） */}
          <Card
            size="small"
            style={CARD_STYLE}
            headStyle={CARD_HEAD_STYLE}
            bodyStyle={CARD_BODY_STYLE}
            title={(
              <Space>
                <span><CodeOutlined style={{ color: '#0ea5e9', marginRight: 6 }} />③ 字段选择器（CSS）</span>
                <Tooltip title="查看 CSS 选择器写法示例">
                  <Button
                    type="link" size="small" icon={<BookOutlined />}
                    style={{ padding: 0, height: 'auto' }}
                    onClick={() => setCssHelpOpen(true)}
                  >
                    选择器速查
                  </Button>
                </Tooltip>
              </Space>
            )}
          >
            <Alert
              type="warning" showIcon
              style={{ marginBottom: 12 }}
              message="选择器写法示例"
              description={(
                <div style={{ fontSize: 12, lineHeight: 1.7 }}>
                  类选择器：<code>.post-title</code> ／ ID：<code>#main</code> ／ 后代：<code>.list .item</code><br />
                  标签属性：<code>a.detail</code>（class=detail 的 a 标签）／ 多个匹配取第一个
                </div>
              )}
            />

            <Typography.Text type="secondary" style={{ display: 'block', marginBottom: 8 }}>
              必填：list_item + detail_link
            </Typography.Text>
            <Space wrap size="middle" style={{ display: 'flex', marginBottom: 8 }}>
              <Form.Item
                label="列表项选择器"
                name={['selectors', 'list_item']}
                rules={[{ required: true, message: 'list_item 必填：列表页中每一项的容器' }]}
                style={{ flex: 1, minWidth: 280 }}
                tooltip="列表页中，每一条文章记录的外层容器（HTML 节点）。每个匹配到的节点都会生成一条文章"
                extra="示例：.post-list .post-item"
              >
                <Input placeholder=".post-list .post-item" />
              </Form.Item>
              <Form.Item
                label="详情链接"
                name={['selectors', 'detail_link']}
                rules={[{ required: true, message: 'detail_link 必填' }]}
                style={{ flex: 1, minWidth: 280 }}
                tooltip="从 list_item 容器内取详情页 URL 的元素（通常是 <a> 标签）"
                extra="必填"
              >
                <Input placeholder="a.detail-link" />
              </Form.Item>
              <Form.Item
                label="链接属性"
                name={['selectors', 'detail_link_attr']}
                style={{ width: 180 }}
                tooltip="取哪个属性作为 URL，默认 href。少数站点用 data-href"
              >
                <Input placeholder="href" />
              </Form.Item>
            </Space>

            <Typography.Text type="secondary" style={{ display: 'block', margin: '12px 0 8px' }}>
              详情页字段（从详情页提取）
            </Typography.Text>
            <Space wrap size="middle" style={{ display: 'flex' }}>
              <Form.Item
                label="标题"
                name={['selectors', 'title', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="详情页中标题元素的选择器"
              >
                <Input placeholder="h1.post-title" />
              </Form.Item>
              <Form.Item
                label="正文"
                name={['selectors', 'content', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="详情页正文的容器，会提取内部文本/HTML"
              >
                <Input placeholder=".post-content" />
              </Form.Item>
              <Form.Item
                label="分类"
                name={['selectors', 'category', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="可选：文章分类面包屑"
              >
                <Input placeholder=".post-category" />
              </Form.Item>
              <Form.Item
                label="标签"
                name={['selectors', 'tags', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="可选：标签容器，内部多个子节点会作为多个标签"
              >
                <Input placeholder=".post-tags" />
              </Form.Item>
              <Form.Item
                label="图片"
                name={['selectors', 'images', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="详情页正文内所有图片元素，会走异步上传管线"
              >
                <Input placeholder=".post-content img" />
              </Form.Item>
              <Form.Item
                label="网盘链接"
                name={['selectors', 'pan_links', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="详情页中的网盘链接（quark/baidu/123pan 等 9 平台会自动识别并提取码）"
              >
                <Input placeholder=".download-links a" />
              </Form.Item>
              <Form.Item
                label="直链"
                name={['selectors', 'direct_links', 'css']}
                style={{ flex: 1, minWidth: 260 }}
                tooltip="可选：直接下载链接（.zip/.rar/.pdf 等）"
              >
                <Input placeholder=".direct-download a" />
              </Form.Item>
            </Space>
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
              <TextArea rows={2} placeholder={DEFAULT_USER_AGENT} />
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
                    <TextArea
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

      {/* 模板选择（T054 增强：内置 + 自定义分组） */}
      <Modal
        title="从模板创建"
        open={templatePickerOpen}
        onCancel={() => setTemplatePickerOpen(false)}
        footer={null}
        width={640}
      >
        {templates.length === 0 ? (
          <Empty description="暂无可用模板" />
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
                          <Paragraph type="secondary" style={{ marginTop: 8, marginBottom: 0, fontSize: 12 }}>
                            {tpl.description}
                          </Paragraph>
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

      {/* T054: 保存为模板弹窗 */}
      <Modal
        title="另存为自定义模板"
        open={saveTplOpen}
        onCancel={() => setSaveTplOpen(false)}
        onOk={handleSaveAsTemplate}
        confirmLoading={saveTplLoading}
        okText="保存"
        cancelText="取消"
      >
        <Form form={saveTplForm} layout="vertical" preserve={false}>
          <Form.Item
            name="name"
            label="模板名称"
            rules={[{ required: true, message: '请输入模板名' }]}
          >
            <Input placeholder="如：我的资源站模板" />
          </Form.Item>
          <Form.Item name="description" label="描述（可选）">
            <Input.TextArea rows={3} placeholder="模板用途、适配站点说明" />
          </Form.Item>
          <Alert
            type="info" showIcon
            message="模板将存储在系统配置中，可在新建任务时复用。同名模板会被覆盖。"
          />
        </Form>
      </Modal>

      {/* 测试预览 */}
      <Modal
        title={testTaskId ? `任务 #${testTaskId} 测试预览（不落库）` : '测试预览'}
        open={testOpen}
        onCancel={() => setTestOpen(false)}
        footer={null}
        width={900}
      >
        {testLoading ? (
          <div style={{ textAlign: 'center', padding: 48 }}><Spin tip="抓取中..." /></div>
        ) : testPreview ? (
          <>
            {testPreview.selector_validation.missing_fields.length > 0 && (
              <Alert
                style={{ marginBottom: 12 }}
                type="warning"
                showIcon
                message={`以下选择器未命中: ${testPreview.selector_validation.missing_fields.join(', ')}`}
              />
            )}
            <Descriptions size="small" column={3} bordered style={{ marginBottom: 12 }}>
              <Descriptions.Item label="列表页条数">{testPreview.list_count}</Descriptions.Item>
              <Descriptions.Item label="实际预览">{testPreview.preview_count}</Descriptions.Item>
              <Descriptions.Item label="list_item">{testPreview.selector_validation.list_item_ok ? '✓' : '✗'}</Descriptions.Item>
            </Descriptions>
            {testPreview.articles.length === 0 ? (
              <Empty description="未抓到任何条目 — 请检查 list_item / detail_link 选择器" />
            ) : (
              <Space direction="vertical" style={{ width: '100%' }} size="middle">
                {testPreview.articles.map((a, i) => (
                  <div key={i} style={{ border: '1px solid #e5e7eb', borderRadius: 8, padding: 12 }}>
                    <Text strong>{a.title ?? '(无标题)'}</Text>
                    <br />
                    <Text type="secondary" style={{ fontSize: 12 }}>{a.source_url}</Text>
                    {a.content_snippet && (
                      <Paragraph type="secondary" ellipsis={{ rows: 2 }} style={{ marginTop: 8, marginBottom: 0 }}>
                        {a.content_snippet}
                      </Paragraph>
                    )}
                    <Space wrap style={{ marginTop: 8 }}>
                      {a.pan_links.map((p, j) => (
                        <Tag key={`p${j}`} color="blue">{p.platform}{p.extract_code ? ` · ${p.extract_code}` : ''}</Tag>
                      ))}
                      {a.direct_links.map((_, j) => (
                        <Tag key={`d${j}`} color="geekblue">直链#{j + 1}</Tag>
                      ))}
                      {a.images.length > 0 && <Tag color="purple">图 ×{a.images.length}</Tag>}
                      {a.field_warnings.map((w, j) => (
                        <Tag key={`w${j}`} color="warning">{w}</Tag>
                      ))}
                    </Space>
                  </div>
                ))}
              </Space>
            )}
          </>
        ) : (
          <Empty description="无预览数据" />
        )}
      </Modal>

      {/* CSS 选择器速查 */}
      <Modal
        title="CSS 选择器速查"
        open={cssHelpOpen}
        onCancel={() => setCssHelpOpen(false)}
        footer={<Button type="primary" onClick={() => setCssHelpOpen(false)}>明白了</Button>}
        width={720}
      >
        <Typography.Paragraph type="secondary">
          CSS 选择器告诉爬虫「页面里哪些位置的字段需要抓」。下面是最常用的写法：
        </Typography.Paragraph>

        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label=".classname">
            <Text code>.post-title</Text> 选中所有 <code>class="post-title"</code> 的元素
          </Descriptions.Item>
          <Descriptions.Item label="#id">
            <Text code>#main</Text> 选中 <code>id="main"</code> 的元素（页面内唯一）
          </Descriptions.Item>
          <Descriptions.Item label="tagname">
            <Text code>a</Text> / <Text code>img</Text> 选中所有 a / img 标签
          </Descriptions.Item>
          <Descriptions.Item label="A B（后代）">
            <Text code>.list .item</Text> 选中 .list 内部所有 .item（任意层级）
          </Descriptions.Item>
          <Descriptions.Item label="A &gt; B（直接子代）">
            <Text code>.list &gt; .item</Text> 只选 .list 直接子节点中的 .item
          </Descriptions.Item>
          <Descriptions.Item label="A.className">
            <Text code>a.detail-link</Text> 选 class=detail-link 的 a 标签
          </Descriptions.Item>
          <Descriptions.Item label="A[attr]">
            <Text code>a[href]</Text> 选带 href 属性的 a 标签；<Text code>input[type=text]</Text> 选文本框
          </Descriptions.Item>
          <Descriptions.Item label="多选 A, B">
            <Text code>.title, .name</Text> 同时匹配 .title 或 .name
          </Descriptions.Item>
        </Descriptions>

        <Typography.Paragraph style={{ marginTop: 16 }}>
          <Typography.Text strong>如何找到页面里使用的选择器？</Typography.Text>
        </Typography.Paragraph>
        <ol style={{ paddingLeft: 20, lineHeight: 1.8 }}>
          <li>用 Chrome 打开目标站点的列表页 / 详情页</li>
          <li>右键点要抓的元素 → <code>检查</code>（Inspect）</li>
          <li>在 DevTools 看元素的 <code>class=</code> / <code>id=</code></li>
          <li>组合出选择器，填到表单，用「测试运行」验证</li>
        </ol>

        <Alert
          type="warning" showIcon
          style={{ marginTop: 12 }}
          message="匹配多条时取第一个"
          description="除 list_item 是匹配多条外，其他字段（标题/正文/分类...）匹配到多条时只取第一个。需要精确匹配请在选择器里加上层级的限定。"
        />
      </Modal>

      {/* AI 结果解析弹窗 */}
      <Modal
        title={(
          <span>
            <RobotOutlined style={{ color: '#7c3aed', marginRight: 6 }} />
            粘贴 AI 返回的 JSON，自动填入表单
          </span>
        )}
        open={aiResultOpen}
        onCancel={() => { setAiResultOpen(false); setAiResultText('') }}
        onOk={handleParseAiResult}
        okText="解析并填入"
        cancelText="取消"
        width={720}
        destroyOnClose
      >
        <Alert
          type="info" showIcon
          style={{ marginBottom: 12 }}
          message="把 AI 输出的完整 JSON 粘贴到下方文本框（包含 ```json 代码块也可以，会自动剥离）"
          description={(
            <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12, color: '#6b7280' }}>
              <li>支持裸 JSON / <code>```json ... ```</code> 代码块 / 带解释文字的混合输出</li>
              <li><code>null</code> 字段会被跳过，<code>pagination_selector</code> 空字符串等同未启用</li>
              <li>解析成功后会覆盖当前表单的选择器字段，请先确认表单内容可被替换</li>
            </ul>
          )}
        />
        <TextArea
          rows={16}
          value={aiResultText}
          onChange={(e) => setAiResultText(e.target.value)}
          placeholder={'示例：\n{\n  "list_item": ".post-list .post-item",\n  "detail_link": "a.title",\n  "title": { "css": "h1.art-title" },\n  "content": { "css": ".article-body" },\n  "pan_links": { "css": ".article-body a[href*=\\"pan.baidu\\"]" },\n  "pagination_selector": ".pagination a",\n  "max_pages": 0\n}'}
          style={{ fontFamily: 'monospace', fontSize: 12 }}
        />
      </Modal>
    </div>
  )
}

export default CrawlerTasks
