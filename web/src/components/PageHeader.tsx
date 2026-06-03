import React from 'react'

interface PageHeaderProps {
  title: string
  description?: string
  extra?: React.ReactNode
}

const PageHeader: React.FC<PageHeaderProps> = ({ title, description, extra }) => {
  return (
    <div style={{
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      marginBottom: 24,
    }}>
      <div>
        <h2 style={{
          margin: 0,
          fontSize: 22,
          fontWeight: 600,
          color: '#1e1b4b',
          lineHeight: 1.3,
        }}>
          {title}
        </h2>
        {description && (
          <div style={{
            marginTop: 4,
            fontSize: 14,
            color: '#6b7280',
          }}>
            {description}
          </div>
        )}
      </div>
      {extra && <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>{extra}</div>}
    </div>
  )
}

export default PageHeader
