import React, { useEffect, useState } from 'react'
import { Card, Form, Input, Button, message, Space, Alert } from 'antd'
import { CheckCircleOutlined, CloseCircleOutlined, ThunderboltOutlined } from '@ant-design/icons'
import apiClient from '../api/client'

interface ProxyTestResult {
  success: boolean
  message: string
  latency_ms?: number
}

const Settings: React.FC = () => {
  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)
  const [testLoading, setTestLoading] = useState(false)
  const [testResult, setTestResult] = useState<ProxyTestResult | null>(null)
  const [envDefaults, setEnvDefaults] = useState<Record<string, string>>({})

  const fetchOptions = async () => {
    try {
      const res = await apiClient.get('/options')
      if (res.data.data) form.setFieldsValue(res.data.data)
      if (res.data.env_defaults) setEnvDefaults(res.data.env_defaults)
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    fetchOptions()
  }, [form])

  const saveOptions = async (values: Record<string, string>) => {
    setLoading(true)
    try {
      await apiClient.put('/options', values)
      message.success('设置已保存')
      setTestResult(null) // 清空上次测试结果
    } catch {
      message.error('保存失败')
    } finally {
      setLoading(false)
    }
  }

  const testProxy = async () => {
    setTestLoading(true)
    setTestResult(null)
    try {
      // 先保存当前表单值
      const values = form.getFieldsValue()
      await apiClient.put('/options', values)

      const res = await apiClient.post('/options/test-proxy')
      setTestResult(res.data)
      if (res.data.success) {
        message.success(res.data.message)
      } else {
        message.warning(res.data.message)
      }
    } catch (e: any) {
      const msg = e.response?.data?.message || e.message || '测试失败'
      setTestResult({ success: false, message: msg })
      message.error(msg)
    } finally {
      setTestLoading(false)
    }
  }

  const getPlaceholder = (key: string) => {
    const val = envDefaults[key]
    if (!val) return undefined
    return `当前使用环境变量: ${val}`
  }

  return (
    <div>
      <h2>系统设置</h2>

      <Card style={{ marginBottom: 16 }}>
        <Alert
          message="配置优先级说明"
          description="系统配置页面填写的值优先于环境变量（.env 文件）中的配置。留空则使用环境变量中的值。"
          type="info"
          showIcon
          style={{ marginBottom: 24 }}
        />

        <Form form={form} onFinish={saveOptions} layout="vertical">
          {/* 代理配置 */}
          <Form.Item
            name="proxy_url"
            label="代理地址"
            help="支持 HTTP/SOCKS5 代理，如 socks5://127.0.0.1:1080 或 http://proxy:8080"
          >
            <Input placeholder={getPlaceholder('proxy_url') || 'socks5://127.0.0.1:1080'} />
          </Form.Item>

          {testResult && (
            <div style={{ marginBottom: 16 }}>
              {testResult.success ? (
                <Alert
                  message={
                    <Space>
                      <CheckCircleOutlined style={{ color: '#52c41a' }} />
                      <span>{testResult.message}</span>
                    </Space>
                  }
                  type="success"
                  showIcon={false}
                />
              ) : (
                <Alert
                  message={
                    <Space>
                      <CloseCircleOutlined style={{ color: '#ff4d4f' }} />
                      <span>{testResult.message}</span>
                    </Space>
                  }
                  type="error"
                  showIcon={false}
                />
              )}
            </div>
          )}

          <Form.Item>
            <Space>
              <Button type="primary" htmlType="submit" loading={loading}>
                保存设置
              </Button>
              <Button
                icon={<ThunderboltOutlined />}
                onClick={testProxy}
                loading={testLoading}
              >
                测试代理连接
              </Button>
            </Space>
          </Form.Item>

          {/* Telegram 配置 */}
          <Card
            title="Telegram 配置"
            size="small"
            style={{ marginBottom: 16 }}
            type="inner"
          >
            <Form.Item
              name="tg_app_id"
              label="Telegram APP ID"
              help="留空则使用环境变量中的值，填写后将覆盖环境变量"
            >
              <Input placeholder={getPlaceholder('tg_app_id') || '从 my.telegram.org 获取'} />
            </Form.Item>
            <Form.Item
              name="tg_app_hash"
              label="Telegram APP Hash"
              help="留空则使用环境变量中的值，填写后将覆盖环境变量"
            >
              <Input placeholder={getPlaceholder('tg_app_hash') || '从 my.telegram.org 获取'} />
            </Form.Item>
          </Card>

          {/* 其他配置 */}
          <Card title="其他配置" size="small" type="inner">
            <Form.Item name="image_group" label="图床群组 ID">
              <Input placeholder="-100xxxxxxxxxx" />
            </Form.Item>
          </Card>
        </Form>
      </Card>
    </div>
  )
}

export default Settings
