import React, { useState } from 'react'
import { Card, Steps, Input, Button, message, Typography, Space } from 'antd'
import { useNavigate, useSearchParams } from 'react-router-dom'
import apiClient from '../api/client'

const { Text } = Typography

const ClientAuth: React.FC = () => {
  const [searchParams] = useSearchParams()
  const clientId = searchParams.get('id') || ''
  const navigate = useNavigate()
  const [step, setStep] = useState(0) // 0: phone, 1: code, 2: password, 3: done
  const [value, setValue] = useState('')

  const submitAuth = async () => {
    if (!value) { message.warning('请输入值'); return }
    const type = step === 0 ? 'phone' : step === 1 ? 'code' : 'password'
    try {
      await apiClient.post(`/clients/${clientId}/auth`, { type, value })
      if (step === 0) { setStep(1); message.success('验证码已发送') }
      else if (step === 1) { setStep(2); message.success('验证码已验证') }
      else { setStep(3); message.success('认证完成！') }
      setValue('')
    } catch (e: any) { message.error(e.message || '认证失败') }
  }

  return (
    <div style={{ maxWidth: 500, margin: '0 auto' }}>
      <Card title={`认证客户端 ${clientId}`}>
        <Steps current={step} items={[
          { title: '手机号' }, { title: '验证码' }, { title: '两步验证' }, { title: '完成' },
        ]} style={{ marginBottom: 24 }} />
        {step < 3 ? (
          <Space direction="vertical" style={{ width: '100%' }}>
            <Text>{step === 0 ? '输入手机号以开始认证' : step === 1 ? '输入收到的验证码' : '输入两步验证密码（如需要）'}</Text>
            <Input value={value} onChange={e => setValue(e.target.value)}
              placeholder={step === 0 ? '手机号' : step === 1 ? '验证码' : '两步验证密码'}
              onPressEnter={submitAuth} />
            <Button type="primary" onClick={submitAuth} block>提交</Button>
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
