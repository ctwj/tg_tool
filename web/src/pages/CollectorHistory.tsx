import React, { useEffect, useState } from 'react'
import { Table, Button, Tag, Typography, message, Select, Space, Modal, Spin, Alert, Empty } from 'antd'
import { ArrowLeftOutlined, ExperimentOutlined, ThunderboltOutlined, FileTextOutlined, LinkOutlined, CopyOutlined } from '@ant-design/icons'
import { useParams, useLocation, useNavigate } from 'react-router-dom'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import type { ResourceDraftView } from '../types'

const { Paragraph } = Typography

// 网盘分类颜色映射
const categoryColors: Record<string, string> = {
  quark: '#4C6EF5', aliyun: '#7C3AED', baidu: '#2563EB',
  uc: '#0891B2', '115': '#059669', '123pan': '#D97706',
  tianyi: '#DC2626', xunlei: '#EA580C',
}

const categoryLabels: Record<string, string> = {
  quark: '夸克网盘', aliyun: '阿里云盘', baidu: '百度网盘',
  uc: 'UC网盘', '115': '115网盘', '123pan': '123网盘',
  tianyi: '天翼网盘', xunlei: '迅雷网盘',
}

interface CollectorHistory {
  id: number
  collector_id: number
  channel_id: number
  message_id: number
  post_time: string
  raw_data: string | null
  is_auto_push: boolean
  remote_id: string | null
  created_at: string
  is_extracted: boolean
}

const CollectorHistory: React.FC = () => {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  const navigate = useNavigate()
  const state = location.state as { channel_name?: string; channel_id?: number } | null

  const [data, setData] = useState<CollectorHistory[]>([])
  const [loading, setLoading] = useState(false)
  const [pagination, setPagination] = useState({ page: 1, pageSize: 20, total: 0 })
  const [extractedFilter, setExtractedFilter] = useState<boolean | undefined>(undefined)

  // 资源提取弹窗状态
  const [extractModalOpen, setExtractModalOpen] = useState(false)
  const [extractingRecord, setExtractingRecord] = useState<CollectorHistory | null>(null)
  const [extractMode, setExtractMode] = useState<'rule' | 'ai'>('rule')
  const [extractResults, setExtractResults] = useState<ResourceDraftView[]>([])
  const [extractLoading, setExtractLoading] = useState(false)
  const [extractError, setExtractError] = useState('')
  const [extractSuccess, setExtractSuccess] = useState(false)

  const fetch = async (page = 1) => {
    if (!id) return
    setLoading(true)
    try {
      const params: Record<string, string> = {
        collector_id: id,
        page: String(page),
        page_size: '20',
      }
      if (extractedFilter !== undefined) {
        params.is_extracted = String(extractedFilter)
      }
      const res = await apiClient.get('/collectors/histories', { params })
      setData(res.data.data?.list ?? [])
      setPagination(prev => ({ ...prev, page, total: res.data.data?.pagination?.total ?? 0 }))
    } catch { message.error('获取采集记录失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetch() }, [id, extractedFilter])

  const parseRawData = (raw: string | null): { text: string; mediaType?: string } => {
    if (!raw) return { text: '(无内容)' }
    try {
      const d = JSON.parse(raw)
      return { text: d.text || '(无文本)', mediaType: d.media_type }
    } catch {
      return { text: raw.substring(0, 100) }
    }
  }

  const openExtractModal = async (record: CollectorHistory) => {
    setExtractingRecord(record)
    setExtractResults([])
    setExtractError('')
    setExtractSuccess(false)
    setExtractModalOpen(true)
    try {
      const res = await apiClient.get('/options')
      const mode = res.data.data?.push_extract_mode
      setExtractMode(mode === 'ai' ? 'ai' : 'rule')
    } catch {
      setExtractMode('rule')
    }
  }

  const doExtract = async (dryRun: boolean) => {
    if (!extractingRecord) return
    setExtractLoading(true)
    setExtractError('')
    setExtractSuccess(false)
    try {
      const res = await apiClient.post(`/resources/extract/${extractingRecord.id}`, {
        dry_run: dryRun,
        extract_mode: extractMode,
      }, { timeout: 120000 })
      if (res.data?.success) {
        setExtractResults(res.data.data?.resources ?? [])
        if (!dryRun) {
          setExtractSuccess(true)
          fetch(pagination.page)
          setExtractingRecord(prev => prev ? { ...prev, is_extracted: true } : prev)
        }
      } else {
        setExtractError(res.data?.message || '提取失败')
      }
    } catch (err: any) {
      const msg = err?.response?.data?.message
      setExtractError(msg || '提取过程中发生错误，请稍后重试')
    } finally {
      setExtractLoading(false)
    }
  }

  const columns = [
    { title: '消息ID', dataIndex: 'message_id', key: 'message_id', width: 90 },
    {
      title: '内容',
      key: 'content',
      render: (_: any, r: CollectorHistory) => {
        const parsed = parseRawData(r.raw_data)
        return (
          <div>
            <Paragraph ellipsis={{ rows: 2, expandable: true, symbol: '展开' }} style={{ marginBottom: 0 }}>
              {parsed.text}
            </Paragraph>
            {parsed.mediaType && (
              <Tag color="blue" style={{ marginTop: 4 }}>
                {parsed.mediaType === 'photo' ? '图片' : parsed.mediaType === 'document' ? '文件' : parsed.mediaType}
              </Tag>
            )}
          </div>
        )
      },
    },
    {
      title: '来源', key: 'source', width: 80,
      render: (_: any, r: CollectorHistory) => (
        <Tag color={r.is_auto_push ? 'green' : '#6366f1'} style={{ margin: 0 }}>
          {r.is_auto_push ? '实时' : '手动'}
        </Tag>
      ),
    },
    {
      title: '已提取', key: 'is_extracted', width: 80,
      render: (_: any, r: CollectorHistory) => (
        <Tag color={r.is_extracted ? '#6366f1' : 'default'} style={{ margin: 0 }}>
          {r.is_extracted ? '是' : '否'}
        </Tag>
      ),
    },
    {
      title: '采集时间', dataIndex: 'post_time', key: 'post_time', width: 170,
      render: (v: string) => v ? new Date(v + 'Z').toLocaleString('zh-CN') : '-',
    },
    {
      title: '操作', key: 'action', width: 100, fixed: 'right' as const,
      render: (_: any, r: CollectorHistory) => (
        <Button type="link" size="small" onClick={() => openExtractModal(r)}>
          资源提取
        </Button>
      ),
    },
  ]

  const titleSuffix = state?.channel_name || state?.channel_id || `#${id}`
  const { containerRef, scrollY } = useTableScrollY()

  const rawContent = extractingRecord ? parseRawData(extractingRecord.raw_data) : null
  const hasRawData = !!extractingRecord?.raw_data && extractingRecord.raw_data.trim() !== ''
  const isExtracted = !!extractingRecord?.is_extracted

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <PageHeader
        title={`采集记录 — ${titleSuffix}`}
        description={`共 ${pagination.total} 条记录`}
        extra={
          <Button icon={<ArrowLeftOutlined />} onClick={() => navigate('/collectors')}>
            返回采集器
          </Button>
        }
      />

      <div style={{ flexShrink: 0, marginBottom: 12 }}>
        <Space>
          <span style={{ color: '#6b7280', fontSize: 14 }}>提取状态：</span>
          <Select
            value={extractedFilter}
            onChange={setExtractedFilter}
            style={{ width: 150 }}
            allowClear
            placeholder="全部"
            options={[
              { label: '已提取', value: true },
              { label: '未提取', value: false },
            ]}
          />
        </Space>
      </div>

      <div ref={containerRef} style={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>
        <Table
          dataSource={data}
          columns={columns}
          rowKey="id"
          loading={loading}
          scroll={{ y: scrollY, x: 800 }}
          style={{ background: '#fff', borderRadius: 12 }}
          pagination={{
            current: pagination.page,
            total: pagination.total,
            pageSize: pagination.pageSize,
            onChange: (p) => fetch(p),
            showTotal: (t) => `共 ${t} 条`,
            showSizeChanger: false,
          }}
        />
      </div>

      {/* ────── 资源提取弹窗 ────── */}
      <Modal
        title={null}
        open={extractModalOpen}
        onCancel={() => setExtractModalOpen(false)}
        footer={null}
        width={960}
        destroyOnClose
        styles={{
          body: { padding: 0 },
        }}
      >
        {/* 自定义标题栏 */}
        <div style={{
          padding: '20px 24px 16px',
          borderBottom: '1px solid #f0f0f0',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <div style={{
              width: 36, height: 36, borderRadius: 10,
              background: 'linear-gradient(135deg, #6366f1, #8b5cf6)',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
            }}>
              <FileTextOutlined style={{ color: '#fff', fontSize: 18 }} />
            </div>
            <div>
              <div style={{ fontWeight: 600, fontSize: 16, color: '#1f2937' }}>资源提取</div>
              <div style={{ fontSize: 12, color: '#9ca3af', marginTop: 2 }}>
                消息 #{extractingRecord?.message_id} · {extractingRecord?.post_time ? new Date(extractingRecord.post_time + 'Z').toLocaleString('zh-CN') : ''}
              </div>
            </div>
          </div>
          <Select
            value={extractMode}
            onChange={setExtractMode}
            style={{ width: 140 }}
            options={[
              { label: '规则提取', value: 'rule' },
              { label: 'AI 提取', value: 'ai' },
            ]}
          />
        </div>

        {/* 状态提示条 */}
        {isExtracted && (
          <div style={{ padding: '0 24px', marginTop: 12 }}>
            <Alert type="info" message="该记录已提取过资源，可使用测试模式重新预览" showIcon style={{ borderRadius: 8 }} />
          </div>
        )}
        {extractSuccess && (
          <div style={{ padding: '0 24px', marginTop: 12 }}>
            <Alert type="success" message="资源提取成功，已写入数据库" showIcon style={{ borderRadius: 8 }} />
          </div>
        )}
        {extractError && (
          <div style={{ padding: '0 24px', marginTop: 12 }}>
            <Alert type="error" message={extractError} showIcon closable onClose={() => setExtractError('')} style={{ borderRadius: 8 }} />
          </div>
        )}

        {/* 主内容区：左右分栏 */}
        <div style={{ display: 'flex', gap: 0, padding: '16px 24px 0' }}>
          {/* 左侧：消息内容 */}
          <div style={{ flex: 1, paddingRight: 20, minHeight: 200 }}>
            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              marginBottom: 10,
            }}>
              <span style={{
                fontSize: 12, fontWeight: 600, color: '#6b7280',
                textTransform: 'uppercase', letterSpacing: '0.05em',
              }}>
                消息内容
              </span>
              {extractResults.length > 0 && (
                <Button
                  type="text"
                  size="small"
                  icon={<CopyOutlined />}
                  onClick={() => {
                    const log = [
                      `===== 资源提取日志 =====`,
                      `消息ID: ${extractingRecord?.message_id}`,
                      `提取模式: ${extractMode === 'ai' ? 'AI 提取' : '规则提取'}`,
                      `提取时间: ${new Date().toLocaleString('zh-CN')}`,
                      `提取结果数: ${extractResults.length}`,
                      ``,
                      `----- 原始消息 -----`,
                      rawContent?.text || '(无内容)',
                      ``,
                      `----- 提取结果 -----`,
                      ...extractResults.map((item, i) => [
                        `[资源 ${i + 1}]`,
                        `  标题: ${item.title}`,
                        `  链接: ${item.url?.join('\n        ') || '(无)'}`,
                        `  描述: ${item.description || '(无)'}`,
                        `  分类: ${categoryLabels[item.category || ''] || item.category || '(无)'}`,
                        `  标签: ${item.tags || '(无)'}`,
                        `  来源: ${item.source}`,
                      ].join('\n')),
                    ].join('\n')
                    navigator.clipboard.writeText(log).then(
                      () => message.success('日志已复制到剪贴板'),
                      () => message.error('复制失败'),
                    )
                  }}
                  style={{ fontSize: 12, color: '#6b7280' }}
                >
                  复制日志
                </Button>
              )}
            </div>
            {hasRawData ? (
              <div style={{
                maxHeight: 420, overflowY: 'auto', padding: 16,
                background: '#f9fafb', borderRadius: 10,
                border: '1px solid #f3f4f6',
              }}>
                <Paragraph style={{
                  whiteSpace: 'pre-wrap', wordBreak: 'break-word', fontSize: 13,
                  lineHeight: 1.75, color: '#374151', margin: 0,
                }}>
                  {rawContent?.text}
                </Paragraph>
                {rawContent?.mediaType && (
                  <div style={{ marginTop: 12 }}>
                    <Tag color="blue" style={{ borderRadius: 6 }}>
                      {rawContent.mediaType === 'photo' ? '图片' : rawContent.mediaType === 'document' ? '文件' : rawContent.mediaType}
                    </Tag>
                  </div>
                )}
              </div>
            ) : (
              <div style={{
                padding: 48, textAlign: 'center', color: '#d1d5db',
                background: '#fafafa', borderRadius: 10,
                border: '1px dashed #e5e7eb',
              }}>
                <FileTextOutlined style={{ fontSize: 36, marginBottom: 8 }} />
                <div style={{ fontSize: 13, color: '#9ca3af' }}>无内容可提取</div>
              </div>
            )}
          </div>

          {/* 分隔线 */}
          <div style={{ width: 1, background: '#f0f0f0', margin: '0 4px', alignSelf: 'stretch' }} />

          {/* 右侧：提取结果 */}
          <div style={{ width: 400, paddingLeft: 20, display: 'flex', flexDirection: 'column' }}>
            <div style={{
              fontSize: 12, fontWeight: 600, color: '#6b7280',
              textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 10,
            }}>
              提取结果
              {extractResults.length > 0 && (
                <span style={{
                  marginLeft: 8, fontSize: 11, color: '#fff',
                  background: '#6366f1', borderRadius: 10,
                  padding: '1px 8px', fontWeight: 500,
                }}>
                  {extractResults.length}
                </span>
              )}
            </div>

            {/* 加载态 */}
            {extractLoading ? (
              <div style={{
                flex: 1, display: 'flex', flexDirection: 'column',
                alignItems: 'center', justifyContent: 'center',
                padding: 48, color: '#9ca3af',
              }}>
                <Spin size="large" />
                <div style={{ marginTop: 12, fontSize: 13 }}>
                  {extractMode === 'ai' ? 'AI 正在分析中...' : '正在提取资源...'}
                </div>
              </div>
            ) : extractResults.length > 0 ? (
              /* 结果列表 — 详情卡片 */
              <div style={{ flex: 1, maxHeight: 420, overflowY: 'auto', paddingRight: 4 }}>
                {extractResults.map((item, idx) => (
                  <div key={idx} style={{
                    marginBottom: 12,
                    background: '#fff', borderRadius: 10,
                    border: '1px solid #f3f4f6',
                    boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
                    overflow: 'hidden',
                  }}>
                    {/* 卡片头部：序号 + 分类 */}
                    <div style={{
                      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                      padding: '10px 14px',
                      background: '#f9fafb', borderBottom: '1px solid #f3f4f6',
                    }}>
                      <span style={{ fontSize: 12, fontWeight: 600, color: '#6b7280' }}>
                        资源 {idx + 1}
                      </span>
                      {item.category && (
                        <Tag style={{
                          margin: 0, borderRadius: 6, fontSize: 11,
                          color: '#fff', border: 'none',
                          background: categoryColors[item.category] || '#6b7280',
                        }}>
                          {categoryLabels[item.category] || item.category}
                        </Tag>
                      )}
                    </div>

                    {/* 卡片主体：字段表格 */}
                    <div style={{ padding: '10px 14px 14px' }}>
                      {/* 标题 */}
                      <div style={{ marginBottom: 10 }}>
                        <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 3, fontWeight: 500 }}>
                          标题
                        </div>
                        <div style={{
                          fontWeight: 600, fontSize: 14, color: '#1f2937',
                          lineHeight: 1.5,
                        }}>
                          {item.title}
                        </div>
                      </div>

                      {/* 链接 */}
                      {item.url?.length > 0 && (
                        <div style={{ marginBottom: 10 }}>
                          <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 3, fontWeight: 500 }}>
                            链接
                          </div>
                          {item.url.map((u, i) => (
                            <div key={i} style={{
                              display: 'flex', alignItems: 'flex-start', gap: 6,
                              fontSize: 12, color: '#6366f1',
                              wordBreak: 'break-all', lineHeight: 1.6,
                              background: '#f5f3ff', padding: '4px 8px',
                              borderRadius: 6, marginBottom: 4,
                            }}>
                              <LinkOutlined style={{ fontSize: 11, marginTop: 3, flexShrink: 0 }} />
                              <span style={{ fontFamily: 'monospace' }}>{u}</span>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* 描述 */}
                      {item.description && (
                        <div style={{ marginBottom: 10 }}>
                          <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 3, fontWeight: 500 }}>
                            描述
                          </div>
                          <div style={{
                            fontSize: 13, color: '#4b5563', lineHeight: 1.6,
                            overflow: 'hidden', display: '-webkit-box',
                            WebkitLineClamp: 3, WebkitBoxOrient: 'vertical',
                          }}>
                            {item.description}
                          </div>
                        </div>
                      )}

                      {/* 标签 */}
                      {item.tags && item.tags.split(',').filter(Boolean).length > 0 && (
                        <div>
                          <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 4, fontWeight: 500 }}>
                            标签
                          </div>
                          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                            {item.tags.split(',').filter(Boolean).map((t, i) => (
                              <Tag key={i} style={{
                                margin: 0, borderRadius: 6, fontSize: 11,
                                background: '#f3f4f6', border: 'none', color: '#6b7280',
                              }}>
                                {t.trim()}
                              </Tag>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* 来源 */}
                      <div style={{ marginTop: 10, display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span style={{ fontSize: 11, color: '#9ca3af', fontWeight: 500 }}>来源</span>
                        <Tag style={{
                          margin: 0, borderRadius: 6, fontSize: 11,
                          background: '#ecfdf5', border: 'none', color: '#059669',
                        }}>
                          {item.source === 'tg' ? 'Telegram' : item.source}
                        </Tag>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              /* 空状态 */
              <div style={{
                flex: 1, display: 'flex', flexDirection: 'column',
                alignItems: 'center', justifyContent: 'center',
                padding: 32,
              }}>
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={
                    <span style={{ color: '#d1d5db', fontSize: 13 }}>
                      点击下方按钮开始提取
                    </span>
                  }
                />
              </div>
            )}
          </div>
        </div>

        {/* 底部操作栏 */}
        <div style={{
          padding: '16px 24px 20px',
          borderTop: '1px solid #f0f0f0',
          display: 'flex', justifyContent: 'flex-end', gap: 10,
          marginTop: 16,
        }}>
          <Button
            size="middle"
            icon={<ExperimentOutlined />}
            loading={extractLoading}
            disabled={!hasRawData}
            onClick={() => doExtract(true)}
            style={{ borderRadius: 8, minWidth: 100 }}
          >
            测试
          </Button>
          <Button
            type="primary"
            size="middle"
            icon={<ThunderboltOutlined />}
            loading={extractLoading}
            disabled={!hasRawData || isExtracted}
            onClick={() => doExtract(false)}
            style={{
              borderRadius: 8, minWidth: 100,
              background: isExtracted ? undefined : '#6366f1',
              boxShadow: isExtracted ? undefined : '0 2px 4px rgba(99,102,241,0.3)',
            }}
          >
            {isExtracted ? '已提取' : '提取'}
          </Button>
        </div>
      </Modal>
    </div>
  )
}

export default CollectorHistory
