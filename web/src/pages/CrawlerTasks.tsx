import React, { useEffect, useState, useMemo } from 'react'
import {
  Table, Button, Modal, Form, Input, InputNumber, Select, Switch, Space,
  message, Tag, Popconfirm, Drawer, Tooltip, Typography, Alert, Descriptions, Empty, Spin,
} from 'antd'
import {
  PlusOutlined, EditOutlined, DeleteOutlined, PlayCircleOutlined,
  ThunderboltOutlined, ExportOutlined, ImportOutlined, SaveOutlined, ReloadOutlined,
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
      }
      if (values.list_urls.length === 0) {
        message.warning('请至少填写一个列表页 URL')
        return
      }
    } catch {
      return
    }
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
    }
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
        width={720}
        extra={[
          <Button key="cancel" onClick={() => setEditorOpen(false)}>取消</Button>,
          editing ? (
            <Tooltip key="saveTpl" title="将当前任务配置存为可复用的自定义模板">
              <Button icon={<SaveOutlined />} onClick={() => setSaveTplOpen(true)}>
                另存为模板
              </Button>
            </Tooltip>
          ) : null,
          <Button key="save" type="primary" onClick={handleSubmit}>保存</Button>,
        ]}
        destroyOnClose
      >
        <Form form={form} layout="vertical" initialValues={emptyInput() as any}>
          <Form.Item label="任务名" name="name" rules={[{ required: true, message: '请输入任务名' }]}>
            <Input placeholder="如：example-resource-site" />
          </Form.Item>
          <Form.Item label="启用" name="enabled" valuePropName="checked">
            <Switch />
          </Form.Item>

          <Form.Item label="列表页 URL（每行一个）" required>
            <Form.List name="list_urls">
              {(fields, { add, remove }) => (
                <>
                  {fields.map((f) => (
                    <Space key={f.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                      <Form.Item name={f.name} noStyle rules={[{ required: true, message: 'URL 不能为空' }]}>
                        <Input placeholder="https://example.com/list?page=1" style={{ width: 560 }} />
                      </Form.Item>
                      <Button type="text" danger size="small" icon={<DeleteOutlined />} onClick={() => remove(f.name)} />
                    </Space>
                  ))}
                  <Button type="dashed" icon={<PlusOutlined />} onClick={() => add('')}>添加 URL</Button>
                </>
              )}
            </Form.List>
          </Form.Item>

          <Form.Item
            label="抓取模式"
            name="two_stage"
            valuePropName="checked"
            tooltip={
              <div>
                <div><b>两阶段</b>（推荐）：列表页 → 提取详情链接 → 抓取详情页字段</div>
                <div><b>单阶段</b>：直接按 list_item 抓取列表页字段（适用于列表自带完整内容的站点，如 RSS-like）</div>
              </div>
            }
          >
            <Switch checkedChildren="两阶段" unCheckedChildren="单阶段" />
          </Form.Item>
          <Alert
            type="info" showIcon
            style={{ marginBottom: 16 }}
            message="关闭两阶段时，list_item 选择器命中的每一项会被当作独立文章直接提取字段，detail_link / detail_link_attr 不会被使用。"
          />

          <Typography.Title level={5} style={{ marginTop: 16 }}>字段选择器（CSS）</Typography.Title>
          <Form.Item label="列表项选择器 (list_item)" name={['selectors', 'list_item']} rules={[{ required: true }]}>
            <Input placeholder=".post-list .post-item" />
          </Form.Item>
          <Form.Item label="详情链接 (detail_link)" name={['selectors', 'detail_link']} rules={[{ required: true }]}>
            <Input placeholder="a.detail-link" />
          </Form.Item>
          <Form.Item label="详情链接属性 (detail_link_attr)" name={['selectors', 'detail_link_attr']}>
            <Input placeholder="href" style={{ width: 200 }} />
          </Form.Item>

          <Space wrap size="middle">
            <Form.Item label="标题 (title.css)" name={['selectors', 'title', 'css']} style={{ width: 280 }}>
              <Input placeholder="h1.post-title" />
            </Form.Item>
            <Form.Item label="正文 (content.css)" name={['selectors', 'content', 'css']} style={{ width: 280 }}>
              <Input placeholder=".post-content" />
            </Form.Item>
            <Form.Item label="分类 (category.css)" name={['selectors', 'category', 'css']} style={{ width: 280 }}>
              <Input placeholder=".post-category" />
            </Form.Item>
            <Form.Item label="标签 (tags.css)" name={['selectors', 'tags', 'css']} style={{ width: 280 }}>
              <Input placeholder=".post-tags" />
            </Form.Item>
            <Form.Item label="图片 (images.css)" name={['selectors', 'images', 'css']} style={{ width: 280 }}>
              <Input placeholder=".post-content img" />
            </Form.Item>
            <Form.Item label="网盘链接 (pan_links.css)" name={['selectors', 'pan_links', 'css']} style={{ width: 280 }}>
              <Input placeholder=".download-links a" />
            </Form.Item>
            <Form.Item label="直链 (direct_links.css)" name={['selectors', 'direct_links', 'css']} style={{ width: 280 }}>
              <Input placeholder=".direct-download a" />
            </Form.Item>
          </Space>

          <Typography.Title level={5} style={{ marginTop: 16 }}>调度与并发</Typography.Title>
          <Space wrap size="middle">
            <Form.Item label="间隔(分钟)" name="interval_minutes" rules={[{ required: true }]}>
              <InputNumber min={1} max={1440} style={{ width: 150 }} />
            </Form.Item>
            <Form.Item label="任务级并发" name="task_concurrency" rules={[{ required: true }]}>
              <InputNumber min={1} max={10} style={{ width: 150 }} />
            </Form.Item>
            <Form.Item label="请求间隔(ms)" name="request_delay_ms">
              <InputNumber min={0} max={60000} style={{ width: 150 }} />
            </Form.Item>
            <Form.Item label="最大连续失败" name="max_consecutive_failures">
              <InputNumber min={1} max={100} style={{ width: 150 }} />
            </Form.Item>
          </Space>

          <Typography.Title level={5} style={{ marginTop: 16 }}>网络</Typography.Title>
          <Form.Item label="User-Agent" name="user_agent">
            <TextArea rows={2} placeholder={DEFAULT_USER_AGENT} />
          </Form.Item>
          <Form.Item label="代理（覆盖系统 http_proxy_url）" name="proxy">
            <Input placeholder="http://127.0.0.1:7890 或留空" />
          </Form.Item>
          <Form.Item label="自动链接检测" name="auto_link_check" valuePropName="checked" tooltip="抓取后自动调用 PanCheck 校验网盘链接">
            <Switch />
          </Form.Item>

          <Typography.Title level={5} style={{ marginTop: 16 }}>其他</Typography.Title>
          <Form.Item label="模板来源（标记用）" name="template_source">
            <Input placeholder="generic_resource_site 或自定义" />
          </Form.Item>
          <Form.Item label="拦截检测覆盖（JSON）" name="block_detection_config">
            <TextArea rows={3} placeholder='留空走默认；示例：{"empty_threshold_chars": 200}' />
          </Form.Item>
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
    </div>
  )
}

export default CrawlerTasks
