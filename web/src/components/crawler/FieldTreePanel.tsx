import { useCallback, useEffect, useState } from 'react'
import {
  Alert,
  Badge,
  Button,
  Collapse,
  Empty,
  Modal,
  Space,
  Spin,
  Tag,
  Tooltip,
  Tree,
  Typography,
} from 'antd'
import {
  AppstoreAddOutlined,
  DeleteOutlined,
  EditOutlined,
  FileAddOutlined,
  PlusOutlined,
} from '@ant-design/icons'
import type { DataNode } from 'antd/es/tree'
import * as crawlerApi from '../../api/crawler'
import type {
  CreateFieldNodeBody,
  ExtractorMode,
  FieldNode,
  FieldRule,
  FieldScope,
  FieldTree,
  QuickFieldPreset,
  SourceLayer,
} from '../../types'
import FieldNodeEditor, {
  EXTRACTOR_MODE_LABELS,
  FIELD_TYPE_LABELS,
  SOURCE_LAYER_LABELS,
} from './FieldNodeEditor'
import PresetFieldPicker from './PresetFieldPicker'

const { Text } = Typography

const MODE_COLORS: Record<ExtractorMode, string> = {
  css: 'blue',
  regex: 'purple',
  prefix_suffix: 'cyan',
  json_path: 'geekblue',
  meta_attr: 'gold',
  header_field: 'orange',
  follow_url: 'magenta',
}

export interface FieldTreePanelProps {
  taskId: number
  tree: FieldTree | null
  loading: boolean
  error: string | null
  /** 当前 URL（列表页，用于初始化 list_page 字段的 probe URL） */
  currentUrl: string
  /** 详情样本 URL（用于初始化 detail_page 字段的 probe URL；未取样本时 undefined） */
  detailUrl?: string
  userAgent?: string
  proxy?: string
  /** 行内快捷创建的预填配置（变化时自动打开编辑器并预填表单） */
  quickPreset?: QuickFieldPreset | null
  /** preset 被消费后由父组件清空（避免引用相同时不重复触发） */
  onPresetConsumed?: () => void
  /** 树刷新回调（创建/更新/删除后由父组件重新拉取） */
  onRefresh: () => void
}

/** 右侧字段树展示与编辑（US1 T032） */
export default function FieldTreePanel({
  taskId,
  tree,
  loading,
  error,
  currentUrl,
  detailUrl,
  userAgent,
  proxy,
  quickPreset,
  onPresetConsumed,
  onRefresh,
}: FieldTreePanelProps) {
  // 编辑器状态
  const [editorOpen, setEditorOpen] = useState(false)
  const [editorScope, setEditorScope] = useState<FieldScope>('list_page')
  const [editorParent, setEditorParent] = useState<number | null>(null)
  const [editorInitial, setEditorInitial] = useState<FieldNode['spec'] | null>(null)
  /** 行内快捷创建传入的预填配置（手动新增/编辑时清空） */
  const [editorPreset, setEditorPreset] = useState<Omit<QuickFieldPreset, 'scope'> | null>(null)
  // 预置字段库
  const [presetOpen, setPresetOpen] = useState(false)
  const [presetScope, setPresetScope] = useState<FieldScope>('list_page')
  const [insertingPreset, setInsertingPreset] = useState(false)
  const [insertError, setInsertError] = useState<string | null>(null)

  // 监听外部 quickPreset：变化时自动打开编辑器并应用预填
  useEffect(() => {
    if (!quickPreset) return
    setEditorScope(quickPreset.scope)
    setEditorParent(null)
    setEditorInitial(null)
    setEditorPreset({
      suggested_name: quickPreset.suggested_name,
      suggested_display_name: quickPreset.suggested_display_name,
      field_type: quickPreset.field_type,
      source_layer: quickPreset.source_layer,
      extractor_mode: quickPreset.extractor_mode,
      rule: quickPreset.rule,
      script_index: quickPreset.script_index,
    })
    setEditorOpen(true)
    onPresetConsumed?.()
    // 故意只依赖 quickPreset：onPresetConsumed 由父组件稳定提供即可
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [quickPreset])

  const openAddEditor = (scope: FieldScope, parent: number | null) => {
    setEditorScope(scope)
    setEditorParent(parent)
    setEditorInitial(null)
    setEditorPreset(null) // 用户主动新增，清空 preset
    setEditorOpen(true)
  }
  const openEditEditor = (scope: FieldScope, spec: NonNullable<FieldNode['spec']>) => {
    setEditorScope(scope)
    setEditorParent(spec.parent_id ?? null)
    setEditorInitial(spec)
    setEditorPreset(null) // 编辑模式不应用 preset
    setEditorOpen(true)
  }
  const openPreset = (scope: FieldScope) => {
    setPresetScope(scope)
    setPresetOpen(true)
  }

  const handleDelete = (node: FieldNode) => {
    const spec = node.spec
    if (!spec?.id) return
    Modal.confirm({
      title: `删除字段「${spec.display_name}」？`,
      content: '该字段的所有子孙节点将被一并删除（DB 外键 ON DELETE CASCADE）',
      okText: '删除',
      cancelText: '取消',
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await crawlerApi.deleteFieldNode(taskId, spec.id!)
          onRefresh()
        } catch (e: unknown) {
          const err = e as { response?: { data?: { message?: string } }; message?: string }
          Modal.error({
            title: '删除失败',
            content: err.response?.data?.message ?? err.message ?? '未知错误',
          })
        }
      },
    })
  }

  const handleInsertPreset = useCallback(
    async (keys: string[]) => {
      setInsertingPreset(true)
      setInsertError(null)
      try {
        // 简单实现：批量插入同名空白字段（用户后续编辑规则）
        // 实际可改为按 field_library 的 suggested_extractor 预填默认 rule
        const scope = presetScope
        for (const key of keys) {
          const body: CreateFieldNodeBody = {
            parent_id: null,
            scope,
            name: key,
            display_name: key,
            field_type: 'string',
            source_layer: 'html' as SourceLayer,
            extractor_mode: 'css' as ExtractorMode,
            rule: { mode: 'css', spec: { selector: '', attr: 'text' } } as FieldRule,
            post_processors: [],
            is_active: true,
          }
          await crawlerApi.createFieldNode(taskId, body)
        }
        setPresetOpen(false)
        onRefresh()
      } catch (e: unknown) {
        const err = e as { response?: { data?: { message?: string } }; message?: string }
        setInsertError(err.response?.data?.message ?? err.message ?? '插入失败')
      } finally {
        setInsertingPreset(false)
      }
    },
    [presetScope, taskId, onRefresh],
  )

  if (loading) {
    return (
      <div style={{ textAlign: 'center', padding: 40 }}>
        <Spin />
      </div>
    )
  }
  if (error) {
    return <Alert type="error" showIcon message={error} />
  }
  if (!tree) {
    return <Empty description="暂无字段树（请先在左侧填入 URL 并点继续）" />
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <Collapse
        defaultActiveKey={['list_page', 'detail_page']}
        items={[
          {
            key: 'list_page',
            label: (
              <Space>
                <Text strong>列表页字段</Text>
                <Badge count={countNodes(tree.list_page)} overflowCount={999} style={{ backgroundColor: '#1677ff' }} />
              </Space>
            ),
            extra: (
              <Space onClick={(e) => e.stopPropagation()}>
                <Button size="small" icon={<AppstoreAddOutlined />} onClick={() => openPreset('list_page')}>
                  预置字段
                </Button>
                <Button
                  size="small"
                  type="primary"
                  icon={<PlusOutlined />}
                  onClick={() => openAddEditor('list_page', null)}
                >
                  新增字段
                </Button>
              </Space>
            ),
            children: (
              <NodeList
                nodes={tree.list_page}
                onAddChild={(parentId) => openAddEditor('list_page', parentId)}
                onEdit={(spec) => openEditEditor('list_page', spec)}
                onDelete={handleDelete}
              />
            ),
          },
          {
            key: 'detail_page',
            label: (
              <Space>
                <Text strong>详情页字段</Text>
                <Badge count={countNodes(tree.detail_page)} overflowCount={999} style={{ backgroundColor: '#52c41a' }} />
              </Space>
            ),
            extra: (
              <Space onClick={(e) => e.stopPropagation()}>
                <Button size="small" icon={<AppstoreAddOutlined />} onClick={() => openPreset('detail_page')}>
                  预置字段
                </Button>
                <Button
                  size="small"
                  type="primary"
                  icon={<PlusOutlined />}
                  onClick={() => openAddEditor('detail_page', null)}
                >
                  新增字段
                </Button>
              </Space>
            ),
            children: (
              <NodeList
                nodes={tree.detail_page}
                onAddChild={(parentId) => openAddEditor('detail_page', parentId)}
                onEdit={(spec) => openEditEditor('detail_page', spec)}
                onDelete={handleDelete}
              />
            ),
          },
        ]}
      />

      <FieldNodeEditor
        open={editorOpen}
        taskId={taskId}
        parentNodeId={editorParent}
        scope={editorScope}
        initialUrl={
          editorScope === 'detail_page' && detailUrl ? detailUrl : currentUrl
        }
        userAgent={userAgent}
        proxy={proxy}
        initial={editorInitial}
        creationPreset={editorPreset}
        onSaved={() => {
          setEditorOpen(false)
          onRefresh()
        }}
        onCancel={() => setEditorOpen(false)}
      />

      <PresetFieldPicker
        open={presetOpen}
        scope={presetScope}
        onConfirm={handleInsertPreset}
        onCancel={() => setPresetOpen(false)}
      />
      {insertingPreset && (
        <div style={{ textAlign: 'center', padding: 20 }}>
          <Spin tip="批量插入中..." />
        </div>
      )}
      {insertError && <Alert type="error" showIcon message={insertError} />}
    </div>
  )
}

// ===================== NodeList =====================

function NodeList({
  nodes,
  onAddChild,
  onEdit,
  onDelete,
}: {
  nodes: FieldNode[]
  onAddChild: (parentId: number) => void
  onEdit: (spec: NonNullable<FieldNode['spec']>) => void
  onDelete: (node: FieldNode) => void
}) {
  if (nodes.length === 0) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="暂无字段"
        style={{ padding: 12 }}
      />
    )
  }
  const treeData: DataNode[] = nodes.map((n) => toDataNode(n, onAddChild, onEdit, onDelete))
  return <Tree treeData={treeData} defaultExpandAll blockNode showLine={{ showLeafIcon: false }} />
}

function toDataNode(
  node: FieldNode,
  onAddChild: (parentId: number) => void,
  onEdit: (spec: NonNullable<FieldNode['spec']>) => void,
  onDelete: (node: FieldNode) => void,
): DataNode {
  const spec = node.spec
  const title = spec ? (
    <Space size={4} wrap>
      <Text strong>{spec.display_name}</Text>
      <Text type="secondary" style={{ fontSize: 11 }}>
        ({spec.name})
      </Text>
      <Tag color={MODE_COLORS[spec.extractor_mode]}>{EXTRACTOR_MODE_LABELS[spec.extractor_mode]}</Tag>
      <Tag>{SOURCE_LAYER_LABELS[spec.source_layer]}</Tag>
      <Tag color={spec.field_type === 'link_card' ? 'magenta' : 'default'}>{FIELD_TYPE_LABELS[spec.field_type]}</Tag>
      {!spec.is_active && <Tag color="red">停用</Tag>}
      <Space size={2}>
        <Tooltip title="编辑">
          <Button
            size="small"
            type="text"
            icon={<EditOutlined />}
            onClick={(e) => {
              e.stopPropagation()
              onEdit(spec)
            }}
          />
        </Tooltip>
        <Tooltip title="新增子字段">
          <Button
            size="small"
            type="text"
            icon={<FileAddOutlined />}
            onClick={(e) => {
              e.stopPropagation()
              onAddChild(spec.id!)
            }}
          />
        </Tooltip>
        <Tooltip title="删除（含子孙）">
          <Button
            size="small"
            type="text"
            danger
            icon={<DeleteOutlined />}
            onClick={(e) => {
              e.stopPropagation()
              onDelete(node)
            }}
          />
        </Tooltip>
      </Space>
    </Space>
  ) : (
    <Space>
      <Text type="danger">[解析失败] {node.row ? 'DB 行存在但 spec 解析失败' : '未知'}</Text>
      <Tag color="red">{node.error}</Tag>
    </Space>
  )
  return {
    key: spec?.id ?? Math.random(),
    title,
    children: node.children.map((c) => toDataNode(c, onAddChild, onEdit, onDelete)),
  }
}

function countNodes(nodes: FieldNode[]): number {
  let n = 0
  for (const node of nodes) {
    n += 1
    n += countNodes(node.children)
  }
  return n
}
