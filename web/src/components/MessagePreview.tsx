import React from 'react'
import { Typography, Image } from 'antd'

const { Text, Paragraph } = Typography

interface MessagePreviewProps {
  rawData?: string
  content?: string
}

const MessagePreview: React.FC<MessagePreviewProps> = ({ rawData, content }) => {
  let parsed: Record<string, any> = {}
  if (rawData) {
    try { parsed = JSON.parse(rawData) } catch {}
  }

  const mediaType = parsed.media_type || ''
  const mediaUrl = parsed.media_url || ''

  return (
    <div style={{ maxWidth: 400 }}>
      {content && (
        <Paragraph ellipsis={{ rows: 3, expandable: true }} style={{ marginBottom: 8 }}>
          {content}
        </Paragraph>
      )}
      {mediaType === 'photo' && mediaUrl && (
        <Image src={mediaUrl} alt="photo" style={{ maxHeight: 200, borderRadius: 4 }} />
      )}
      {mediaType === 'video' && (
        <Text type="secondary">🎬 视频</Text>
      )}
      {mediaType === 'document' && (
        <Text type="secondary">📄 文件: {parsed.file_name || 'unknown'}</Text>
      )}
    </div>
  )
}

export default MessagePreview
