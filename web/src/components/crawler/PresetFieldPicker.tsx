import { useEffect, useState } from 'react'
import { Alert, Button, Checkbox, Empty, Modal, Spin, Typography } from 'antd'
import * as crawlerApi from '../../api/crawler'
import type { FieldLibraryCategory, FieldScope } from '../../types'

const { Text, Title } = Typography

export interface PresetFieldPickerProps {
  open: boolean
  scope: FieldScope
  onConfirm: (keys: string[]) => void
  onCancel: () => void
}

/** 预置字段库勾选弹窗（US1 T031）
 *
 * 用户从预设字段库勾选一批字段，批量插入到当前 scope。
 * 实际插入由 FieldTreePanel 调用 `createFieldNode` 完成。
 */
export default function PresetFieldPicker({
  open,
  scope,
  onConfirm,
  onCancel,
}: PresetFieldPickerProps) {
  const [loading, setLoading] = useState(false)
  const [categories, setCategories] = useState<FieldLibraryCategory[]>([])
  const [error, setError] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())

  useEffect(() => {
    if (!open) return
    setLoading(true)
    setError(null)
    setSelected(new Set())
    crawlerApi
      .getFieldLibrary()
      .then((res) => {
        if (res.success && res.data) {
          setCategories(res.data)
        } else {
          setError(res.message ?? '加载失败')
        }
      })
      .catch((e: unknown) => {
        const err = e as { response?: { data?: { message?: string } }; message?: string }
        setError(err.response?.data?.message ?? err.message ?? '加载失败')
      })
      .finally(() => setLoading(false))
  }, [open])

  function toggle(key: string) {
    const next = new Set(selected)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    setSelected(next)
  }

  function handleConfirm() {
    onConfirm(Array.from(selected))
  }

  return (
    <Modal
      open={open}
      title={`添加预置字段 — ${scope}`}
      width={720}
      onCancel={onCancel}
      destroyOnClose
      footer={[
        <Button key="cancel" onClick={onCancel}>
          取消
        </Button>,
        <Button
          key="ok"
          type="primary"
          disabled={selected.size === 0}
          onClick={handleConfirm}
        >
          添加 {selected.size > 0 ? `(${selected.size})` : ''}
        </Button>,
      ]}
    >
      <Text type="secondary">勾选要批量插入到 {scope} 的字段（建议模式下立即创建空白规则）</Text>

      {loading && (
        <div style={{ textAlign: 'center', padding: 40 }}>
          <Spin />
        </div>
      )}
      {error && <Alert type="error" showIcon message={error} style={{ marginTop: 12 }} />}
      {!loading && !error && categories.length === 0 && (
        <Empty description="字段库为空（请检查种子数据）" style={{ padding: 40 }} />
      )}

      {!loading && categories.length > 0 && (
        <div style={{ marginTop: 16, maxHeight: '60vh', overflow: 'auto' }}>
          {categories.map((cat) => (
            <div key={cat.category} style={{ marginBottom: 16 }}>
              <Title level={5} style={{ marginBottom: 8 }}>
                {cat.label} <Text type="secondary" style={{ fontSize: 12 }}>({cat.category})</Text>
              </Title>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 6 }}>
                {cat.entries.map((e) => (
                  <Checkbox
                    key={e.key}
                    checked={selected.has(e.key)}
                    onChange={() => toggle(e.key)}
                  >
                    <Text strong style={{ marginRight: 6 }}>
                      {e.display_name}
                    </Text>
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      {e.key} · {e.field_type}
                      {e.suggested_extractor ? ` · ${e.suggested_extractor}` : ''}
                    </Text>
                  </Checkbox>
                ))}
              </div>
              {e_hint(cat)}
            </div>
          ))}
        </div>
      )}
    </Modal>
  )
}

/** 防止 react key 警告的占位组件（如分类无描述直接返回 null） */
function e_hint(cat: FieldLibraryCategory): React.ReactNode {
  const firstDesc = cat.entries.find((e) => e.description)?.description
  if (!firstDesc) return null
  return (
    <Text type="secondary" style={{ fontSize: 11, marginTop: 4, display: 'block' }}>
      示例：{firstDesc}
    </Text>
  )
}
