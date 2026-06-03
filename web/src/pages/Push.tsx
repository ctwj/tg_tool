import React, { useEffect, useState } from 'react'
import { Button, Table, message, Space, Modal, Form, Input, InputNumber, Switch, Select, Statistic, Card, Row, Col, Tag, Typography, Divider } from 'antd'
import { RocketOutlined, ReloadOutlined, SettingOutlined, BarChartOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import PageHeader from '../components/PageHeader'

const { Text } = Typography

const Push: React.FC = () => {
  // 推送历史
  const [histories, setHistories] = useState<any[]>([])
  const [loading, setLoading] = useState(false)
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)

  // 弹窗
  const [statsOpen, setStatsOpen] = useState(false)
  const [configOpen, setConfigOpen] = useState(false)
  const [configSaving, setConfigSaving] = useState(false)
  const [form] = Form.useForm()

  // 统计
  const [stats, setStats] = useState({ total: 0, success: 0, failed: 0 })

  const fetchHistories = async (p: number = 1) => {
    setLoading(true)
    try {
      const res = await apiClient.get(`/push/histories?page=${p}&page_size=20`)
      const data = res.data.data
      setHistories(data?.list ?? [])
      setTotal(data?.pagination?.total ?? 0)
      setPage(p)
    } catch { message.error('获取推送历史失败') }
    finally { setLoading(false) }
  }

  const fetchStats = async () => {
    try {
      const res = await apiClient.get('/push/stats')
      setStats(res.data.data ?? { total: 0, success: 0, failed: 0 })
    } catch { /* ignore */ }
  }

  const fetchConfig = async () => {
    try {
      const res = await apiClient.get('/options')
      const data = res.data.data ?? {}
      form.setFieldsValue({
        api_url: data.push_api_url || '',
        api_token: data.push_api_token || '',
        target: data.push_target || '',
        batch_size: parseInt(data.push_batch_size) || 1000,
        auto_push: data.push_auto_push === '1' || data.push_auto_push === 'true',
        interval: parseInt(data.push_interval) || 30,
        extract_mode: data.push_extract_mode || 'rule',
        auto_extract: data.push_auto_extract === '1' || data.push_auto_extract === 'true',
        extract_interval: parseInt(data.push_extract_interval) || 30,
        ai_endpoints: data.push_ai_endpoints || '',
        ai_prompt: data.push_ai_prompt || '',
      })
    } catch { /* ignore */ }
  }

  useEffect(() => { fetchHistories(1); fetchStats() }, [])

  // 手动推送
  const triggerPush = async () => {
    try {
      const checkRes = await apiClient.get('/push/config-check')
      if (checkRes.data?.success) {
        const { is_valid, missing } = checkRes.data.data || {}
        if (!is_valid) {
          const missingLabels: Record<string, string> = {
            push_api_url: '推送 API 地址',
            push_api_token: 'API Token',
            push_target: '推送目标',
          }
          const items = (missing || []).map((k: string) => missingLabels[k] || k)
          Modal.warning({
            title: '推送配置不完整',
            content: (
              <div>
                <p>请先在推送配置中补充以下项：</p>
                <ul>{items.map((item: string) => <li key={item}>{item}</li>)}</ul>
              </div>
            ),
          })
          return
        }
      }

      const res = await apiClient.post('/push/trigger', {})
      if (res.data?.success) {
        message.success(res.data?.data?.message || `推送完成，处理 ${res.data?.data?.processed_count ?? 0} 条`)
      } else {
        message.warning(res.data?.message || '推送未成功')
      }
      fetchHistories(page); fetchStats()
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '推送失败')
    }
  }

  const retryFailed = async () => {
    try {
      const res = await apiClient.post('/push/retry')
      message.success(res.data?.message || '重试已触发')
      fetchHistories(page); fetchStats()
    } catch (e: any) {
      message.error(e.message || '重试失败')
    }
  }

  const openConfig = () => {
    fetchConfig()
    setConfigOpen(true)
  }

  const saveConfig = async (values: any) => {
    setConfigSaving(true)
    try {
      await apiClient.put('/push/scheduler', {
        api_url: values.api_url || '',
        api_token: values.api_token || '',
        target: values.target || '',
        batch_size: values.batch_size || 1000,
        auto_push: values.auto_push ? '1' : '0',
        interval: values.interval || 30,
      })

      await apiClient.put('/push/extract-config', {
        extract_mode: values.extract_mode || 'rule',
        auto_extract: values.auto_extract ? '1' : '0',
        extract_interval: String(values.extract_interval || 30),
        ai_endpoints: values.ai_endpoints || '',
        ai_prompt: values.ai_prompt || '',
      })

      message.success('配置已保存')
      setConfigOpen(false)
    } catch (e: any) {
      message.error(e.response?.data?.error || e.message || '保存失败')
    } finally {
      setConfigSaving(false)
    }
  }

  const openStats = async () => {
    await fetchStats()
    setStatsOpen(true)
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '批次ID', dataIndex: 'batch_id', key: 'batch_id', width: 180, ellipsis: true },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === 'success'
        ? <Tag color="green" style={{ margin: 0 }}>成功</Tag>
        : <Tag color="red" style={{ margin: 0 }}>失败</Tag>,
    },
    { title: '数据量', dataIndex: 'data_count', key: 'data_count', width: 80 },
    { title: '消息', dataIndex: 'message', key: 'message', ellipsis: true },
    {
      title: '错误信息', dataIndex: 'error_msg', key: 'error_msg', ellipsis: true,
      render: (v: string) => v ? <Text type="danger">{v}</Text> : '-',
    },
    {
      title: '推送时间', dataIndex: 'pushed_at', key: 'pushed_at', width: 170,
      render: (v: string) => v ? new Date(v + 'Z').toLocaleString('zh-CN') : '-',
    },
  ]

  return (
    <div>
      <PageHeader
        title="推送管理"
        description="管理消息推送和调度配置"
        extra={
          <Space>
            <Button icon={<BarChartOutlined />} onClick={openStats}>推送统计</Button>
            <Button icon={<SettingOutlined />} onClick={openConfig}>推送配置</Button>
          </Space>
        }
      />

      {/* 操作栏 */}
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<RocketOutlined />} onClick={triggerPush}>手动推送</Button>
        <Button icon={<ReloadOutlined />} onClick={retryFailed}>重试失败</Button>
        <Button onClick={() => { fetchHistories(page); fetchStats() }}>刷新</Button>
      </Space>

      {/* 推送历史表格 */}
      <Table
        dataSource={histories}
        columns={columns}
        rowKey="id"
        loading={loading}
        pagination={{
          current: page,
          total,
          pageSize: 20,
          onChange: (p) => fetchHistories(p),
          showTotal: (t) => `共 ${t} 条`,
          size: 'small',
        }}
        style={{ background: '#fff', borderRadius: 12 }}
      />

      {/* 推送统计弹窗 */}
      <Modal
        title="推送统计"
        open={statsOpen}
        onCancel={() => setStatsOpen(false)}
        footer={<Button onClick={() => setStatsOpen(false)}>关闭</Button>}
        width={520}
      >
        <Row gutter={16}>
          <Col span={8}>
            <Card><Statistic title="总推送" value={stats.total} prefix={<RocketOutlined />} /></Card>
          </Col>
          <Col span={8}>
            <Card><Statistic title="成功" value={stats.success} valueStyle={{ color: '#3f8600' }} /></Card>
          </Col>
          <Col span={8}>
            <Card><Statistic title="失败" value={stats.failed} valueStyle={{ color: '#cf1322' }} /></Card>
          </Col>
        </Row>
      </Modal>

      {/* 推送配置弹窗 */}
      <Modal
        title="推送配置"
        open={configOpen}
        onCancel={() => setConfigOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={configSaving}
        okText="保存全部配置"
        width={700}
      >
        <Form form={form} onFinish={saveConfig} layout="vertical">
          <Divider orientation="left">基本推送配置</Divider>
          <Form.Item name="api_url" label="推送 API 地址"
            rules={[{ required: true, message: '请填写推送 API 地址' }]}
            extra="接收推送数据的外部 API 地址，POST JSON 格式">
            <Input placeholder="https://your-api.com/push" />
          </Form.Item>
          <Form.Item name="api_token" label="API Token"
            extra="作为 X-API-Token 请求头发送，用于接口认证">
            <Input.Password placeholder="your-api-token" />
          </Form.Item>
          <Row gutter={16}>
            <Col span={12}>
              <Form.Item name="target" label="推送目标标识"
                extra="标记推送来源，如 external_api">
                <Input placeholder="external_api" />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item name="batch_size" label="每批推送数量"
                extra="单次推送处理的最大消息数">
                <InputNumber min={1} max={10000} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
          </Row>
          <Form.Item name="auto_push" label="自动定时推送" valuePropName="checked"
            extra="开启后按设定间隔自动推送未推送的采集数据">
            <Switch checkedChildren="开" unCheckedChildren="关" />
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.auto_push !== cur.auto_push}>
            {({ getFieldValue }) => getFieldValue('auto_push') ? (
              <Form.Item name="interval" label="推送间隔（分钟）"
                extra="每隔多少分钟自动推送一次，最小 1 分钟">
                <InputNumber min={1} max={1440} style={{ width: 200 }} />
              </Form.Item>
            ) : null}
          </Form.Item>

          <Divider orientation="left">资源提取配置</Divider>
          <Row gutter={16}>
            <Col span={12}>
              <Form.Item name="extract_mode" label="提取模式"
                extra="规则模式使用内置正则，AI 模式调用大模型增强提取">
                <Select placeholder="选择提取模式" options={[
                  { label: '规则提取（推荐）', value: 'rule' },
                  { label: 'AI 增强', value: 'ai' },
                ]} />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item name="auto_extract" label="自动定时提取" valuePropName="checked"
                extra="开启后按间隔自动扫描并提取未处理的采集记录">
                <Switch checkedChildren="开" unCheckedChildren="关" />
              </Form.Item>
            </Col>
          </Row>
          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.auto_extract !== cur.auto_extract}>
            {({ getFieldValue }) => getFieldValue('auto_extract') ? (
              <Form.Item name="extract_interval" label="提取间隔（分钟）"
                extra="每隔多少分钟自动提取一次">
                <InputNumber min={1} max={1440} style={{ width: 200 }} />
              </Form.Item>
            ) : null}
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, cur) => prev.extract_mode !== cur.extract_mode}>
            {({ getFieldValue }) => getFieldValue('extract_mode') === 'ai' ? (
              <>
                <Form.Item name="ai_endpoints" label="AI API 端点列表"
                  extra='OpenAI 兼容格式，JSON 数组。如：[{"url":"https://api.openai.com","key":"sk-xxx","model":"gpt-4o"}]'>
                  <Input.TextArea rows={3} placeholder='[{"url":"https://api.openai.com","key":"sk-xxx","model":"gpt-4o"}]' />
                </Form.Item>
                <Form.Item name="ai_prompt" label="AI 提示词模板（可选）"
                  extra="留空使用默认提示词">
                  <Input.TextArea rows={2} placeholder="从以下 Telegram 消息中提取结构化资源信息..." />
                </Form.Item>
              </>
            ) : null}
          </Form.Item>
        </Form>
      </Modal>
    </div>
  )
}

export default Push
