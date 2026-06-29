import { useMemo, useState } from 'react'
import { Alert, Button, Collapse, Empty, Input, Segmented, Space, Table, Tag, Tooltip, Typography } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import Editor from 'react-simple-code-editor'
import Prism from 'prismjs'
import 'prismjs/components/prism-markup' // HTML 高亮基础
import type {
  ExtractorMode,
  FieldRule,
  FieldType,
  MetaKeyKind,
  MetaTag,
  QuickFieldPreset,
  SourceLayer,
  SourceMaterial,
} from '../../types'

const { Text, Paragraph } = Typography

export type SourceViewerTab = 'headers' | 'html' | 'script' | 'meta'

/**
 * 快捷创建回调签名：除 scope 外的字段（scope 由父组件按当前素材 tab 注入）。
 */
export type QuickCreateHandler = (preset: Omit<QuickFieldPreset, 'scope'>) => void

export interface SourceViewerProps {
  material: SourceMaterial
  /** 初始 tab，默认 html */
  defaultTab?: SourceViewerTab
  /** 高亮关键词（验证字段命中时高亮 source_fragment 提取的值在源码中的位置） */
  highlightValues?: string[]
  /** 行内「创建为字段」回调（不传则隐藏所有快捷创建按钮） */
  onQuickCreate?: QuickCreateHandler
}

const TAB_OPTIONS = [
  { label: 'HTML', value: 'html' as const },
  { label: 'Headers', value: 'headers' as const },
  { label: 'Script', value: 'script' as const },
  { label: 'Meta', value: 'meta' as const },
]

/** 4 tab 只读源码查看器（US1 T029） */
export default function SourceViewer({
  material,
  defaultTab = 'html',
  highlightValues = [],
  onQuickCreate,
}: SourceViewerProps) {
  const [tab, setTab] = useState<SourceViewerTab>(defaultTab)
  const [scriptFilter, setScriptFilter] = useState('')
  const [expandedScripts, setExpandedScripts] = useState<number[]>([])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ padding: '8px 12px', borderBottom: '1px solid #f0f0f0' }}>
        <Segmented options={TAB_OPTIONS} value={tab} onChange={(v) => setTab(v as SourceViewerTab)} />
        <Text type="secondary" style={{ marginLeft: 12, fontSize: 12 }}>
          最终 URL：<Text code copyable>{material.final_url}</Text>
          <Tag color="blue" style={{ marginLeft: 8 }}>HTTP {material.status}</Tag>
          <Text type="secondary">{material.duration_ms} ms</Text>
        </Text>
      </div>

      <div style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: 12 }}>
        {tab === 'html' && <HtmlPane html={material.html} highlightValues={highlightValues} />}
        {tab === 'headers' && <HeadersPane headers={material.headers} onQuickCreate={onQuickCreate} />}
        {tab === 'script' && (
          <ScriptPane
            scripts={material.scripts}
            filter={scriptFilter}
            onFilter={setScriptFilter}
            expanded={expandedScripts}
            onExpanded={setExpandedScripts}
            onQuickCreate={onQuickCreate}
          />
        )}
        {tab === 'meta' && <MetaPane metas={material.metas} onQuickCreate={onQuickCreate} />}
      </div>
    </div>
  )
}

// ============================================================================
// 快捷创建辅助：MetaKeyKind → attr_name / 派生字段名 / 字段类型推断
// ============================================================================

/** MetaKeyKind → meta_attr 规则的 attr_name（与后端 meta_attr_matches 对齐） */
function metaKindToAttrName(kind: MetaKeyKind): string {
  switch (kind) {
    case 'name':
      return 'name'
    case 'property':
      return 'property'
    case 'http_equiv':
      return 'http-equiv'
    case 'other':
      // charset / 自定义：后端用 key 字段直接比对，attr_name 任意；回退 'name' 让用户改
      return 'name'
  }
}

/** 把 meta.key / header 名 / 任意字符串转成合法字段名（小写英文+下划线） */
function deriveFieldName(raw: string, fallback: string): string {
  const cleaned = raw
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .replace(/_{2,}/g, '_')
  if (!cleaned) return fallback
  if (!/^[a-z]/.test(cleaned)) return `${fallback}_${cleaned}`
  return cleaned.slice(0, 32)
}

/** 派生显示名：保留原 key 即可 */
function deriveDisplayName(raw: string): string {
  return raw.length > 48 ? raw.slice(0, 48) + '…' : raw
}

/** og:image / og:title 等 → image / title 字段类型；其余按 string */
function inferFieldTypeFromMeta(key: string): FieldType {
  const k = key.toLowerCase()
  if (k.includes('image') || k.includes('cover') || k.includes('thumb')) return 'image'
  if (k === 'og:url' || k === 'canonical') return 'url'
  return 'string'
}

/** 一键创建按钮（行内） */
function QuickCreateButton({
  onClick,
  label,
}: {
  onClick: () => void
  label?: string
}) {
  return (
    <Tooltip title={label ?? '基于此行一键创建字段，规则已自动填好'}>
      <Button
        size="small"
        type="link"
        icon={<PlusOutlined />}
        onClick={onClick}
      >
        创建字段
      </Button>
    </Tooltip>
  )
}

// ===================== HTML Pane =====================

function HtmlPane({ html, highlightValues }: { html: string; highlightValues: string[] }) {
  if (!html) {
    return <Empty description="无 HTML 内容" />
  }
  return (
    <div>
      {highlightValues.length > 0 && (
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 8 }}
          message={`高亮 ${highlightValues.length} 个匹配值（绿色背景）`}
        />
      )}
      <Editor
        value={highlightInHtml(html, highlightValues)}
        onValueChange={() => {}}
        highlight={(code) => Prism.highlight(code, Prism.languages.markup, 'markup')}
        padding={12}
        readOnly
        textareaClassName="source-viewer-html-readonly"
        style={{
          fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
          fontSize: 12,
          minHeight: 400,
          background: '#fafafa',
          border: '1px solid #f0f0f0',
          borderRadius: 4,
        }}
      />
      <style>{`
        .source-viewer-html-readonly { background: transparent !important; outline: none !important; }
      `}</style>
    </div>
  )
}

/** 在 HTML 中给指定值打上高亮 mark（简单 substring 替换；避免破坏 HTML tag） */
function highlightInHtml(html: string, values: string[]): string {
  if (!values.length) return html
  let out = html
  for (const v of values) {
    if (!v || v.length < 2) continue
    // 转义正则特殊字符
    const safe = v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    try {
      out = out.replace(new RegExp(safe, 'g'), (m) => `__HL_OPEN__${m}__HL_CLOSE__`)
    } catch {
      // ignore
    }
  }
  // 替换为 span（先开/关占位避免正则再次命中）
  out = out.replace(/__HL_OPEN__/g, '<mark style="background:#bae637;padding:0 2px;">')
  out = out.replace(/__HL_CLOSE__/g, '</mark>')
  return out
}

// ===================== Headers Pane =====================

function HeadersPane({
  headers,
  onQuickCreate,
}: {
  headers: Record<string, string>
  onQuickCreate?: QuickCreateHandler
}) {
  const entries = Object.entries(headers).sort(([a], [b]) => a.localeCompare(b))
  if (entries.length === 0) {
    return <Empty description="无 Headers" />
  }
  return (
    <Table
      size="small"
      pagination={false}
      dataSource={entries.map(([k, v], i) => ({ key: i, name: k, value: v }))}
      columns={[
        { title: 'Header', dataIndex: 'name', width: 200, render: (v: string) => <Text code>{v}</Text> },
        {
          title: 'Value',
          dataIndex: 'value',
          render: (v: string) => (
            <Paragraph style={{ margin: 0, wordBreak: 'break-all' }} copyable>
              {v}
            </Paragraph>
          ),
        },
        ...(onQuickCreate
          ? [
              {
                title: '操作',
                dataIndex: 'name',
                width: 100,
                render: (name: string) => (
                  <QuickCreateButton
                    onClick={() => {
                      const rule: FieldRule = {
                        mode: 'header_field',
                        spec: { header_name: name },
                      }
                      onQuickCreate({
                        suggested_name: deriveFieldName(name, 'header'),
                        suggested_display_name: deriveDisplayName(name),
                        field_type: 'string' as FieldType,
                        source_layer: 'header' as SourceLayer,
                        extractor_mode: 'header_field' as ExtractorMode,
                        rule,
                        script_index: null,
                      })
                    }}
                  />
                ),
              },
            ]
          : []),
      ]}
    />
  )
}

// ===================== Script Pane =====================

function ScriptPane({
  scripts,
  filter,
  onFilter,
  expanded,
  onExpanded,
  onQuickCreate,
}: {
  scripts: SourceMaterial['scripts']
  filter: string
  onFilter: (v: string) => void
  expanded: number[]
  onExpanded: (ids: number[]) => void
  onQuickCreate?: QuickCreateHandler
}) {
  const filtered = useMemo(() => {
    if (!filter.trim()) return scripts
    const f = filter.toLowerCase()
    return scripts.filter(
      (s) =>
        (s.src ?? '').toLowerCase().includes(f) ||
        (s.content ?? '').toLowerCase().includes(f),
    )
  }, [scripts, filter])

  if (scripts.length === 0) {
    return <Empty description="页面无 <script> 块" />
  }

  const toggle = (idx: number) => {
    if (expanded.includes(idx)) {
      onExpanded(expanded.filter((i) => i !== idx))
    } else {
      onExpanded([...expanded, idx])
    }
  }

  return (
    <div>
      <Input.Search
        placeholder="过滤脚本（src 或内容）"
        value={filter}
        onChange={(e) => onFilter(e.target.value)}
        allowClear
        style={{ marginBottom: 8 }}
      />
      <Text type="secondary">共 {scripts.length} 个 script 块，过滤后 {filtered.length} 个</Text>
      <Collapse
        accordion={false}
        activeKey={expanded}
        onChange={(keys) => onExpanded((keys as unknown[]).map((k) => Number(k)))}
        style={{ marginTop: 8 }}
        items={filtered.map((s) => {
          const key = s.index
          const header = (
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <Tag color={s.src ? 'blue' : 'default'}>#{s.index}</Tag>
              {s.src ? (
                <Text code copyable style={{ flex: 1 }}>
                  {s.src}
                </Text>
              ) : (
                <Text type="secondary">内联脚本（{(s.content ?? '').length} 字符）</Text>
              )}
            </div>
          )
          return {
            key,
            label: header,
            children: s.content ? (
              <Space direction="vertical" style={{ width: '100%' }} size={8}>
                {onQuickCreate && (
                  <Button
                    size="small"
                    type="dashed"
                    icon={<PlusOutlined />}
                    onClick={() => {
                      const rule: FieldRule = {
                        mode: 'json_path',
                        spec: { path: '$.' },
                      }
                      onQuickCreate({
                        suggested_name: `script_${s.index}_field`,
                        suggested_display_name: `脚本 #${s.index} 字段`,
                        field_type: 'string',
                        source_layer: 'script',
                        extractor_mode: 'json_path',
                        rule,
                        script_index: s.index,
                      })
                    }}
                  >
                    在此脚本上新建 JSON Path 字段
                  </Button>
                )}
                <Editor
                  value={s.content}
                  onValueChange={() => {}}
                  highlight={(code) => Prism.highlight(code, Prism.languages.markup, 'markup')}
                  padding={8}
                  readOnly
                  textareaClassName="source-viewer-script-readonly"
                  style={{
                    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
                    fontSize: 12,
                    background: '#fafafa',
                  }}
                />
              </Space>
            ) : (
              <Empty description="外链脚本（需单独抓取才能查看内容）" />
            ),
            extra: (
              <Tag
                onClick={(e) => {
                  e.stopPropagation()
                  toggle(key)
                }}
              >
                {expanded.includes(key) ? '收起' : '展开'}
              </Tag>
            ),
          }
        })}
      />
    </div>
  )
}

// ===================== Meta Pane =====================

function MetaPane({
  metas,
  onQuickCreate,
}: {
  metas: MetaTag[]
  onQuickCreate?: QuickCreateHandler
}) {
  if (metas.length === 0) {
    return <Empty description="无 <meta> 标签" />
  }
  return (
    <Table
      size="small"
      pagination={false}
      dataSource={metas.map((m, i) => ({ key: i, kind: m.key_kind, meta_key: m.key, content: m.content }))}
      columns={[
        {
          title: 'key_kind',
          dataIndex: 'kind',
          width: 110,
          render: (v: string) => <Tag>{v}</Tag>,
        },
        {
          title: 'key',
          dataIndex: 'meta_key',
          width: 200,
          render: (v: string) => <Text code>{v}</Text>,
        },
        {
          title: 'content',
          dataIndex: 'content',
          render: (v: string) => (
            <Paragraph style={{ margin: 0, wordBreak: 'break-all' }} copyable>
              {v}
            </Paragraph>
          ),
        },
        ...(onQuickCreate
          ? [
              {
                title: '操作',
                dataIndex: 'kind',
                width: 100,
                render: (_v: string, record: { kind: MetaKeyKind; meta_key: string }) => (
                  <QuickCreateButton
                    onClick={() => {
                      const attrName = metaKindToAttrName(record.kind)
                      const rule: FieldRule = {
                        mode: 'meta_attr',
                        spec: {
                          attr_name: attrName,
                          attr_value: record.meta_key,
                          content_key: 'content',
                        },
                      }
                      onQuickCreate({
                        suggested_name: deriveFieldName(record.meta_key, 'meta'),
                        suggested_display_name: deriveDisplayName(record.meta_key),
                        field_type: inferFieldTypeFromMeta(record.meta_key),
                        source_layer: 'meta',
                        extractor_mode: 'meta_attr',
                        rule,
                        script_index: null,
                      })
                    }}
                  />
                ),
              },
            ]
          : []),
      ]}
    />
  )
}
