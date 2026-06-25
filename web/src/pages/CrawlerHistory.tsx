import React, { useEffect, useState, useMemo, useCallback } from 'react'
import {
  Table, Button, Select, Space, message, Tag, Tooltip,
  Typography, Card, Row, Col, Statistic, Progress, Spin,
} from 'antd'
import {
  ReloadOutlined, CheckCircleOutlined, CloseCircleOutlined,
  WarningOutlined, StopOutlined, ClockCircleOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import { useNavigate } from 'react-router-dom'
import PageHeader from '../components/PageHeader'
import { useTableScrollY } from '../hooks/useTableScroll'
import * as crawlerApi from '../api/crawler'
import type {
  CrawlerRunHistory, CrawlerHistoryStats, CrawlerTask,
} from '../types'

const { Text, Paragraph } = Typography

// ─── 状态元信息 ──────────────────────────────────────────────────────────
const STATUS_META: Record<string, { color: string; icon: React.ReactNode; label: string }> = {
  success: { color: 'success', icon: <CheckCircleOutlined />, label: '成功' },
  partial: { color: 'warning', icon: <WarningOutlined />, label: '部分成功' },
  failed: { color: 'error', icon: <CloseCircleOutlined />, label: '失败' },
  blocked: { color: 'error', icon: <StopOutlined />, label: '被拦截' },
}

// ─── BlockType 中文标签（与后端 block_detector 对齐） ────────────────────
const BLOCK_TYPE_LABEL: Record<string, string> = {
  HttpBlocked: 'HTTP 拦截 (403/429/503)',
  Cloudflare: 'Cloudflare 防火墙',
  LoginWall: '登录墙',
  Captcha: '验证码',
  EmptyResponse: '空响应',
}

// ─── 主组件 ──────────────────────────────────────────────────────────────
const CrawlerHistory: React.FC = () => {
  const navigate = useNavigate()
  const [list, setList] = useState<CrawlerRunHistory[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(false)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(20)
  const [taskIdFilter, setTaskIdFilter] = useState<number | undefined>(undefined)
  const [statusFilter, setStatusFilter] = useState<string | undefined>(undefined)
  const [dateRange] = useState<[string, string] | null>(null)

  const [stats, setStats] = useState<CrawlerHistoryStats | null>(null)
  const [statsLoading, setStatsLoading] = useState(false)
  const [tasks, setTasks] = useState<CrawlerTask[]>([])

  const { containerRef: tableContainerRef, scrollY: tableScrollY } = useTableScrollY()

  // 加载任务列表（筛选用）
  useEffect(() => {
    (async () => {
      try {
        const res = await crawlerApi.listTasks({ page: 1, page_size: 200 })
        setTasks(res.data?.list ?? [])
      } catch { /* ignore */ }
    })()
  }, [])

  // 拉取历史列表
  const fetchList = useCallback(async () => {
    setLoading(true)
    try {
      const res = await crawlerApi.listHistories({
        page, page_size: pageSize,
        task_id: taskIdFilter,
        status: statusFilter,
        started_after: dateRange?.[0],
        started_before: dateRange?.[1],
      })
      setList(res.data?.list ?? [])
      setTotal(res.data?.pagination?.total ?? 0)
    } catch (e: any) {
      message.error('获取历史失败: ' + (e?.message ?? ''))
    } finally {
      setLoading(false)
    }
  }, [page, pageSize, taskIdFilter, statusFilter, dateRange])

  const fetchStats = useCallback(async () => {
    setStatsLoading(true)
    try {
      const res = await crawlerApi.getHistoryStats({ days: 7 })
      setStats(res.data ?? null)
    } catch (e: any) {
      // 静默
    } finally {
      setStatsLoading(false)
    }
  }, [])

  useEffect(() => { fetchList() }, [fetchList])
  useEffect(() => { fetchStats() }, [fetchStats])

  // 成功率
  const successRate = useMemo(() => {
    if (!stats || stats.total_runs === 0) return 0
    return Math.round(((stats.success + stats.partial * 0.5) / stats.total_runs) * 100)
  }, [stats])

  // block_breakdown 柱状数据
  const blockBars = useMemo(() => {
    if (!stats) return []
    const entries = Object.entries(stats.block_breakdown)
    const max = Math.max(1, ...entries.map(([, v]) => v))
    return entries
      .sort((a, b) => b[1] - a[1])
      .map(([k, v]) => ({
        label: BLOCK_TYPE_LABEL[k] ?? k,
        count: v,
        percent: Math.round((v / max) * 100),
      }))
  }, [stats])

  const columns = [
    {
      title: '任务名', dataIndex: 'task_name', key: 'task_name', width: 180, ellipsis: true,
      render: (n: string, r: CrawlerRunHistory) => (
        <Tooltip title={`task_id=${r.task_id}`}>
          <Text style={{ fontWeight: 500 }}>{n}</Text>
        </Tooltip>
      ),
    },
    {
      title: '状态', dataIndex: 'status', width: 110, key: 'status',
      render: (s: string) => {
        const m = STATUS_META[s] ?? STATUS_META.failed
        return <Tag color={m.color} icon={m.icon}>{m.label}</Tag>
      },
    },
    {
      title: '开始时间', dataIndex: 'started_at', width: 150, key: 'started_at',
      render: (t: string) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {dayjs(t).format('MM-DD HH:mm:ss')}
        </Text>
      ),
    },
    {
      title: '耗时', dataIndex: 'duration_ms', width: 90, key: 'duration_ms',
      render: (ms: number | null) => {
        if (ms == null) return <Text type="secondary">-</Text>
        if (ms < 1000) return <Text>{ms}ms</Text>
        if (ms < 60_000) return <Text>{(ms / 1000).toFixed(1)}s</Text>
        return <Text>{(ms / 60_000).toFixed(1)}min</Text>
      },
    },
    {
      title: '抓取', dataIndex: 'crawled_count', width: 70, key: 'crawled',
      align: 'center' as const,
      render: (n: number) => <Text>{n}</Text>,
    },
    {
      title: '新增', dataIndex: 'new_count', width: 70, key: 'new',
      align: 'center' as const,
      render: (n: number) => <Text type="success" strong>{n}</Text>,
    },
    {
      title: '跳过', dataIndex: 'skipped_count', width: 70, key: 'skipped',
      align: 'center' as const,
      render: (n: number) => <Text type="secondary">{n}</Text>,
    },
    {
      title: '失败', dataIndex: 'failed_count', width: 70, key: 'failed',
      align: 'center' as const,
      render: (n: number) => n > 0 ? <Text type="danger" strong>{n}</Text> : <Text type="secondary">0</Text>,
    },
    {
      title: '拦截类型', dataIndex: 'block_type', width: 160, key: 'block_type',
      render: (b: string | null) => b ? (
        <Tag color="error">{BLOCK_TYPE_LABEL[b] ?? b}</Tag>
      ) : <Text type="secondary">-</Text>,
    },
    {
      title: '错误摘要', dataIndex: 'error_message', key: 'error', ellipsis: true,
      render: (msg: string | null) => msg ? (
        <Tooltip title={msg}>
          <Text type="danger" style={{ fontSize: 12 }}>{msg}</Text>
        </Tooltip>
      ) : <Text type="secondary">-</Text>,
    },
  ]

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      <PageHeader
        title="爬虫历史"
        description="任务运行历史、拦截告警与成功率统计"
        extra={
          <Button icon={<ReloadOutlined />} onClick={() => { fetchList(); fetchStats() }}>
            刷新
          </Button>
        }
      />

      {/* 顶部统计卡片 */}
      <Spin spinning={statsLoading}>
        <Row gutter={12}>
          <Col xs={12} sm={8} md={6} lg={4}>
            <Card size="small">
              <Statistic
                title="最近 7 天运行"
                value={stats?.total_runs ?? 0}
                prefix={<ClockCircleOutlined />}
              />
            </Card>
          </Col>
          <Col xs={12} sm={8} md={6} lg={4}>
            <Card size="small">
              <Statistic
                title="成功率"
                value={successRate}
                suffix="%"
                valueStyle={{ color: successRate >= 80 ? '#10b981' : successRate >= 50 ? '#f59e0b' : '#ef4444' }}
              />
              <Progress percent={successRate} showInfo={false} size="small"
                strokeColor={successRate >= 80 ? '#10b981' : successRate >= 50 ? '#f59e0b' : '#ef4444'} />
            </Card>
          </Col>
          <Col xs={12} sm={8} md={6} lg={4}>
            <Card size="small">
              <Statistic
                title="成功"
                value={stats?.success ?? 0}
                valueStyle={{ color: '#10b981' }}
                prefix={<CheckCircleOutlined />}
              />
            </Card>
          </Col>
          <Col xs={12} sm={8} md={6} lg={4}>
            <Card size="small">
              <Statistic
                title="失败"
                value={(stats?.failed ?? 0) + (stats?.blocked ?? 0)}
                valueStyle={{ color: '#ef4444' }}
                prefix={<CloseCircleOutlined />}
              />
            </Card>
          </Col>
          <Col xs={12} sm={8} md={6} lg={4}>
            <Card
              size="small"
              style={{
                border: (stats?.auto_blocked_tasks ?? 0) > 0 ? '2px solid #ef4444' : undefined,
                cursor: (stats?.auto_blocked_tasks ?? 0) > 0 ? 'pointer' : 'default',
              }}
              onClick={() => {
                if ((stats?.auto_blocked_tasks ?? 0) > 0) navigate('/crawler/tasks')
              }}
              hoverable={(stats?.auto_blocked_tasks ?? 0) > 0}
            >
              <Statistic
                title="自动停用任务"
                value={stats?.auto_blocked_tasks ?? 0}
                valueStyle={{
                  color: (stats?.auto_blocked_tasks ?? 0) > 0 ? '#ef4444' : '#9ca3af',
                }}
                prefix={<StopOutlined />}
              />
              {(stats?.auto_blocked_tasks ?? 0) > 0 && (
                <Text type="danger" style={{ fontSize: 11 }}>点击查看 →</Text>
              )}
            </Card>
          </Col>
          <Col xs={24} sm={12} md={12} lg={4}>
            <Card size="small" title="拦截类型分布" styles={{ header: { minHeight: 36, padding: '6px 12px' } }}>
              {blockBars.length === 0 ? (
                <Text type="secondary" style={{ fontSize: 12 }}>无拦截记录</Text>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                  {blockBars.slice(0, 4).map(b => (
                    <div key={b.label} style={{ fontSize: 11 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                        <Text type="secondary">{b.label}</Text>
                        <Text strong>{b.count}</Text>
                      </div>
                      <Progress percent={b.percent} showInfo={false} size="small" strokeColor="#ef4444" />
                    </div>
                  ))}
                </div>
              )}
            </Card>
          </Col>
        </Row>
      </Spin>

      {/* 筛选栏 */}
      <Space wrap style={{ flexShrink: 0 }}>
        <Select
          placeholder="按任务筛选"
          allowClear
          style={{ width: 200 }}
          value={taskIdFilter}
          onChange={v => { setTaskIdFilter(v); setPage(1) }}
          options={tasks.map(t => ({ label: t.name, value: t.id }))}
        />
        <Select
          placeholder="按状态筛选"
          allowClear
          style={{ width: 140 }}
          value={statusFilter}
          onChange={v => { setStatusFilter(v); setPage(1) }}
          options={[
            { label: '成功', value: 'success' },
            { label: '部分成功', value: 'partial' },
            { label: '失败', value: 'failed' },
            { label: '被拦截', value: 'blocked' },
          ]}
        />
      </Space>

      {/* 表格 */}
      <div ref={tableContainerRef} style={{ flex: 1, minHeight: 0 }}>
        <Table
          rowKey="id"
          dataSource={list}
          columns={columns as any}
          loading={loading}
          scroll={{ x: 1100, y: tableScrollY }}
          size="middle"
          pagination={{
            current: page,
            pageSize,
            total,
            showSizeChanger: true,
            showTotal: t => `共 ${t} 条`,
            onChange: (p, ps) => { setPage(p); setPageSize(ps) },
          }}
          expandable={{
            rowExpandable: r => !!r.error_message || !!r.block_type,
            expandedRowRender: r => (
              <div style={{ padding: '8px 0' }}>
                {r.block_type && (
                  <Paragraph style={{ marginBottom: 8 }}>
                    <Text strong>拦截类型: </Text>
                    <Tag color="error">{BLOCK_TYPE_LABEL[r.block_type] ?? r.block_type}</Tag>
                  </Paragraph>
                )}
                {r.error_message && (
                  <Paragraph style={{ marginBottom: 0 }}>
                    <Text strong>错误详情:</Text>
                    <pre style={{
                      background: '#f9fafb', padding: 8, borderRadius: 4,
                      fontSize: 12, whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                      margin: '4px 0 0',
                    }}>
                      {r.error_message}
                    </pre>
                  </Paragraph>
                )}
              </div>
            ),
          }}
        />
      </div>
    </div>
  )
}

export default CrawlerHistory
