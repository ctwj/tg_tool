import React, { useEffect, useState, useRef } from 'react'
import { Card, Row, Col, Tag, Table, Statistic, Pagination, Empty, Tooltip, Space, Button, Progress, Badge, Tabs, message } from 'antd'
import {
  ClockCircleOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
  CloudSyncOutlined,
  FieldTimeOutlined,
} from '@ant-design/icons'
import apiClient from '../api/client'
import type {
  SchedulersStatus,
  PushHistory,
  ExtractHistory,
  ExtractHistoryStats,
} from '../types'
import PageHeader from '../components/PageHeader'

interface PushStats {
  total: number
  success: number
  failed: number
}

const pushPageSize = 20
const extractPageSize = 20

// 运行中状态的呼吸动画（pulse）— 让"运行中"有活感
const pulseStyle = `
@keyframes sched-pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.55; transform: scale(0.85); }
}
.sched-status-dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #10b981;
  margin-right: 5px;
  animation: sched-pulse 1.8s ease-in-out infinite;
}
.sched-card-hover {
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}
.sched-card-hover:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(14, 165, 233, 0.12);
}
`

const Scheduler: React.FC = () => {
  // 调度状态
  const [schedulers, setSchedulers] = useState<SchedulersStatus | null>(null)
  const [statusLoading, setStatusLoading] = useState(false)

  // 推送历史
  const [pushStats, setPushStats] = useState<PushStats | null>(null)
  const [pushHistories, setPushHistories] = useState<PushHistory[]>([])
  const [pushPage, setPushPage] = useState(1)
  const [pushTotal, setPushTotal] = useState(0)
  const [pushLoading, setPushLoading] = useState(false)

  // 提取历史
  const [extractStats, setExtractStats] = useState<ExtractHistoryStats | null>(null)
  const [extractHistories, setExtractHistories] = useState<ExtractHistory[]>([])
  const [extractPage, setExtractPage] = useState(1)
  const [extractTotal, setExtractTotal] = useState(0)
  const [extractLoading, setExtractLoading] = useState(false)

  // 倒计时（每秒刷新一次）
  const [now, setNow] = useState(Date.now())
  const timerRef = useRef<ReturnType<typeof setInterval>>()

  // 获取调度状态（每 30 秒轮询）
  const fetchStatus = async () => {
    setStatusLoading(true)
    try {
      const res = await apiClient.get('/status')
      if (res.data?.success) {
        setSchedulers(res.data.data?.schedulers)
      }
    } catch {
      /* ignore */
    } finally {
      setStatusLoading(false)
    }
  }

  // 获取推送统计 + 历史
  const fetchPushData = async (page: number) => {
    setPushLoading(true)
    try {
      const [statsRes, histRes] = await Promise.all([
        apiClient.get('/push/stats'),
        apiClient.get('/push/histories', { params: { page, page_size: pushPageSize } }),
      ])
      if (statsRes.data?.success) setPushStats(statsRes.data.data)
      if (histRes.data?.success) {
        setPushHistories(histRes.data.data?.list || [])
        setPushTotal(histRes.data.data?.pagination?.total || 0)
      }
    } catch {
      /* ignore */
    } finally {
      setPushLoading(false)
    }
  }

  // 获取提取统计 + 历史
  const fetchExtractData = async (page: number) => {
    setExtractLoading(true)
    try {
      const [statsRes, histRes] = await Promise.all([
        apiClient.get('/extract-histories/stats'),
        apiClient.get('/extract-histories', { params: { page, page_size: extractPageSize } }),
      ])
      if (statsRes.data?.success) setExtractStats(statsRes.data.data)
      if (histRes.data?.success) {
        setExtractHistories(histRes.data.data?.list || [])
        setExtractTotal(histRes.data.data?.pagination?.total || 0)
      }
    } catch {
      /* ignore */
    } finally {
      setExtractLoading(false)
    }
  }

  useEffect(() => {
    fetchStatus()
    fetchPushData(1)
    fetchExtractData(1)
    // 调度状态每 30 秒轮询
    const poll = setInterval(fetchStatus, 30000)
    // 倒计时每秒刷新
    timerRef.current = setInterval(() => setNow(Date.now()), 1000)
    return () => {
      clearInterval(poll)
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [])

  // 计算倒计时
  const calcCountdown = (nextRun?: string): string => {
    if (!nextRun) return '—'
    // 后端返回本地时间字符串 "2026-06-09 10:30:00"，按本地时间解析
    const target = new Date(nextRun.replace(' ', 'T')).getTime()
    const diff = target - now
    if (diff <= 0) return '即将执行'
    const mins = Math.floor(diff / 60000)
    const secs = Math.floor((diff % 60000) / 1000)
    if (mins > 0) return `${mins} 分 ${secs} 秒后`
    return `${secs} 秒后`
  }

  // 计算当前周期的执行进度百分比（用于进度环）
  const calcProgress = (nextRun?: string, intervalMinutes?: number): number => {
    if (!nextRun || !intervalMinutes) return 0
    const target = new Date(nextRun.replace(' ', 'T')).getTime()
    const diff = target - now
    if (diff <= 0) return 100
    const totalMs = intervalMinutes * 60 * 1000
    // 已过时间占比 = 进度
    return Math.min(100, Math.max(0, Math.round(((totalMs - diff) / totalMs) * 100)))
  }

  // 手动刷新所有数据
  const refreshAll = () => {
    fetchStatus()
    fetchPushData(pushPage)
    fetchExtractData(extractPage)
    message.success('已刷新')
  }

  // 成功率颜色（绿/橙/红）
  const rateColor = (rate: number): string => {
    if (rate >= 95) return '#10b981'
    if (rate >= 80) return '#f59e0b'
    return '#ef4444'
  }

  // 推送历史表格列
  const pushColumns = [
    { title: 'ID', dataIndex: 'id', width: 70 },
    { title: '批次', dataIndex: 'batch_id', width: 160, ellipsis: true },
    { title: '目标', dataIndex: 'target', width: 120 },
    {
      title: '状态',
      dataIndex: 'status',
      width: 100,
      render: (s: string) => (
        <Badge
          status={s === 'success' ? 'success' : s === 'pending' ? 'warning' : 'error'}
          text={s === 'success' ? '成功' : s === 'pending' ? '重试中' : '失败'}
        />
      ),
    },
    { title: '数据量', dataIndex: 'data_count', width: 80 },
    {
      title: '消息/错误',
      ellipsis: true,
      render: (_: unknown, r: PushHistory) => r.error_msg || r.message || '-',
    },
    {
      title: '时间',
      dataIndex: 'pushed_at',
      width: 160,
      render: (t: string) => t?.replace('T', ' ').substring(0, 19) || '-',
    },
  ]

  // 提取历史表格列
  const extractColumns = [
    { title: 'ID', dataIndex: 'id', width: 70 },
    {
      title: '状态',
      dataIndex: 'status',
      width: 100,
      render: (s: string) => (
        <Badge status={s === 'success' ? 'success' : 'error'} text={s === 'success' ? '成功' : '失败'} />
      ),
    },
    { title: '扫描', dataIndex: 'total_scanned', width: 80 },
    { title: '提取', dataIndex: 'extracted', width: 80 },
    { title: '跳过', dataIndex: 'skipped', width: 80 },
    { title: '错误', dataIndex: 'errors', width: 80 },
    {
      title: '消息',
      ellipsis: true,
      render: (_: unknown, r: ExtractHistory) => r.message || '-',
    },
    {
      title: '时间',
      dataIndex: 'executed_at',
      width: 160,
      render: (t: string) => t?.replace('T', ' ').substring(0, 19) || '-',
    },
  ]

  const pushRateNum = pushStats && pushStats.total > 0 ? (pushStats.success / pushStats.total) * 100 : 0
  const pushRateStr = pushRateNum > 0 ? pushRateNum.toFixed(1) + '%' : '—'
  const extractRateNum = extractStats && extractStats.total > 0 ? (extractStats.success / extractStats.total) * 100 : 0
  const extractRateStr = extractRateNum > 0 ? extractRateNum.toFixed(1) + '%' : '—'

  return (
    <div style={{ height: '100%', overflowY: 'auto', overflowX: 'hidden' }}>
      <style>{pulseStyle}</style>
      <PageHeader
        title="调度监控"
        description="定时任务运行状态与执行历史"
        extra={
          <Button icon={<ReloadOutlined />} onClick={refreshAll} loading={statusLoading}>
            刷新
          </Button>
        }
      />

      {/* 调度状态卡片 */}
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={24} md={8}>
          <Card
            className="sched-card-hover"
            loading={statusLoading && !schedulers}
            style={{ borderRadius: 12 }}
            title={
              <Space>
                <ThunderboltOutlined style={{ color: '#0369a1' }} />
                <span>推送调度</span>
              </Space>
            }
            extra={
              schedulers?.push_running ? (
                <span style={{ fontSize: 13, color: '#10b981' }}>
                  <span className="sched-status-dot" />运行中
                </span>
              ) : (
                <Tag color="default">已停止</Tag>
              )
            }
          >
            {schedulers && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
                <Progress
                  type="circle"
                  size={64}
                  percent={schedulers.push_running ? calcProgress(schedulers.push_next_run, schedulers.push_interval_minutes) : 0}
                  strokeColor="#0369a1"
                  format={() => <ClockCircleOutlined style={{ fontSize: 18, color: schedulers.push_running ? '#0369a1' : '#d1d5db' }} />}
                />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 12, color: '#9ca3af' }}>
                    间隔 {schedulers.push_interval_minutes} 分钟
                  </div>
                  <div style={{ fontSize: 13, color: '#1f2937', fontWeight: 500, margin: '2px 0' }}>
                    {schedulers.push_running ? calcCountdown(schedulers.push_next_run) : '—'}
                  </div>
                  <div style={{ fontSize: 11, color: '#9ca3af' }}>
                    {schedulers.push_next_run?.substring(11) || '未计划'}
                  </div>
                </div>
              </div>
            )}
          </Card>
        </Col>
        <Col xs={24} sm={24} md={8}>
          <Card
            className="sched-card-hover"
            loading={statusLoading && !schedulers}
            style={{ borderRadius: 12 }}
            title={
              <Space>
                <CloudSyncOutlined style={{ color: '#0ea5e9' }} />
                <span>提取调度</span>
              </Space>
            }
            extra={
              schedulers?.extract_running ? (
                <span style={{ fontSize: 13, color: '#10b981' }}>
                  <span className="sched-status-dot" />运行中
                </span>
              ) : (
                <Tag color="default">已停止</Tag>
              )
            }
          >
            {schedulers && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
                <Progress
                  type="circle"
                  size={64}
                  percent={schedulers.extract_running ? calcProgress(schedulers.extract_next_run, schedulers.extract_interval_minutes) : 0}
                  strokeColor="#0ea5e9"
                  format={() => <ClockCircleOutlined style={{ fontSize: 18, color: schedulers.extract_running ? '#0ea5e9' : '#d1d5db' }} />}
                />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 12, color: '#9ca3af' }}>
                    间隔 {schedulers.extract_interval_minutes} 分钟
                  </div>
                  <div style={{ fontSize: 13, color: '#1f2937', fontWeight: 500, margin: '2px 0' }}>
                    {schedulers.extract_running ? calcCountdown(schedulers.extract_next_run) : '—'}
                  </div>
                  <div style={{ fontSize: 11, color: '#9ca3af' }}>
                    {schedulers.extract_next_run?.substring(11) || '未计划'}
                  </div>
                </div>
              </div>
            )}
          </Card>
        </Col>
        <Col xs={24} sm={24} md={8}>
          <Card
            className="sched-card-hover"
            loading={statusLoading && !schedulers}
            style={{ borderRadius: 12 }}
            title={
              <Space>
                <FieldTimeOutlined style={{ color: '#10b981' }} />
                <span>图片转发</span>
              </Space>
            }
            extra={
              schedulers?.forward_running ? (
                <span style={{ fontSize: 13, color: '#10b981' }}>
                  <span className="sched-status-dot" />运行中
                </span>
              ) : (
                <Tag color="default">未启用</Tag>
              )
            }
          >
            {schedulers && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
                <Progress
                  type="circle"
                  size={64}
                  percent={schedulers.forward_running ? 100 : 0}
                  strokeColor="#10b981"
                  showInfo={false}
                />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 12, color: '#9ca3af' }}>
                    间隔 {schedulers.forward_interval_secs} 秒
                  </div>
                  <div style={{ fontSize: 13, color: '#1f2937', fontWeight: 500, margin: '2px 0' }}>
                    {schedulers.forward_running ? '队列处理中' : '未配置'}
                  </div>
                  {!schedulers.forward_running && (
                    <div style={{ fontSize: 11, color: '#9ca3af' }}>
                      需配置图床 Bot 后启用
                    </div>
                  )}
                </div>
              </div>
            )}
          </Card>
        </Col>
      </Row>

      {/* 统计卡片 */}
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col xs={24} md={12}>
          <Card title="推送统计" size="small" style={{ borderRadius: 12 }}>
            <Row gutter={16} align="middle">
              <Col span={6}>
                <Statistic title="总次数" value={pushStats?.total ?? 0} />
              </Col>
              <Col span={6}>
                <Statistic title="成功" value={pushStats?.success ?? 0} valueStyle={{ color: '#10b981' }} />
              </Col>
              <Col span={6}>
                <Statistic title="失败" value={pushStats?.failed ?? 0} valueStyle={{ color: '#ef4444' }} />
              </Col>
              <Col span={6}>
                <div style={{ fontSize: 12, color: '#6b7280', marginBottom: 6 }}>成功率</div>
                <div style={{ fontSize: 20, fontWeight: 600, color: rateColor(pushRateNum), lineHeight: 1.2 }}>
                  {pushRateStr}
                </div>
                {pushRateNum > 0 && (
                  <Progress
                    percent={pushRateNum}
                    showInfo={false}
                    size="small"
                    strokeColor={rateColor(pushRateNum)}
                    style={{ marginBottom: 0, marginTop: 4 }}
                  />
                )}
              </Col>
            </Row>
          </Card>
        </Col>
        <Col xs={24} md={12}>
          <Card title="提取统计" size="small" style={{ borderRadius: 12 }}>
            <Row gutter={16} align="middle">
              <Col span={6}>
                <Statistic title="总次数" value={extractStats?.total ?? 0} />
              </Col>
              <Col span={6}>
                <Statistic title="成功" value={extractStats?.success ?? 0} valueStyle={{ color: '#10b981' }} />
              </Col>
              <Col span={6}>
                <Statistic title="失败" value={extractStats?.failed ?? 0} valueStyle={{ color: '#ef4444' }} />
              </Col>
              <Col span={6}>
                <Tooltip title={`最近一次成功提取 ${extractStats?.last_extracted ?? 0} 条资源`}>
                  <div style={{ fontSize: 12, color: '#6b7280', marginBottom: 6 }}>成功率</div>
                  <div style={{ fontSize: 20, fontWeight: 600, color: rateColor(extractRateNum), lineHeight: 1.2 }}>
                    {extractRateStr}
                  </div>
                  {extractRateNum > 0 && (
                    <Progress
                      percent={extractRateNum}
                      showInfo={false}
                      size="small"
                      strokeColor={rateColor(extractRateNum)}
                      style={{ marginBottom: 0, marginTop: 4 }}
                    />
                  )}
                </Tooltip>
              </Col>
            </Row>
          </Card>
        </Col>
      </Row>

      {/* 历史记录（Tab 切换，节省空间） */}
      <Card size="small" style={{ borderRadius: 12 }}>
        <Tabs
          defaultActiveKey="push"
          items={[
            {
              key: 'push',
              label: (
                <span>
                  <ThunderboltOutlined /> 推送历史
                  {pushTotal > 0 && <Badge count={pushTotal} overflowCount={999} style={{ marginLeft: 6, backgroundColor: '#0369a1' }} />}
                </span>
              ),
              children: (
                <>
                  <Table
                    dataSource={pushHistories}
                    columns={pushColumns}
                    rowKey="id"
                    loading={pushLoading}
                    pagination={false}
                    size="small"
                    locale={{ emptyText: <Empty description="暂无推送记录" /> }}
                  />
                  {pushTotal > pushPageSize && (
                    <div style={{ textAlign: 'right', marginTop: 12 }}>
                      <Pagination
                        current={pushPage}
                        total={pushTotal}
                        pageSize={pushPageSize}
                        size="small"
                        showTotal={t => `共 ${t} 条`}
                        onChange={p => {
                          setPushPage(p)
                          fetchPushData(p)
                        }}
                      />
                    </div>
                  )}
                </>
              ),
            },
            {
              key: 'extract',
              label: (
                <span>
                  <CloudSyncOutlined /> 提取历史
                  {extractTotal > 0 && <Badge count={extractTotal} overflowCount={999} style={{ marginLeft: 6, backgroundColor: '#0ea5e9' }} />}
                </span>
              ),
              children: (
                <>
                  <Table
                    dataSource={extractHistories}
                    columns={extractColumns}
                    rowKey="id"
                    loading={extractLoading}
                    pagination={false}
                    size="small"
                    locale={{ emptyText: <Empty description="暂无提取记录" /> }}
                  />
                  {extractTotal > extractPageSize && (
                    <div style={{ textAlign: 'right', marginTop: 12 }}>
                      <Pagination
                        current={extractPage}
                        total={extractTotal}
                        pageSize={extractPageSize}
                        size="small"
                        showTotal={t => `共 ${t} 条`}
                        onChange={p => {
                          setExtractPage(p)
                          fetchExtractData(p)
                        }}
                      />
                    </div>
                  )}
                </>
              ),
            },
          ]}
        />
      </Card>
    </div>
  )
}

export default Scheduler
