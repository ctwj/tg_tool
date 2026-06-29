/**
 * FieldValueRenderer — 文章字段值动态渲染（feature 043 US1）
 *
 * 根据 field_path / value 的形态（单值 vs 多值）+ 关联 FieldTree 的 field_type
 * 选择合适的渲染策略：string/url/image/number/datetime/link_card/custom。
 *
 * 未命中字段显示"未命中"灰标（FR-027 / SC-004 字段保真）。
 */

import { useMemo } from 'react'
import { Tag, Typography, Image, Space, Empty } from 'antd'
import LinkIcon from '@ant-design/icons/LinkOutlined'
import {
  FieldNode,
  FieldTree,
  FieldType,
  ArticleFieldValue,
} from '../../types'

const { Text, Link, Paragraph } = Typography

interface FieldValueRendererProps {
  /** 该文章的所有字段值长表行 */
  values: ArticleFieldValue[]
  /** 关联的字段树（用于查询 field_type） */
  fieldTree?: FieldTree | null
  /** 限制渲染哪些 field_path（不传 = 全部渲染） */
  onlyPaths?: string[]
  /** 空数据时显示 */
  emptyHint?: string
}

/** 命中类型 */
type HitKind = 'string' | 'url' | 'image' | 'number' | 'datetime' | 'text' | 'custom'

/** 按 field_path 查找 field_type */
function lookupFieldType(fieldTree: FieldTree | null | undefined, path: string): FieldType | undefined {
  if (!fieldTree) return undefined
  const segments = path.split('/').filter(Boolean) // ['list_page', 'link_card', 'cover']
  if (segments.length === 0) return undefined
  // 第 0 段是 scope
  const roots = segments[0] === 'detail_page' ? fieldTree.detail_page : fieldTree.list_page
  return walk(roots, segments.slice(1))
}

function walk(nodes: FieldNode[], segments: string[]): FieldType | undefined {
  if (segments.length === 0) return undefined
  const head = segments[0]
  const found = nodes.find((n) => n.spec?.name === head)
  if (!found) return undefined
  if (segments.length === 1) return found.spec?.field_type
  return walk(found.children, segments.slice(1))
}

/** 把 field_type 映射为渲染分类 */
function classifyField(fieldType: FieldType | undefined, value: string): HitKind {
  if (fieldType === 'image') return 'image'
  if (fieldType === 'url' || fieldType === 'link_card') return 'url'
  if (fieldType === 'number') return 'number'
  if (fieldType === 'datetime') return 'datetime'
  if (fieldType === 'text') return 'text'
  if (fieldType === 'custom') return 'custom'
  // 无 field_type 时根据 value 形态启发式
  if (/^https?:\/\//i.test(value)) {
    if (/\.(png|jpe?g|webp|gif|svg|bmp)(\?|$)/i.test(value)) return 'image'
    return 'url'
  }
  return 'string'
}

/** 渲染单个值 */
function renderSingle(value: string, kind: HitKind, keyPrefix: string) {
  switch (kind) {
    case 'image':
      return (
        <div key={keyPrefix} style={{ marginBottom: 4 }}>
          <Image
            src={value}
            alt={value}
            width={120}
            style={{ borderRadius: 6, objectFit: 'cover' }}
            fallback="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxMjAiIGhlaWdodD0iOTAiPjxyZWN0IHdpZHRoPSIxMjAiIGhlaWdodD0iOTAiIGZpbGw9IiNlZWUiLz48dGV4dCB4PSI2MCIgeT0iNDUiIGZvbnQtc2l6ZT0iMTAiIHRleHQtYW5jaG9yPSJtaWRkbGUiIGZpbGw9IiNhYWEiPu5pbWFnZTwvdGV4dD48L3N2Zz4="
          />
          <div>
            <Text type="secondary" style={{ fontSize: 11, wordBreak: 'break-all' }}>
              {value}
            </Text>
          </div>
        </div>
      )
    case 'url':
      return (
        <div key={keyPrefix}>
          <Link href={value} target="_blank" rel="noopener noreferrer">
            <Space size={4}>
              <LinkIcon />
              <span style={{ wordBreak: 'break-all' }}>{value}</span>
            </Space>
          </Link>
        </div>
      )
    case 'number':
      return (
        <Tag key={keyPrefix} color="blue">
          {value}
        </Tag>
      )
    case 'datetime':
      return (
        <Tag key={keyPrefix} color="purple">
          {value}
        </Tag>
      )
    case 'text':
      return (
        <Paragraph key={keyPrefix} style={{ marginBottom: 4, whiteSpace: 'pre-wrap' }}>
          {value}
        </Paragraph>
      )
    case 'custom':
    case 'string':
    default:
      return (
        <Text key={keyPrefix} style={{ wordBreak: 'break-all' }}>
          {value}
        </Text>
      )
  }
}

/** 渲染单条 field_path 的所有命中 */
function renderFieldGroup(
  path: string,
  hits: ArticleFieldValue[],
  missed: number,
  fieldTree: FieldTree | null | undefined,
): React.ReactNode {
  const fieldType = lookupFieldType(fieldTree, path)
  // 用 display_name 或 path 最后一段作为标题
  const label = path.split('/').filter(Boolean).slice(-1)[0] ?? path

  return (
    <div
      key={path}
      style={{
        marginBottom: 12,
        padding: 8,
        border: '1px solid #e5e7eb',
        borderRadius: 6,
        background: '#fafafa',
      }}
    >
      <div style={{ marginBottom: 4, display: 'flex', alignItems: 'center', gap: 8 }}>
        <Text strong style={{ fontSize: 13 }}>
          {label}
        </Text>
        <Text type="secondary" style={{ fontSize: 11 }}>
          {path}
        </Text>
        {fieldType && (
          <Tag color="geekblue" style={{ fontSize: 11 }}>
            {fieldType}
          </Tag>
        )}
        <Tag color={hits.length > 0 ? 'green' : 'default'} style={{ fontSize: 11 }}>
          命中 {hits.length}
        </Tag>
        {missed > 0 && (
          <Tag color="default" style={{ fontSize: 11 }}>
            未命中 {missed}
          </Tag>
        )}
      </div>
      {hits.length === 0 ? (
        <Tag color="default">未命中</Tag>
      ) : hits.length === 1 ? (
        renderSingle(
          hits[0].value_text ?? String(hits[0].value_number ?? ''),
          classifyField(fieldType, hits[0].value_text ?? ''),
          `${path}-0`,
        )
      ) : (
        <Space direction="vertical" size={4} style={{ width: '100%' }}>
          {hits.map((h, idx) => {
            const v = h.value_text ?? String(h.value_number ?? '')
            const kind = classifyField(fieldType, v)
            // 若多值全部是 url 形态，统一用列表
            return renderSingle(v, kind, `${path}-${idx}`)
          })}
        </Space>
      )}
    </div>
  )
}

export default function FieldValueRenderer({
  values,
  fieldTree,
  onlyPaths,
  emptyHint = '暂无字段数据',
}: FieldValueRendererProps) {
  const groups = useMemo(() => {
    // 按 field_path 分组（保持插入顺序）
    const map = new Map<string, { hits: ArticleFieldValue[]; missed: number }>()
    for (const v of values) {
      if (onlyPaths && !onlyPaths.includes(v.field_path)) continue
      let entry = map.get(v.field_path)
      if (!entry) {
        entry = { hits: [], missed: 0 }
        map.set(v.field_path, entry)
      }
      if (v.is_hit) {
        entry.hits.push(v)
      } else {
        entry.missed += 1
      }
    }
    // 按 field_path 字典序排序（detail_page < list_page 顺序因 '/' 自然排序）
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b))
  }, [values, onlyPaths])

  if (groups.length === 0) {
    return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={emptyHint} />
  }

  return (
    <div>
      {groups.map(([path, { hits, missed }]) =>
        renderFieldGroup(path, hits, missed, fieldTree ?? null),
      )}
    </div>
  )
}
