import { useCallback, useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import {
  Alert,
  Button,
  Card,
  Col,
  Input,
  Row,
  Space,
  Spin,
  Tabs,
  Typography,
} from 'antd'
import { ArrowLeftOutlined, ArrowRightOutlined, ReloadOutlined } from '@ant-design/icons'
import * as crawlerApi from '../api/crawler'
import type { CrawlerTask, FieldScope, FieldTree, QuickFieldPreset, SourceMaterial } from '../types'
import SourceViewer from '../components/crawler/SourceViewer'
import FieldTreePanel from '../components/crawler/FieldTreePanel'

const { Text, Title } = Typography

/** 字段配置器主页面（US1 T033）
 *
 * 顶部 URL 输入 → 调用 fetchSource 拉 4-tab 素材 → 左右分栏：
 * 左：SourceViewer（4 tab 只读源码）
 * 右：FieldTreePanel（字段树 CRUD）
 */
export default function CrawlerFieldConfigurator() {
  const [params] = useSearchParams()
  const navigate = useNavigate()
  const taskId = Number(params.get('taskId') ?? 0)
  const initialUrl = params.get('listUrl') ?? ''

  const [task, setTask] = useState<CrawlerTask | null>(null)
  const [taskLoading, setTaskLoading] = useState(true)
  const [taskError, setTaskError] = useState<string | null>(null)

  const [urlInput, setUrlInput] = useState(initialUrl)
  const [fetching, setFetching] = useState(false)
  const [fetchError, setFetchError] = useState<string | null>(null)
  const [material, setMaterial] = useState<SourceMaterial | null>(null)

  // US3：列表/详情页作用域切换 + 详情样本素材
  const [scope, setScope] = useState<FieldScope>('list_page')
  const [detailUrl, setDetailUrl] = useState<string | null>(null)
  const [detailMaterial, setDetailMaterial] = useState<SourceMaterial | null>(null)
  const [detailFetching, setDetailFetching] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)

  const [tree, setTree] = useState<FieldTree | null>(null)
  const [treeLoading, setTreeLoading] = useState(false)
  const [treeError, setTreeError] = useState<string | null>(null)

  /** 行内快捷创建的预填配置（SourceViewer 触发 → FieldTreePanel 消费打开编辑器） */
  const [quickPreset, setQuickPreset] = useState<QuickFieldPreset | null>(null)

  // 加载任务
  useEffect(() => {
    if (!taskId) {
      setTaskLoading(false)
      setTaskError('缺少 taskId 参数')
      return
    }
    setTaskLoading(true)
    crawlerApi
      .getTask(taskId)
      .then((res) => {
        if (res.success && res.data) {
          setTask(res.data)
          if (!initialUrl) {
            // 任务 list_urls 第一个作为默认 URL
            const firstUrl = parseListUrls(res.data.list_urls)[0]
            if (firstUrl) setUrlInput(firstUrl)
          }
        } else {
          setTaskError(res.message ?? '任务加载失败')
        }
      })
      .catch((e: unknown) => {
        const err = e as { response?: { data?: { message?: string } }; message?: string }
        setTaskError(err.response?.data?.message ?? err.message ?? '任务加载失败')
      })
      .finally(() => setTaskLoading(false))
  }, [taskId, initialUrl])

  const refreshTree = useCallback(() => {
    if (!taskId) return
    setTreeLoading(true)
    setTreeError(null)
    crawlerApi
      .getTaskFieldTree(taskId)
      .then((res) => {
        if (res.success && res.data) {
          setTree(res.data)
        } else {
          setTreeError(res.message ?? '字段树加载失败')
        }
      })
      .catch((e: unknown) => {
        const err = e as { response?: { data?: { message?: string } }; message?: string }
        setTreeError(err.response?.data?.message ?? err.message ?? '字段树加载失败')
      })
      .finally(() => setTreeLoading(false))
  }, [taskId])

  // 初次加载树
  useEffect(() => {
    refreshTree()
  }, [refreshTree])

  const handleFetch = async () => {
    if (!urlInput.trim()) {
      setFetchError('请填写 URL')
      return
    }
    setFetching(true)
    setFetchError(null)
    setMaterial(null)
    try {
      const res = await crawlerApi.fetchSource({
        url: urlInput.trim(),
        user_agent: task?.user_agent ?? undefined,
        proxy: task?.proxy ?? undefined,
      })
      if (res.success && res.data) {
        setMaterial(res.data)
      } else {
        setFetchError(res.message ?? '抓取失败')
      }
    } catch (e: unknown) {
      const err = e as { response?: { data?: { message?: string } }; message?: string }
      setFetchError(err.response?.data?.message ?? err.message ?? '抓取失败')
    } finally {
      setFetching(false)
    }
  }

  /** US3：取详情样本素材（首次切到 detail_page tab 时触发） */
  const handleFetchDetailSample = useCallback(async () => {
    if (!taskId || !urlInput.trim()) return
    setDetailFetching(true)
    setDetailError(null)
    try {
      const res = await crawlerApi.fetchDetailSample({
        task_id: taskId,
        list_url: urlInput.trim(),
        user_agent: task?.user_agent ?? undefined,
        proxy: task?.proxy ?? undefined,
      })
      if (res.success && res.data) {
        setDetailUrl(res.data.detail_url)
        setDetailMaterial(res.data.material)
      } else {
        setDetailError(res.message ?? '取详情样本失败')
      }
    } catch (e: unknown) {
      const err = e as { response?: { data?: { message?: string } }; message?: string }
      setDetailError(err.response?.data?.message ?? err.message ?? '取详情样本失败')
    } finally {
      setDetailFetching(false)
    }
  }, [taskId, urlInput, task])

  /** 切 scope 时按需触发抓取 */
  const handleScopeChange = (key: string) => {
    const next = key as FieldScope
    setScope(next)
    if (next === 'detail_page' && !detailMaterial && !detailFetching) {
      handleFetchDetailSample()
    }
  }

  if (taskLoading) {
    return (
      <div style={{ padding: 40, textAlign: 'center' }}>
        <Spin />
      </div>
    )
  }
  if (taskError) {
    return (
      <div style={{ padding: 16 }}>
        <Alert type="error" showIcon message={taskError} />
      </div>
    )
  }

  return (
    <div style={{ padding: '16px 16px 24px', display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <Title level={4} style={{ marginTop: 0 }}>
        <Space align="center">
          <Button
            type="text"
            icon={<ArrowLeftOutlined />}
            onClick={() => navigate('/crawler/tasks')}
            title="返回任务列表"
          />
          <span>字段配置器{task ? ` — ${task.name}` : ''}</span>
        </Space>
      </Title>

      <Card size="small" style={{ marginBottom: 12 }}>
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="输入列表/详情 URL（https://...）"
            value={urlInput}
            onChange={(e) => setUrlInput(e.target.value)}
            onPressEnter={handleFetch}
            prefix={<Text type="secondary" style={{ fontSize: 12 }}>URL</Text>}
          />
          <Button
            type="primary"
            icon={<ArrowRightOutlined />}
            onClick={handleFetch}
            loading={fetching}
          >
            继续
          </Button>
          <Button icon={<ReloadOutlined />} onClick={refreshTree} loading={treeLoading}>
            刷新树
          </Button>
        </Space.Compact>
        {fetchError && (
          <Alert
            type="error"
            showIcon
            message={fetchError}
            style={{ marginTop: 8 }}
            closable
            onClose={() => setFetchError(null)}
          />
        )}
      </Card>

      <Row gutter={12} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Col span={14} style={{ height: '100%', display: 'flex', flexDirection: 'column', minHeight: 0 }}>
          <Card
            size="small"
            styles={{ body: { padding: 0, flex: 1, minHeight: 0, overflow: 'hidden' } }}
            style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
            title={
              <Tabs
                activeKey={scope}
                onChange={handleScopeChange}
                size="small"
                style={{ marginBottom: 0 }}
                tabBarExtraContent={
                  scope === 'detail_page' ? (
                    <Button
                      size="small"
                      icon={<ReloadOutlined />}
                      onClick={handleFetchDetailSample}
                      loading={detailFetching}
                      style={{ marginLeft: 8 }}
                      title="改完 link_card / url 字段后点此重新抓取详情样本"
                    >
                      重新抓取详情
                    </Button>
                  ) : null
                }
                items={[
                  { key: 'list_page', label: '列表页素材' },
                  {
                    key: 'detail_page',
                    label: (
                      <span>
                        详情页素材
                        {detailUrl && (
                          <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>
                            ({detailUrl.length > 40 ? detailUrl.slice(0, 40) + '…' : detailUrl})
                          </Text>
                        )}
                      </span>
                    ),
                  },
                ]}
              />
            }
          >
            {scope === 'list_page' ? (
              material ? (
                <SourceViewer
                  material={material}
                  onQuickCreate={(preset) => setQuickPreset({ ...preset, scope: 'list_page' })}
                />
              ) : (
                <div style={{ padding: 40, textAlign: 'center' }}>
                  <Text type="secondary">
                    {fetching ? '抓取中...' : '请输入 URL 并点继续'}
                  </Text>
                </div>
              )
            ) : detailFetching ? (
              <div style={{ padding: 40, textAlign: 'center' }}>
                <Spin tip="取详情样本中..." />
              </div>
            ) : detailError ? (
              <div style={{ padding: 16, overflow: 'auto', maxHeight: '100%' }}>
                <Alert
                  type="warning"
                  showIcon
                  message="详情样本获取失败"
                  description={
                    <Space direction="vertical" size={4}>
                      <Text>{detailError}</Text>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        需要先在 list_page 配置一个 url 类字段（如 link_card 下的 url 子字段），
                        系统才能自动取到首条详情链接样本。
                      </Text>
                      <Button size="small" onClick={handleFetchDetailSample}>
                        重试
                      </Button>
                    </Space>
                  }
                />
              </div>
            ) : detailMaterial ? (
              <SourceViewer
                material={detailMaterial}
                onQuickCreate={(preset) => setQuickPreset({ ...preset, scope: 'detail_page' })}
              />
            ) : (
              <div style={{ padding: 40, textAlign: 'center' }}>
                <Text type="secondary">点击「详情页素材」tab 自动取样本</Text>
              </div>
            )}
          </Card>
        </Col>
        <Col span={10} style={{ height: '100%', display: 'flex', flexDirection: 'column', minHeight: 0 }}>
          <Card
            size="small"
            styles={{ body: { padding: 12, flex: 1, minHeight: 0, overflow: 'auto' } }}
            style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
            title={<Text strong>字段树</Text>}
          >
            <FieldTreePanel
              taskId={taskId}
              tree={tree}
              loading={treeLoading}
              error={treeError}
              currentUrl={urlInput}
              detailUrl={detailUrl ?? undefined}
              userAgent={task?.user_agent ?? undefined}
              proxy={task?.proxy ?? undefined}
              quickPreset={quickPreset}
              onPresetConsumed={() => setQuickPreset(null)}
              onRefresh={refreshTree}
            />
          </Card>
        </Col>
      </Row>
    </div>
  )
}

function parseListUrls(raw: unknown): string[] {
  if (Array.isArray(raw)) return raw as string[]
  if (typeof raw === 'string') {
    try {
      const v = JSON.parse(raw)
      if (Array.isArray(v)) return v as string[]
    } catch {
      return raw.split('\n').map((s) => s.trim()).filter(Boolean)
    }
  }
  return []
}
