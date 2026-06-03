import React, { useEffect, useState } from 'react'
import { Card, Steps, Input, Button, message, Typography, Space, Alert, Spin } from 'antd'
import { CheckCircleFilled, SendOutlined } from '@ant-design/icons'
import { useNavigate, useSearchParams } from 'react-router-dom'
import apiClient from '../api/client'

const { Text } = Typography

interface ClientInfo {
  id: string
  client_type: string
  phone: string
  status: string
}

const ClientAuth: React.FC = () => {
  const [searchParams] = useSearchParams()
  const clientId = searchParams.get('id') || ''
  const navigate = useNavigate()

  const [clientInfo, setClientInfo] = useState<ClientInfo | null>(null)
  const [step, setStep] = useState(0)
  const [value, setValue] = useState('')
  const [loading, setLoading] = useState(false)

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

  const sendPhoneCode = async (phone: string) => {
    setLoading(true)
    try {
      await apiClient.post(`/clients/${clientId}/auth`, { type: 'phone', value: phone })
      setStep(1)
      message.success('验证码已发送，请查收 Telegram')
    } catch (e: any) {
      const msg = e.response?.data?.error || e.message || '发送验证码失败'
      message.error(msg)
      setStep(0)
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

  if (!clientInfo) {
    return (
      <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <Spin size="large" />
      </div>
    )
  }

  const isBot = clientInfo.client_type === 'Bot'

  // 成功页面
  const successContent = (
    <div style={{ textAlign: 'center', padding: '20px 0' }}>
      <div style={{
        width: 64,
        height: 64,
        borderRadius: '50%',
        background: '#ecfdf5',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        margin: '0 auto 16px',
      }}>
        <CheckCircleFilled style={{ fontSize: 32, color: '#10b981' }} />
      </div>
      <Text style={{ fontSize: 18, fontWeight: 500, color: '#1e1b4b', display: 'block', marginBottom: 16 }}>
        认证完成
      </Text>
      <Button type="primary" onClick={() => navigate('/clients')}>
        返回客户端列表
      </Button>
    </div>
  )

  // Bot 认证
  if (isBot) {
    return (
      <div style={{ height: '100%', overflowY: 'auto' }}>
        <div style={{ maxWidth: 500, margin: '0 auto' }}>
        <Card
          style={{ borderRadius: 16, boxShadow: '0 2px 8px rgba(0,0,0,0.06)' }}
          styles={{ header: { borderBottom: '1px solid #f0f0f0' } }}
          title={
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <SendOutlined style={{ color: '#6366f1' }} />
              Bot 认证 {clientId.substring(0, 8)}...
            </div>
          }
        >
          {step < 3 ? (
            <Space direction="vertical" style={{ width: '100%' }} size="middle">
              <Alert type="info" message="输入 Bot Token 以完成认证" showIcon style={{ borderRadius: 8 }} />
              <Input
                value={value}
                onChange={e => setValue(e.target.value)}
                placeholder="123456:ABC-DEF..."
                onPressEnter={submitAuth}
                size="large"
                autoFocus
                style={{ borderRadius: 10 }}
              />
              <Button type="primary" onClick={submitAuth} block loading={loading} size="large"
                style={{ borderRadius: 10, height: 46 }}>
                认证
              </Button>
            </Space>
          ) : successContent}
        </Card>
        </div>
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
    <div style={{ height: '100%', overflowY: 'auto' }}>
      <div style={{ maxWidth: 500, margin: '0 auto' }}>
      <Card
        style={{ borderRadius: 16, boxShadow: '0 2px 8px rgba(0,0,0,0.06)' }}
        styles={{ header: { borderBottom: '1px solid #f0f0f0' } }}
        title={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <SendOutlined style={{ color: '#6366f1' }} />
            认证客户端 {clientId.substring(0, 8)}...
          </div>
        }
      >
        <Steps current={step} items={stepLabels} style={{ marginBottom: 24 }} size="small" />
        {step < 3 ? (
          <Space direction="vertical" style={{ width: '100%' }} size="middle">
            <Alert type="info" message={descriptions[step]} showIcon style={{ borderRadius: 8 }} />
            {step === 0 ? (
              <>
                <Input
                  value={value}
                  onChange={e => setValue(e.target.value)}
                  placeholder="+8613800138000"
                  onPressEnter={() => value && sendPhoneCode(value)}
                  size="large"
                  autoFocus
                  style={{ borderRadius: 10 }}
                />
                <Button type="primary" onClick={() => sendPhoneCode(value)} block loading={loading} size="large"
                  style={{ borderRadius: 10, height: 46 }}>
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
                  style={{ borderRadius: 10 }}
                />
                <Button type="primary" onClick={submitAuth} block loading={loading} size="large"
                  style={{ borderRadius: 10, height: 46 }}>
                  {step === 1 ? '验证' : '提交密码'}
                </Button>
              </>
            )}
          </Space>
        ) : successContent}
      </Card>
      </div>
    </div>
  )
}

export default ClientAuth
