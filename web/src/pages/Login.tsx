import React from 'react'
import { Form, Input, Button, message, Tabs } from 'antd'
import { UserOutlined, LockOutlined, MailOutlined, SendOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../hooks/useAuth'
import { getRegisterStatus } from '../api/auth'

const Login: React.FC = () => {
  const navigate = useNavigate()
  const { login, register } = useAuth()
  const [loading, setLoading] = React.useState(false)
  const [allowRegister, setAllowRegister] = React.useState(true)
  const [statusLoaded, setStatusLoaded] = React.useState(false)

  React.useEffect(() => {
    getRegisterStatus()
      .then(res => {
        setAllowRegister(res.data?.allow_register !== false)
      })
      .catch(() => {
        setAllowRegister(true)
      })
      .finally(() => {
        setStatusLoaded(true)
      })
  }, [])

  const onLogin = async (values: { username: string; password: string }) => {
    setLoading(true)
    try {
      await login(values.username, values.password)
      message.success('登录成功')
      navigate('/dashboard')
    } catch (e: any) {
      message.error(e.message || '登录失败')
    } finally {
      setLoading(false)
    }
  }

  const onRegister = async (values: { username: string; password: string; email?: string }) => {
    setLoading(true)
    try {
      await register(values.username, values.password, values.email)
      message.success('注册成功，请登录')
    } catch (e: any) {
      message.error(e.message || '注册失败')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="login-bg" style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: '100vh' }}>
      <div style={{
        width: 420,
        background: 'rgba(255, 255, 255, 0.95)',
        backdropFilter: 'blur(20px)',
        borderRadius: 20,
        padding: '48px 40px',
        boxShadow: '0 20px 60px rgba(0, 0, 0, 0.15)',
      }}>
        {/* Brand */}
        <div style={{ textAlign: 'center', marginBottom: 32 }}>
          <div style={{
            width: 56,
            height: 56,
            borderRadius: 16,
            background: 'linear-gradient(135deg, #6366f1, #818cf8)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            margin: '0 auto 16px',
            boxShadow: '0 8px 24px rgba(99, 102, 241, 0.3)',
          }}>
            <SendOutlined style={{ fontSize: 26, color: '#fff' }} />
          </div>
          <h1 style={{ fontSize: 24, fontWeight: 700, color: '#1e1b4b', margin: 0 }}>
            TG Forwarding
          </h1>
          <p style={{ fontSize: 14, color: '#6b7280', marginTop: 4 }}>
            Telegram 消息转发管理平台
          </p>
        </div>

        {!statusLoaded ? (
          <Form style={{ marginTop: 8 }}>
            <Form.Item name="username">
              <Input
                prefix={<UserOutlined style={{ color: '#a5b4fc' }} />}
                placeholder="用户名"
                size="large"
                style={{ borderRadius: 10, height: 46 }}
              />
            </Form.Item>
            <Form.Item name="password">
              <Input.Password
                prefix={<LockOutlined style={{ color: '#a5b4fc' }} />}
                placeholder="密码"
                size="large"
                style={{ borderRadius: 10, height: 46 }}
              />
            </Form.Item>
          </Form>
        ) : allowRegister ? (
          <Tabs
            centered
            items={[
              {
                key: 'login',
                label: <span style={{ fontSize: 15 }}>登录</span>,
                children: (
                  <Form onFinish={onLogin} style={{ marginTop: 8 }}>
                    <Form.Item name="username" rules={[{ required: true, message: '请输入用户名' }]}>
                      <Input
                        prefix={<UserOutlined style={{ color: '#a5b4fc' }} />}
                        placeholder="用户名"
                        size="large"
                        style={{ borderRadius: 10, height: 46 }}
                      />
                    </Form.Item>
                    <Form.Item name="password" rules={[{ required: true, message: '请输入密码' }]}>
                      <Input.Password
                        prefix={<LockOutlined style={{ color: '#a5b4fc' }} />}
                        placeholder="密码"
                        size="large"
                        style={{ borderRadius: 10, height: 46 }}
                      />
                    </Form.Item>
                    <Form.Item style={{ marginBottom: 0 }}>
                      <Button
                        type="primary"
                        htmlType="submit"
                        loading={loading}
                        block
                        size="large"
                        style={{
                          borderRadius: 10,
                          height: 46,
                          fontSize: 15,
                          fontWeight: 500,
                          background: 'linear-gradient(135deg, #6366f1, #818cf8)',
                          border: 'none',
                        }}
                      >
                        登录
                      </Button>
                    </Form.Item>
                  </Form>
                ),
              },
              {
                key: 'register',
                label: <span style={{ fontSize: 15 }}>注册</span>,
                children: (
                  <Form onFinish={onRegister} style={{ marginTop: 8 }}>
                    <Form.Item name="username" rules={[{ required: true, message: '请输入用户名' }]}>
                      <Input
                        prefix={<UserOutlined style={{ color: '#a5b4fc' }} />}
                        placeholder="用户名"
                        size="large"
                        style={{ borderRadius: 10, height: 46 }}
                      />
                    </Form.Item>
                    <Form.Item name="password" rules={[{ required: true, message: '请输入密码' }]}>
                      <Input.Password
                        prefix={<LockOutlined style={{ color: '#a5b4fc' }} />}
                        placeholder="密码"
                        size="large"
                        style={{ borderRadius: 10, height: 46 }}
                      />
                    </Form.Item>
                    <Form.Item name="email">
                      <Input
                        prefix={<MailOutlined style={{ color: '#a5b4fc' }} />}
                        placeholder="邮箱（可选）"
                        size="large"
                        style={{ borderRadius: 10, height: 46 }}
                      />
                    </Form.Item>
                    <Form.Item style={{ marginBottom: 0 }}>
                      <Button
                        type="primary"
                        htmlType="submit"
                        loading={loading}
                        block
                        size="large"
                        style={{
                          borderRadius: 10,
                          height: 46,
                          fontSize: 15,
                          fontWeight: 500,
                          background: 'linear-gradient(135deg, #6366f1, #818cf8)',
                          border: 'none',
                        }}
                      >
                        注册
                      </Button>
                    </Form.Item>
                  </Form>
                ),
              },
            ]}
          />
        ) : (
          <Form onFinish={onLogin} style={{ marginTop: 8 }}>
            <Form.Item name="username" rules={[{ required: true, message: '请输入用户名' }]}>
              <Input
                prefix={<UserOutlined style={{ color: '#a5b4fc' }} />}
                placeholder="用户名"
                size="large"
                style={{ borderRadius: 10, height: 46 }}
              />
            </Form.Item>
            <Form.Item name="password" rules={[{ required: true, message: '请输入密码' }]}>
              <Input.Password
                prefix={<LockOutlined style={{ color: '#a5b4fc' }} />}
                placeholder="密码"
                size="large"
                style={{ borderRadius: 10, height: 46 }}
              />
            </Form.Item>
            <Form.Item style={{ marginBottom: 0 }}>
              <Button
                type="primary"
                htmlType="submit"
                loading={loading}
                block
                size="large"
                style={{
                  borderRadius: 10,
                  height: 46,
                  fontSize: 15,
                  fontWeight: 500,
                  background: 'linear-gradient(135deg, #6366f1, #818cf8)',
                  border: 'none',
                }}
              >
                登录
              </Button>
            </Form.Item>
          </Form>
        )}
      </div>
    </div>
  )
}

export default Login
