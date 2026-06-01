// WebSocket Hook for real-time message notifications
import { useEffect, useRef, useState, useCallback } from 'react'

interface WSMessage {
  type: 'new_message' | 'client_status'
  data: Record<string, any>
}

export function useWebSocket() {
  const [messages, setMessages] = useState<WSMessage[]>([])
  const [connected, setConnected] = useState(false)
  const wsRef = useRef<WebSocket | null>(null)

  const connect = useCallback(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws/messages`
    const token = localStorage.getItem('token')

    const ws = new WebSocket(`${wsUrl}?token=${token}`)
    wsRef.current = ws

    ws.onopen = () => setConnected(true)
    ws.onclose = () => {
      setConnected(false)
      // Reconnect after 5 seconds
      setTimeout(() => connect(), 5000)
    }
    ws.onerror = () => ws.close()

    ws.onmessage = (event) => {
      try {
        const msg: WSMessage = JSON.parse(event.data)
        setMessages(prev => [msg, ...prev].slice(0, 100))
      } catch {}
    }
  }, [])

  useEffect(() => {
    connect()
    return () => {
      wsRef.current?.close()
    }
  }, [connect])

  const clearMessages = useCallback(() => setMessages([]), [])

  return { messages, connected, clearMessages }
}
