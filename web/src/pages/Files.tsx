import React, { useEffect, useState } from 'react'
import { Table, Button, Upload, message, Popconfirm, Space } from 'antd'
import { UploadOutlined, DeleteOutlined, DownloadOutlined } from '@ant-design/icons'
import apiClient from '../api/client'

const Files: React.FC = () => {
  const [files, setFiles] = useState<any[]>([])
  const [loading, setLoading] = useState(false)

  const fetchFiles = async () => {
    setLoading(true)
    try { const res = await apiClient.get('/api/files'); setFiles(res.data.data?.list ?? []) }
    catch { message.error('获取文件列表失败') }
    finally { setLoading(false) }
  }

  useEffect(() => { fetchFiles() }, [])

  const deleteFile = async (id: number) => {
    try { await apiClient.delete(`/api/files/${id}`); message.success('已删除'); fetchFiles() }
    catch (e: any) { message.error(e.message || '删除失败') }
  }

  const uploadProps = {
    name: 'file',
    action: '/api/files',
    headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
    onChange: (info: any) => {
      if (info.file.status === 'done') { message.success('上传成功'); fetchFiles() }
      else if (info.file.status === 'error') { message.error('上传失败') }
    },
  }

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '文件名', dataIndex: 'filename', key: 'filename' },
    { title: '上传者ID', dataIndex: 'uploader_id', key: 'uploader_id', width: 100 },
    { title: '创建时间', dataIndex: 'created_at', key: 'created_at' },
    { title: '操作', key: 'actions', width: 120,
      render: (_: any, r: any) => (
        <Space>
          <Button size="small" icon={<DownloadOutlined />} href={`/api/files/download/${r.filename}`} target="_blank">下载</Button>
          <Popconfirm title="确定删除？" onConfirm={() => deleteFile(r.id)}>
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ]

  return (
    <div>
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <h2>文件管理</h2>
        <Upload {...uploadProps} showUploadList={false}>
          <Button type="primary" icon={<UploadOutlined />}>上传文件</Button>
        </Upload>
      </div>
      <Table dataSource={files} columns={columns} rowKey="id" loading={loading} />
    </div>
  )
}

export default Files
