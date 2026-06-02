import React, { useEffect, useState } from 'react'
import { Card, Steps, Input, Button, message, Typography, Space, Alert, Spin } from 'antd'
import { useNavigate, useSearchParams } from 'react-router-dom'
import apiClient from '../api/client'

const { Text } = Typography

interface ClientInfo {
  id: string
  client_type: string  // 'Client' | 'Bot'
  phone: string
  status: string
}

// step: 0=发送验证码(phone自动), 1=输入验证码, 2=输入两步密码, 3=完成
// bot直接走 bot_token 输入

const ClientAuth: React.FC = () => {
  const [searchParams] = useSearchParams()
  const clientId = searchParams.get('id') || ''
  const navigate = useNavigate()

  const [clientInfo, setClientInfo] = useState<ClientInfo | null>(null)
  const [step, setStep] = useState(0)
  const [value, setValue] = useState('')
  const [loading, setLoading] = useState(false)

  // 加载客户端信息
  useEffect(() => {
    const fetchClient = async () => {
      try {
        const res = await apiClient.get('/clients')
        const list: ClientInfo[] = res.data.data?.list ?? []
        const client = list.find(c => c.id === clientId)
        if (!client) {
          message.error('客户端不存在')
          navigate('/clients')
          return
        }
        setClientInfo(client)

        // 用户账号 + 已有手机号 → 自动发送验证码
        if (client.client_type === 'Client' && client.phone) {
          sendPhoneCode(client.phone)
        }
      } catch {
        message.error('获取客户端信息失败')
        navigate('/clients')
      }
    }
    if (clientId) fetchClient()
  }, [clientId])

  // 自动发送验证码
  const sendPhoneCode = async (phone: string) => {
    setLoading(true)
    try {
      await apiClient.post(`/clients/${clientId}/auth`, { type: 'phone', value: phone })
      setStep(1)
      message.success('验证码已发送，请查收 Telegram')
    } catch (e: any) {
      const msg = e.response?.data?.error || e.message || '发送验证码失败'
      message.error(msg)
      setStep(0) // 回退到手机号输入
    } finally {
      setLoading(false)
    }
  }

  const submitAuth = async () => {
    if (!value) { message.warning('请输入内容'); return }

    const isBot = clientInfo?.client_type === 'Bot'
    const type = isBot ? 'bot_token' : step === 1 ? 'code' : 'password'

    setLoading(true)
    try {
      const res = await apiClient.post(`/clients/${clientId}/auth`, { type, value })
      const status = res.data?.data?.status

      if (isBot) {
        // Bot 认证直接完成
        setStep(3)
        message.success('Bot 认证成功！')
      } else if (step === 1) {
        if (status === 'wait_password') {
          setStep(2)
          message.info('该账号开启了两步验证，请输入密码')
        } else {
          setStep(3)
          message.success('认证成功！')
        }
      } else if (step === 2) {
        setStep(3)
        message.success('认证成功！')
      }
      setValue('')
    } catch (e: any) {
      const msg = e.response?.data?.error || e.message || '认证失败'
      message.error(msg)
    } finally {
      setLoading(false)
    }
  }

  // 还在加载客户端信息
  if (!clientInfo) {
    return (
      <div style={{ maxWidth: 500, margin: '0 auto', textAlign: 'center', padding: 60 }}>
        <Spin size="large" />
      </div>
    )
  }

  const isBot = clientInfo.client_type === 'Bot'

  // Bot 认证：单独的简单流程
  if (isBot) {
    return (
      <div style={{ maxWidth: 500, margin: '0 auto' }}>
        <Card title={`Bot 认证 ${clientId.substring(0, 8)}...`}>
          {step < 3 ? (
            <Space direction="vertical" style={{ width: '100%' }} size="middle">
              <Alert type="info" message="输入 Bot Token 以完成认证" showIcon />
              <Input
                value={value}
                onChange={e => setValue(e.target.value)}
                placeholder="123456:ABC-DEF..."
                onPressEnter={submitAuth}
                size="large"
                autoFocus
              />
              <Button type="primary" onClick={submitAuth} block loading={loading} size="large">
                认证
              </Button>
            </Space>
          ) : (
            <Space direction="vertical" align="center" style={{ width: '100%' }}>
              <Text type="success" style={{ fontSize: 18 }}>&#10003; Bot 认证完成</Text>
              <Button onClick={() => navigate('/clients')}>返回客户端列表</Button>
            </Space>
          )}
        </Card>
      </div>
    )
  }

  // 用户账号认证
  const stepLabels = [
    { title: '发送验证码' },
    { title: '输入验证码' },
    { title: '两步验证' },
    { title: '完成' },
  ]

  const descriptions: Record<number, string> = {
    0: '输入手机号以请求验证码',
    1: `验证码已发送至 ${clientInfo.phone || '你的手机'}，请查收 Telegram`,
    2: '该账号开启了两步验证，请输入密码',
  }

  return (
    <div style={{ maxWidth: 500, margin: '0 auto' }}>
      <Card title={`认证客户端 ${clientId.substring(0, 8)}...`}>
        <Steps current={step} items={stepLabels} style={{ marginBottom: 24 }} />
        {step < 3 ? (
          <Space direction="vertical" style={{ width: '100%' }} size="middle">
            <Alert type="info" message={descriptions[step]} showIcon />
            {step === 0 ? (
              /* 手机号输入（仅当自动发送失败时才显示） */
              <>
                <Input
                  value={value}
                  onChange={e => setValue(e.target.value)}
                  placeholder="+8613800138000"
                  onPressEnter={() => value && sendPhoneCode(value)}
                  size="large"
                  autoFocus
                />
                <Button type="primary" onClick={() => sendPhoneCode(value)} block loading={loading} size="large">
                  发送验证码
                </Button>
              </>
            ) : (
              <>
                <Input
                  value={value}
                  onChange={e => setValue(e.target.value)}
                  placeholder={step === 1 ? '输入验证码' : '两步验证密码'}
                  onPressEnter={submitAuth}
                  size="large"
                  autoFocus
                />
                <Button type="primary" onClick={submitAuth} block loading={loading} size="large">
                  {step === 1 ? '验证' : '提交密码'}
                </Button>
              </>
            )}
          </Space>
        ) : (
          <Space direction="vertical" align="center" style={{ width: '100%' }}>
            <Text type="success" style={{ fontSize: 18 }}>&#10003; 认证完成</Text>
            <Button onClick={() => navigate('/clients')}>返回客户端列表</Button>
          </Space>
        )}
      </Card>
    </div>
  )
}

export default ClientAuth
