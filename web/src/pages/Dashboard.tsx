import React, { useEffect, useState } from 'react'
import { Card, Col, Row, Statistic, Typography } from 'antd'
import { ApiOutlined } from '@ant-design/icons'
import apiClient from '../api/client'
import type { SystemStatus } from '../types'

const { Title } = Typography

const Dashboard: React.FC = () => {
  const [data, setData] = useState<SystemStatus | null>(null)

  useEffect(() => {
    apiClient.get('/status').then(res => setData(res.data.data)).catch(() => {})
  }, [])

  return (
    <div>
      <Title level={3}>仪表盘</Title>
      <Row gutter={16}>
        <Col span={8}>
          <Card><Statistic title="客户端总数" value={data?.clients.total ?? 0} prefix={<ApiOutlined />} /></Card>
        </Col>
        <Col span={8}>
          <Card><Statistic title="在线客户端" value={data?.clients.active ?? 0} prefix={<ApiOutlined />} valueStyle={{ color: '#3f8600' }} /></Card>
        </Col>
        <Col span={8}>
          <Card><Statistic title="系统版本" value={data?.version ?? '-'} /></Card>
        </Col>
      </Row>
    </div>
  )
}

export default Dashboard
