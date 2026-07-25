import React, { useEffect, useState, useMemo, useCallback } from 'react'
import {
  Table, Button, Form, Input, Select, Space, message, Tag, Popconfirm,
  Tooltip, Typography, Drawer, Image as AntImage, Descriptions, Empty, Spin, Badge,
  Card, Row, Col,
} from 'antd'
import {
  ReloadOutlined, DeleteOutlined, EditOutlined, EyeOutlined, CopyOutlined,
  CheckCircleOutlined, WarningOutlined, QuestionCircleOutlined, CloseCircleOutlined,
  RedoOutlined,
} from '@ant-design/icons'
import { fmtUtc } from '../utils/time'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import apiClient from '../api/client'
import * as crawlerApi from '../api/crawler'
import FieldValueRenderer from '../components/crawler/FieldValueRenderer'
import type {
  CrawlerArticleListItem, CrawlerArticleDetail,
  CrawlerTask, ArticleFieldValue, FieldTree,
} from '../types'

const { Text } = Typography

// ─── 网盘平台图标 / 颜色映射 ────────────────────────────────────────────────
const PLATFORM_META: Record<string, { color: string; label: string }> = {
  quark: { color: '#4f46e5', label: '夸克' },
  uc: { color: '#0ea5e9', label: 'UC' },
  baidu: { color: '#3b82f6', label: '百度' },
  tianyi: { color: '#ef4444', label: '天翼' },
  '123pan': { color: '#f59e0b', label: '123云盘' },
  '115': { color: '#10b981', label: '115' },
  aliyun: { color: '#f97316', label: '阿里' },
  xunlei: { color: '#06b6d4', label: '迅雷' },
  mobile: { color: '#a855f7', label: '移动云盘' },
  unknown: { color: '#6b7280', label: '未知' },
}

const VALIDITY_META: Record<string, { color: string; icon: React.ReactNode; label: string }> = {
  valid: { color: 'success', icon: <CheckCircleOutlined />, label: '有效' },
  invalid: { color: 'error', icon: <CloseCircleOutlined />, label: '失效' },
  pending: { color: 'warning', icon: <WarningOutlined />, label: '待检测' },
  unknown: { color: 'default', icon: <QuestionCircleOutlined />, label: '未检测' },
}

const IMAGE_STATUS_META: Record<string, { color: string; label: string }> = {
  pending: { color: 'default', label: '待处理' },
  downloaded: { color: 'processing', label: '已下载' },
  uploading: { color: 'processing', label: '上传中' },
  uploaded: { color: 'success', label: '已上传' },
  failed: { color: 'error', label: '失败' },
}

// ─── 主组件 ──────────────────────────────────────────────────────────────────
const CrawlerResources: React.FC = () => {
  const [list, setList] = useState<CrawlerArticleListItem[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(false)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [keyword, setKeyword] = useState('')
  const [taskIdFilter, setTaskIdFilter] = useState<number | undefined>(undefined)
  const [categoryFilter, setCategoryFilter] = useState<string | undefined>(undefined)

  // 任务列表（筛选用）
  const [tasks, setTasks] = useState<CrawlerTask[]>([])
  // 字段树缓存（按 taskId 缓存，供详情面板 FieldValueRenderer 查 field_type）
  const [fieldTreeMap, setFieldTreeMap] = useState<Record<number, FieldTree | null>>({})

  // 图片域名（缩略图 / 详情图用）
  const [imageDomain, setImageDomain] = useState('')

  // 详情 Drawer
  const [detailOpen, setDetailOpen] = useState(false)
  const [detailLoading, setDetailLoading] = useState(false)
  const [detail, setDetail] = useState<CrawlerArticleDetail | null>(null)
  const [editing, setEditing] = useState(false)
  const [editForm] = Form.useForm()
  const [linkChecking, setLinkChecking] = useState(false)
  const [retryingId, setRetryingId] = useState<number | null>(null)
  // [feature 046 US4] 字段刷新状态：正在刷新的 field_path 集合
  const [refreshingPaths, setRefreshingPaths] = useState<Set<string>>(new Set())

  const { containerRef: tableContainerRef, scrollY: tableScrollY } = useTableScrollY()

  // 加载任务列表（用于筛选）
  useEffect(() => {
    (async () => {
      try {
        const res = await crawlerApi.listTasks({ page: 1, page_size: 200 })
        setTasks(res.data?.list ?? [])
      } catch { /* ignore */ }
    })()
  }, [])

  // 加载图床域名配置
  useEffect(() => {
    (async () => {
      try {
        const res = await apiClient.get('/options')
        const data = res.data?.data ?? {}
        setImageDomain(data.TelegramImageDomain || '')
      } catch { /* ignore */ }
    })()
  }, [])

  // 拉取文章列表
  const fetchList = useCallback(async () => {
    setLoading(true)
    try {
      const res = await crawlerApi.listArticles({
        page, page_size: pageSize,
        task_id: taskIdFilter,
        category: categoryFilter,
        keyword: keyword || undefined,
      })
      setList(res.data?.list ?? [])
      setTotal(res.data?.pagination?.total ?? 0)
    } catch (e: any) {
      message.error('获取文章列表失败: ' + (e?.message ?? ''))
    } finally {
      setLoading(false)
    }
  }, [page, pageSize, taskIdFilter, categoryFilter, keyword])

  useEffect(() => { fetchList() }, [fetchList])

  // 打开详情
  const openDetail = async (id: number) => {
    setDetailOpen(true)
    setDetailLoading(true)
    setEditing(false)
    try {
      const res = await crawlerApi.getArticleDetail(id)
      // 注意：crawlerApi.getArticleDetail 内部已 `return res.data`，故此处 res 即后端 JSON 顶层对象
      // 后端返回 {success, data: CrawlerArticleDetail, extra_fields, field_values, field_stats}
      // 其中 extra_fields/field_values/field_stats 与 data 并列在顶层，不在 data 内
      const body = res as any
      const article: CrawlerArticleDetail = {
        ...(body?.data ?? {}),
        extra_fields: body?.extra_fields,
        field_values: (body?.field_values ?? []) as ArticleFieldValue[],
        field_stats: body?.field_stats,
      }
      setDetail(article)
      // 同步表单
      editForm.setFieldsValue({
        title: article.title,
        content: article.content,
        category: article.category,
        tags: article.tags,
      })
      // 异步拉字段树（按 taskId 缓存，命中即跳过）
      const tid = article.task_id
      if (tid != null && fieldTreeMap[tid] === undefined) {
        crawlerApi.getTaskFieldTree(tid).then(r => {
          const tree = (r as any)?.data ?? null
          setFieldTreeMap(prev => ({ ...prev, [tid]: tree ?? null }))
        }).catch(() => {
          setFieldTreeMap(prev => ({ ...prev, [tid]: null }))
        })
      }
    } catch (e: any) {
      message.error('加载详情失败: ' + (e?.message ?? ''))
    } finally {
      setDetailLoading(false)
    }
  }

  // 保存编辑
  const saveEdit = async () => {
    if (!detail) return
    try {
      const v = await editForm.validateFields()
      await crawlerApi.updateArticle(detail.id, v)
      message.success('已保存')
      setEditing(false)
      // 刷新详情 + 列表
      await openDetail(detail.id)
      fetchList()
    } catch (e: any) {
      if (e?.errorFields) return // 表单校验错误
      message.error('保存失败: ' + (e?.message ?? ''))
    }
  }

  // 删除单条
  const delArticle = async (id: number) => {
    try {
      await crawlerApi.deleteArticle(id)
      message.success('已删除')
      fetchList()
    } catch (e: any) {
      message.error('删除失败: ' + (e?.message ?? ''))
    }
  }

  // 批量删除
  const [selectedIds, setSelectedIds] = useState<number[]>([])
  const batchDelete = async () => {
    if (selectedIds.length === 0) return
    try {
      const res = await crawlerApi.batchDeleteArticles(selectedIds)
      message.success(`已删除 ${res.data?.deleted ?? 0} 条`)
      setSelectedIds([])
      fetchList()
    } catch (e: any) {
      message.error('批量删除失败: ' + (e?.message ?? ''))
    }
  }

  // 复制链接
  const copyText = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => message.success('已复制', 0.8),
      () => message.error('复制失败'),
    )
  }

  // 链接检测
  const checkLinks = async () => {
    if (!detail) return
    setLinkChecking(true)
    try {
      const res = await crawlerApi.checkArticleLinks(detail.id)
      message.success(res.data?.note ? `检测完成 (${res.data.note})` : `已检测 ${res.data?.checked ?? 0} 条`)
      await openDetail(detail.id)
    } catch (e: any) {
      message.error('检测失败: ' + (e?.message ?? ''))
    } finally {
      setLinkChecking(false)
    }
  }

  // 图片重试
  const retryImage = async (imageId: number) => {
    if (!detail) return
    setRetryingId(imageId)
    try {
      await crawlerApi.retryImage(detail.id, imageId)
      message.success('已重置，稍后自动重试')
      await openDetail(detail.id)
    } catch (e: any) {
      message.error('重置失败: ' + (e?.message ?? ''))
    } finally {
      setRetryingId(null)
    }
  }

  // [feature 046 US4] 手动刷新脚本字段（force_refresh=true）
  const refreshField = useCallback(async (fieldPath: string, fieldName: string) => {
    if (!detail) return
    setRefreshingPaths(prev => new Set(prev).add(fieldPath))
    try {
      const res = await crawlerApi.refreshArticleField(detail.id, fieldName)
      const data = (res as any)?.data
      const newV = data?.new_value
      const oldV = data?.old_value
      if (newV && newV !== oldV) {
        message.success(`字段已刷新（耗时 ${data?.duration_ms ?? 0} ms）`)
      } else {
        message.info(`字段刷新完成但值未变化（耗时 ${data?.duration_ms ?? 0} ms）`)
      }
      // 重新打开详情拉新值
      await openDetail(detail.id)
    } catch (e: any) {
      const msg: string = e?.response?.data?.error ?? e?.message ?? '刷新失败'
      // 分类中文映射
      const categorized = msg.replace(/.*\[(\w+)\].*/, (_, cat) => {
        const map: Record<string, string> = {
          syntax_error: '脚本语法错',
          security_violation: '安全策略拦截',
          runtime_error: '脚本运行错',
          type_error: '类型错',
          timeout: '执行超时',
        }
        return map[cat] ? `${map[cat]}` : cat
      })
      message.error('字段刷新失败: ' + categorized)
    } finally {
      setRefreshingPaths(prev => {
        const next = new Set(prev)
        next.delete(fieldPath)
        return next
      })
    }
  }, [detail])

  // 拼接图片可访问 URL
  const buildImageUrl = (fileId: string | null, msgId: number | null): string | null => {
    if (fileId) {
      const d = imageDomain ? imageDomain.replace(/\/+$/, '') : ''
      return d ? `${d}/${fileId}` : `/api/images/${fileId}`
    }
    if (msgId) return `/api/images/${msgId}`
    return null
  }

  // 任务名映射
  const taskNameMap = useMemo(() => {
    const m = new Map<number, string>()
    tasks.forEach(t => m.set(t.id, t.name))
    return m
  }, [tasks])

  // ─── 列定义 ───────────────────────────────────────────────────────────────
  const columns = [
    {
      title: '', dataIndex: 'thumbnail', width: 70, key: 'thumbnail',
      render: (thumb: string | null, r: CrawlerArticleListItem) => {
        // 优先用 extra_fields.cover（爬虫字段提取的封面 URL，可直接用作 img src）；
        // 否则回退到图片代理（thumbnail=Telegram file_id）
        const cover = efStr(r, 'cover')
        const url = cover
          ? cover
          : thumb
            ? (imageDomain ? `${imageDomain.replace(/\/+$/, '')}/${thumb}` : `/api/images/${thumb}`)
            : null
        return url ? (
          <AntImage
            src={url}
            width={50}
            height={50}
            style={{ objectFit: 'cover', borderRadius: 6 }}
            preview={false}
            fallback="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI1MCIgaGVpZ2h0PSI1MCI+PHJlY3Qgd2lkdGg9IjUwIiBoZWlnaHQ9IjUwIiBmaWxsPSIjZTVlN2ViIi8+PC9zdmc+"
          />
        ) : (
          <div style={{
            width: 50, height: 50, borderRadius: 6,
            background: '#f3f4f6', display: 'flex',
            alignItems: 'center', justifyContent: 'center',
            color: '#9ca3af', fontSize: 11,
          }}>无图</div>
        )
      },
    },
    {
      title: '标题', dataIndex: 'title', key: 'title', ellipsis: true,
      render: (t: string | null, r: CrawlerArticleListItem) => {
        // title 列（crawler_articles.title）常因 CSS 命中空元素被填空白字符；先 trim 判定，
        // 空白/空 → 回退到 extra_fields.title / .name；都没有显示"(无标题)"
        const tStr = t && t.trim() ? t : null
        const title = tStr || efStr(r, 'title') || efStr(r, 'name')
        return (
          <Space size={4}>
            <Tooltip title={title ?? '(无标题)'}>
              <a onClick={() => openDetail(r.id)} style={{ fontWeight: 500 }}>
                {title || '(无标题)'}
              </a>
            </Tooltip>
            {r.is_edited && <Tag color="purple" style={{ marginInlineStart: 0 }}>已编辑</Tag>}
          </Space>
        )
      },
    },
    {
      title: '来源', dataIndex: 'source_type', width: 110, key: 'source_type',
      render: (s: string, r: CrawlerArticleListItem) => (
        <Tooltip title={taskNameMap.get(r.task_id ?? -1) ?? s}>
          <Tag>{s}</Tag>
        </Tooltip>
      ),
    },
    {
      title: '分类', dataIndex: 'category', width: 100, key: 'category', ellipsis: true,
      render: (c: string | null) => c ? <Tag color="blue">{c}</Tag> : <Text type="secondary">-</Text>,
    },
    {
      title: '下载', dataIndex: 'pan_link_count', width: 70, key: 'pan',
      align: 'center' as const,
      // 网盘/直链计数表（crawler_article_links）从未写入，恒为 0；用 extra_fields.download_url 命中数兜底
      render: (n: number, r: CrawlerArticleListItem) => {
        const c = Math.max(n, efCount(r, 'download_url'))
        return c > 0 ? <Badge count={c} style={{ backgroundColor: '#4f46e5' }} /> : <Text type="secondary">0</Text>
      },
    },
    {
      title: '直链', dataIndex: 'direct_link_count', width: 70, key: 'direct',
      align: 'center' as const,
      render: (n: number) => n > 0 ? <Badge count={n} color="#0ea5e9" /> : <Text type="secondary">0</Text>,
    },
    {
      title: '图片', dataIndex: 'image_count', width: 70, key: 'images',
      align: 'center' as const,
      render: (n: number) => <Badge count={n} color="#10b981" />,
    },
    {
      title: '采集时间', dataIndex: 'crawled_at', width: 150, key: 'crawled_at',
      render: (t: string) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {fmtUtc(t, 'YYYY-MM-DD HH:mm')}
        </Text>
      ),
    },
    {
      title: '操作', key: 'actions', width: 130, fixed: 'right' as const,
      render: (_: any, r: CrawlerArticleListItem) => (
        <Space size={4}>
          <Tooltip title="查看详情">
            <Button size="small" icon={<EyeOutlined />} onClick={() => openDetail(r.id)} />
          </Tooltip>
          <Tooltip title="编辑">
            <Button size="small" icon={<EditOutlined />}
              onClick={async () => { await openDetail(r.id); setEditing(true) }} />
          </Tooltip>
          <Popconfirm title="确认删除？关联的链接和图片也会被删除"
            onConfirm={() => delArticle(r.id)}>
            <Tooltip title="删除">
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ]

  // ─── 渲染 ───────────────────────────────────────────────────────────────
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      <PageHeader
        title="爬虫资源"
        description="管理采集到的文章、网盘链接、直链和图片"
        extra={
          <Space>
            {selectedIds.length > 0 && (
              <Popconfirm title={`确认批量删除 ${selectedIds.length} 条？`} onConfirm={batchDelete}>
                <Button danger icon={<DeleteOutlined />}>
                  批量删除 ({selectedIds.length})
                </Button>
              </Popconfirm>
            )}
            <Button icon={<ReloadOutlined />} onClick={fetchList}>刷新</Button>
          </Space>
        }
      />

      {/* 筛选栏 */}
      <Space wrap style={{ flexShrink: 0 }}>
        <Input.Search
          placeholder="搜索标题或内容"
          allowClear
          style={{ width: 240 }}
          onSearch={v => { setKeyword(v); setPage(1) }}
        />
        <Select
          placeholder="按任务筛选"
          allowClear
          style={{ width: 200 }}
          value={taskIdFilter}
          onChange={v => { setTaskIdFilter(v); setPage(1) }}
          options={tasks.map(t => ({ label: t.name, value: t.id }))}
        />
        <Input
          placeholder="分类"
          allowClear
          style={{ width: 140 }}
          value={categoryFilter ?? ''}
          onChange={e => setCategoryFilter(e.target.value || undefined)}
          onPressEnter={() => { setPage(1); fetchList() }}
        />
      </Space>

      {/* 表格 */}
      <div ref={tableContainerRef} style={{ flex: 1, minHeight: 0 }}>
        <Table
          rowKey="id"
          dataSource={list}
          columns={columns as any}
          loading={loading}
          scroll={{ x: 1000, y: tableScrollY }}
          size="middle"
          rowSelection={{
            selectedRowKeys: selectedIds,
            onChange: (keys) => setSelectedIds(keys as number[]),
          }}
          pagination={{
            current: page,
            pageSize,
            total,
            showSizeChanger: true,
            showTotal: t => `共 ${t} 条`,
            onChange: (p, ps) => { setPage(p); setPageSize(ps) },
          }}
        />
      </div>

      {/* 详情 Drawer */}
      <Drawer
        width={720}
        open={detailOpen}
        onClose={() => { setDetailOpen(false); setEditing(false) }}
        title={detail ? (editing ? '编辑文章' : '文章详情') : '加载中...'}
        extra={
          detail && (
            <Space>
              {editing ? (
                <>
                  <Button onClick={() => setEditing(false)}>取消</Button>
                  <Button type="primary" onClick={saveEdit}>保存</Button>
                </>
              ) : (
                <>
                  <Tooltip title="重新检测所有网盘链接">
                    <Button loading={linkChecking} onClick={checkLinks}
                      icon={<CheckCircleOutlined />}>检测链接</Button>
                  </Tooltip>
                  <Button icon={<EditOutlined />} onClick={() => setEditing(true)}>编辑</Button>
                </>
              )}
            </Space>
          )
        }
      >
        {detailLoading || !detail ? (
          <div style={{ textAlign: 'center', padding: 60 }}><Spin /></div>
        ) : (
          <DetailBody
            detail={detail}
            editing={editing}
            form={editForm}
            buildImageUrl={buildImageUrl}
            copyText={copyText}
            retryImage={retryImage}
            retryingId={retryingId}
            fieldValues={detail.field_values ?? []}
            fieldTree={detail.task_id != null ? fieldTreeMap[detail.task_id] ?? null : null}
            onRefreshField={refreshField}
            refreshingPaths={refreshingPaths}
          />
        )}
      </Drawer>
    </div>
  )
}

// ─── extra_fields 字段取值辅助（爬虫字段树提取结果，后端已拍平注入到每条列表项） ─────────
// 列表/详情的 title/thumbnail/网盘 列读的是空表（crawler_articles.title 等三张表无 INSERT），
// 真实字段值在 extra_fields（key=field_path 末段，值=string|string[]），用它兜底显示。
// 模块级纯函数（无闭包依赖），列表组件与 DetailBody 子组件共用。
//
// 后端 build_extra_fields_json 把同字段多个命中聚合成数组；CSS 选择器（如 h1）可能
// 匹配到多个元素（首个可能是空白装饰元素）。因此数组场景必须扫描全部元素取首个 trim 非空值，
// 不能只看 [0]。
const efStr = (r: { extra_fields?: Record<string, string | string[]> }, key: string): string | null => {
  const v = r.extra_fields?.[key]
  if (Array.isArray(v)) {
    for (const item of v) {
      if (typeof item === 'string' && item.trim()) return item
    }
    return null
  }
  return typeof v === 'string' && v.trim() ? v : null
}
const efCount = (r: { extra_fields?: Record<string, string | string[]> }, key: string): number => {
  const v = r.extra_fields?.[key]
  if (Array.isArray(v)) return v.length
  return v ? 1 : 0
}

// ─── 详情 Drawer 内容（编辑/只读两态） ───────────────────────────────────────
interface DetailBodyProps {
  detail: CrawlerArticleDetail
  editing: boolean
  form: any
  buildImageUrl: (fileId: string | null, msgId: number | null) => string | null
  copyText: (t: string) => void
  retryImage: (imageId: number) => void
  retryingId: number | null
  fieldValues: ArticleFieldValue[]
  fieldTree: FieldTree | null
  /** [feature 046] 字段刷新回调 */
  onRefreshField?: (fieldPath: string, fieldName: string) => Promise<void>
  /** [feature 046] 正在刷新的字段集合 */
  refreshingPaths?: Set<string>
}

const DetailBody: React.FC<DetailBodyProps> = ({
  detail, editing, form, buildImageUrl, copyText, retryImage, retryingId,
  fieldValues, fieldTree, onRefreshField, refreshingPaths,
}) => {
  const panLinks = detail.links.filter(l => l.link_type === 'pan')
  const directLinks = detail.links.filter(l => l.link_type === 'direct')

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* 字段树提取结果（用户在字段配置器里配的所有字段，含未命中） */}
      {!editing && fieldValues.length > 0 && (
        <Card
          title={<Space>字段提取结果 <Badge count={fieldValues.filter(v => v.is_hit).length} showZero style={{ backgroundColor: '#1677ff' }} /></Space>}
          size="small"
        >
          <FieldValueRenderer
            values={fieldValues}
            fieldTree={fieldTree}
            emptyHint="无字段提取数据"
            onRefresh={onRefreshField}
            refreshingPaths={refreshingPaths}
          />
        </Card>
      )}

      {/* 基本信息 */}
      <Card title="基本信息" size="small">
        {editing ? (
          <Form form={form} layout="vertical">
            <Form.Item name="title" label="标题"><Input /></Form.Item>
            <Form.Item name="category" label="分类"><Input /></Form.Item>
            <Form.Item name="tags" label="标签"><Input /></Form.Item>
            <Form.Item name="content" label="正文（HTML）">
              <Input.TextArea rows={8} style={{ fontFamily: 'monospace', fontSize: 12 }} />
            </Form.Item>
          </Form>
        ) : (
          <Descriptions column={2} size="small">
            <Descriptions.Item label="标题" span={2}>
              <Text strong>{(detail.title && detail.title.trim() ? detail.title : null)
                || efStr(detail as any, 'title')
                || efStr(detail as any, 'name')
                || '(无标题)'}</Text>
            </Descriptions.Item>
            <Descriptions.Item label="分类">
              {detail.category ? <Tag color="blue">{detail.category}</Tag> : '-'}
            </Descriptions.Item>
            <Descriptions.Item label="来源">
              <Tag>{detail.source_type}</Tag>
            </Descriptions.Item>
            <Descriptions.Item label="任务" span={2}>
              {detail.task_name ?? detail.task_id ?? '-'}
            </Descriptions.Item>
            <Descriptions.Item label="采集时间" span={2}>
              {fmtUtc(detail.crawled_at)}
            </Descriptions.Item>
            <Descriptions.Item label="原始 URL" span={2}>
              <a href={detail.source_url} target="_blank" rel="noreferrer"
                style={{ wordBreak: 'break-all', fontSize: 12 }}>
                {detail.source_url}
              </a>
            </Descriptions.Item>
            {detail.tags && (
              <Descriptions.Item label="标签" span={2}>{detail.tags}</Descriptions.Item>
            )}
          </Descriptions>
        )}
      </Card>

      {/* 正文：左采集内容（HTML 源码） / 右 HTML 预览（非编辑态） */}
      {!editing && detail.content && (
        <Row gutter={12}>
          <Col span={12}>
            <Card title="采集内容" size="small" styles={{ body: { padding: 0 } }}>
              <pre style={{
                margin: 0, padding: 12,
                maxHeight: 360, overflow: 'auto',
                fontSize: 12, lineHeight: 1.6,
                fontFamily: 'ui-monospace, Menlo, Consolas, "Courier New", monospace',
                whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                color: '#374151',
              }}>
                {detail.content}
              </pre>
            </Card>
          </Col>
          <Col span={12}>
            <Card title="HTML 预览" size="small">
              <div
                style={{ maxHeight: 360, overflow: 'auto', fontSize: 13, lineHeight: 1.7 }}
                dangerouslySetInnerHTML={{ __html: detail.content }}
              />
            </Card>
          </Col>
        </Row>
      )}

      {/* 网盘链接 */}
      <Card
        title={<Space>网盘链接 <Badge count={panLinks.length} showZero style={{ backgroundColor: '#4f46e5' }} /></Space>}
        size="small"
      >
        {panLinks.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="无网盘链接" />
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {panLinks.map(l => {
              const pm = PLATFORM_META[l.platform ?? 'unknown'] ?? PLATFORM_META.unknown
              const vm = VALIDITY_META[l.validity_status] ?? VALIDITY_META.unknown
              return (
                <div key={l.id} style={{
                  border: '1px solid #e5e7eb', borderRadius: 8, padding: 10,
                  display: 'flex', flexDirection: 'column', gap: 6,
                }}>
                  <Space>
                    <Tag color={pm.color}>{pm.label}</Tag>
                    <Tag color={vm.color} icon={vm.icon}>{vm.label}</Tag>
                    {l.validity_reason && (
                      <Tooltip title={l.validity_reason}>
                        <Text type="secondary" style={{ fontSize: 11 }}>原因</Text>
                      </Tooltip>
                    )}
                  </Space>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <Text copyable={false} style={{
                      flex: 1, fontFamily: 'monospace', fontSize: 12,
                      color: '#374151', wordBreak: 'break-all',
                    }}>
                      {l.url}
                    </Text>
                    <Tooltip title="复制链接">
                      <Button size="small" icon={<CopyOutlined />} onClick={() => copyText(l.url)} />
                    </Tooltip>
                  </div>
                  {l.extract_code && (
                    <Space>
                      <Text type="secondary" style={{ fontSize: 12 }}>提取码:</Text>
                      <Text code style={{ letterSpacing: 1 }}>{l.extract_code}</Text>
                      <Tooltip title="复制提取码">
                        <Button size="small" type="link" icon={<CopyOutlined />}
                          onClick={() => copyText(l.extract_code ?? '')} />
                      </Tooltip>
                    </Space>
                  )}
                  {l.last_checked_at && (
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      最近检测: {fmtUtc(l.last_checked_at, 'YYYY-MM-DD HH:mm')}
                    </Text>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </Card>

      {/* 直链 */}
      {directLinks.length > 0 && (
        <Card title={<Space>直链下载 <Badge count={directLinks.length} color="#0ea5e9" /></Space>} size="small">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {directLinks.map(l => (
              <div key={l.id} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <Text style={{
                  flex: 1, fontFamily: 'monospace', fontSize: 12,
                  color: '#374151', wordBreak: 'break-all',
                }}>
                  {l.url}
                </Text>
                <Tooltip title="复制">
                  <Button size="small" icon={<CopyOutlined />} onClick={() => copyText(l.url)} />
                </Tooltip>
              </div>
            ))}
          </div>
        </Card>
      )}

      {/* 图片画廊 */}
      <Card
        title={<Space>图片 <Badge count={detail.images.length} color="#10b981" /></Space>}
        size="small"
      >
        {detail.images.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="无图片" />
        ) : (
          <div style={{
            display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(120px, 1fr))',
            gap: 10,
          }}>
            {detail.images.map(img => {
              const url = buildImageUrl(img.file_id, img.image_message_id)
              const sm = IMAGE_STATUS_META[img.status] ?? IMAGE_STATUS_META.pending
              return (
                <div key={img.id} style={{
                  border: '1px solid #e5e7eb', borderRadius: 8, overflow: 'hidden',
                  display: 'flex', flexDirection: 'column',
                }}>
                  <div style={{
                    height: 100, background: '#f3f4f6',
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                  }}>
                    {url ? (
                      <AntImage
                        src={url}
                        width="100%"
                        height={100}
                        style={{ objectFit: 'cover' }}
                        fallback="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxMjAiIGhlaWdodD0iMTAwIj48cmVjdCB3aWR0aD0iMTIwIiBoZWlnaHQ9IjEwMCIgZmlsbD0iI2YzZjRmNiIvPjwvc3ZnPg=="
                      />
                    ) : (
                      <Text type="secondary" style={{ fontSize: 11 }}>无预览</Text>
                    )}
                  </div>
                  <div style={{ padding: '4px 6px', fontSize: 11, borderTop: '1px solid #f0f0f0' }}>
                    <Space style={{ width: '100%', justifyContent: 'space-between' }}>
                      <Tag color={sm.color} style={{ marginInlineEnd: 0 }}>{sm.label}</Tag>
                      {img.status !== 'uploaded' && (
                        <Tooltip title="重置并重试上传">
                          <Button size="small" type="link"
                            loading={retryingId === img.id}
                            icon={<RedoOutlined />}
                            onClick={() => retryImage(img.id)}
                            style={{ padding: 0 }} />
                        </Tooltip>
                      )}
                    </Space>
                    {img.last_error && (
                      <Tooltip title={img.last_error}>
                        <Text type="danger" style={{ fontSize: 10, display: 'block',
                          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {img.last_error}
                        </Text>
                      </Tooltip>
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </Card>
    </div>
  )
}

export default CrawlerResources
