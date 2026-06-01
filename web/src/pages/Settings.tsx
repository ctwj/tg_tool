import React, { useEffect, useState } from 'react'
import { Card, Form, Input, Button, message } from 'antd'
import apiClient from '../api/client'

const Settings: React.FC = () => {
  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)

  const fetchOptions = async () => {
    try {
      const res = await apiClient.get('/options')
      if (res.data.data) form.setFieldsValue(res.data.data)
    } catch {}
  }

  useEffect(() => { fetchOptions() }, [form])

  const saveOptions = async (values: Record<string, string>) => {
    setLoading(true)
    try {
      await apiClient.put('/options', values)
      message.success('设置已保存')
    } catch { message.error('保存失败') }
    finally { setLoading(false) }
  }

  return (
    <div>
      <h2>系统设置</h2>
      <Card>
        <Form form={form} onFinish={saveOptions} layout="vertical">
          <Form.Item name="proxy_url" label="代理地址">
            <Input placeholder="socks5://127.0.0.1:1080" />
          </Form.Item>
          <Form.Item name="tg_app_id" label="Telegram APP ID">
            <Input placeholder="从 my.telegram.org 获取" />
          </Form.Item>
          <Form.Item name="tg_app_hash" label="Telegram APP Hash">
            <Input placeholder="从 my.telegram.org 获取" />
          </Form.Item>
          <Form.Item name="image_group" label="图床群组 ID">
            <Input placeholder="-100xxxxxxxxxx" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" loading={loading}>保存</Button>
          </Form.Item>
        </Form>
      </Card>
    </div>
  )
}

export default Settings
