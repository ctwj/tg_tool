import { useEffect, useMemo, useState } from 'react'
import {
  Alert,
  AutoComplete,
  Button,
  Card,
  Col,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Row,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
} from 'antd'
import { MinusCircleOutlined, PlusOutlined, SafetyCertificateOutlined } from '@ant-design/icons'
import * as crawlerApi from '../../api/crawler'
import FollowUrlRuleEditor from './FollowUrlRuleEditor'
import type {
  CreateFieldNodeBody,
  ExtractorMode,
  FieldLibraryCategory,
  FieldRule,
  FieldScope,
  FieldNodeSpec,
  FieldType,
  PerParentSample,
  PostProcessor,
  PostProcessorOp,
  ProbeResponse,
  QuickFieldPreset,
  ScriptRule,
  SourceLayer,
  SubRule,
} from '../../types'

const { Text, Paragraph } = Typography

const NAME_REGEX = /^[a-z][a-z0-9_]{0,31}$/

const FIELD_TYPES: FieldType[] = [
  'string',
  'text',
  'url',
  'image',
  'number',
  'datetime',
  'link_card',
  'pagination',
  'custom',
]

const SOURCE_LAYERS: SourceLayer[] = ['html', 'header', 'script', 'meta', 'url']

const EXTRACTOR_MODES: ExtractorMode[] = [
  'css',
  'regex',
  'prefix_suffix',
  'json_path',
  'meta_attr',
  'header_field',
  'follow_url',
  'script',
]

/** 字段类型中文标签（与 FIELD_TYPES 对齐） */
export const FIELD_TYPE_LABELS: Record<FieldType, string> = {
  string: '字符串',
  text: '长文本',
  url: 'URL',
  image: '图片',
  number: '数字',
  datetime: '日期时间',
  link_card: '链接卡片',
  pagination: '分页',
  custom: '自定义',
}

/** 字段类型后端语义说明（决定系统如何处理该字段的提取值） */
const FIELD_TYPE_HINTS: Partial<Record<FieldType, string>> = {
  url: '提取后自动相对 URL → 绝对；list_page 中此类型会作为详情页入口',
  image: 'URL 自动绝对化，并进入图片下载 → 上传图床管线',
  link_card: '容器类型：在其子节点中找 url 子字段，用于关联详情页',
  pagination: '命中值作为"下一页"URL，触发链式翻页（max_pagination_depth 限深）',
  string: '普通字符串，仅存库，不触发特殊后端处理',
  text: '长文本（正文/描述），仅存库',
  number: '数值，自动解析为数字存储',
  datetime: '日期时间字符串，按文本存库',
  custom: '自定义类型，仅存库',
}

/** 源码层中文标签 */
export const SOURCE_LAYER_LABELS: Record<SourceLayer, string> = {
  html: 'HTML 源码',
  header: '响应头',
  script: '脚本块',
  meta: 'Meta 标签',
  url: 'URL 本身',
}

/** 匹配模式中文标签 */
export const EXTRACTOR_MODE_LABELS: Record<ExtractorMode, string> = {
  css: 'CSS 选择器',
  regex: '正则匹配',
  prefix_suffix: '前后缀匹配',
  json_path: 'JSON Path',
  meta_attr: 'Meta 属性',
  header_field: '响应头字段',
  follow_url: '跟随 URL 二次提取',
  script: 'JS 脚本（沙箱）',
}

/** 匹配模式说明（用于在 UI 上给用户提示该模式的适用场景） */
export const EXTRACTOR_MODE_HINTS: Partial<Record<ExtractorMode, string>> = {
  follow_url:
    '两阶段：先用 transit 子规则抓中转 URL → 请求该 URL → 用 extract 子规则在响应上抓最终值。适用于下载地址藏在中转页的场景',
  script:
    'JS 沙箱（rquickjs）求值：(function(ctx){ ... })，注入 ctx.value/fields/url/fetch。仅 detail_page 作用域；超时 100ms；禁用 Function/eval/process/setTimeout。常用于 6 模式无法覆盖的复杂解析（如 jQuery $.get 注入 URL、JSON.parse 嵌套字段）',
}

const POST_PROCESSOR_OPS: PostProcessorOp[] = [
  'trim',
  'html_entity_decode',
  'absolutize_url',
  'first',
  'all',
  'dedupe',
]

/** 后处理链 op 中文标签 */
const POST_PROCESSOR_OP_LABELS: Record<PostProcessorOp, string> = {
  trim: '去空白',
  html_entity_decode: 'HTML 实体解码',
  absolutize_url: '相对 URL 转绝对',
  first: '取首个',
  all: '取全部',
  dedupe: '去重',
}

/** CSS 模式 attr 常用值（提取内容）。
 *  AutoComplete 允许输入任意属性名（如懒加载图片常用的 data-src / data-original），
 *  这里只列出常见项作为快捷建议，不在列表中的值也可手动输入后回车保存。 */
const ATTR_OPTIONS = [
  { value: 'text', label: 'text — 纯文本（去掉 HTML 标签）' },
  { value: 'html', label: 'html — 含标签的 HTML 片段' },
  { value: 'href', label: 'href — 链接 URL（<a> 标签）' },
  { value: 'src', label: 'src — 图片/资源 URL（<img>/<script>）' },
  { value: 'data-src', label: 'data-src — 懒加载图片 URL（常见于 lazyload）' },
  { value: 'data-original', label: 'data-original — jQuery.lazyload 原图 URL' },
  { value: 'data-lazy-src', label: 'data-lazy-src — WP Rocket / a3 lazy load' },
  { value: 'title', label: 'title — title 属性' },
  { value: 'content', label: 'content — content 属性' },
]

export interface FieldNodeEditorProps {
  open: boolean
  taskId: number
  /** 父节点 ID（添加子字段时） */
  parentNodeId?: number | null
  /** 当前作用域 */
  scope: FieldScope
  /** 初始 URL（验证时用） */
  initialUrl: string
  /** 初始 UA / Proxy（来自任务配置） */
  userAgent?: string
  proxy?: string
  /** 编辑时传入已有 spec；新建时传 null */
  initial?: FieldNodeSpec | null
  /** 新建模式下的预填配置（来自 SourceViewer「创建为字段」快捷按钮） */
  creationPreset?: Omit<QuickFieldPreset, 'scope'> | null
  /** [US2] 当前作用域已存在的兄弟字段名（供 ScriptRuleEditor 提示 ctx.fields.<name>） */
  siblingFieldNames?: string[]
  onSaved: () => void
  onCancel: () => void
}

/** 默认 rule（按 mode） */
function defaultRule(mode: ExtractorMode): FieldRule {
  switch (mode) {
    case 'css':
      return { mode: 'css', spec: { selector: '', attr: 'text' } }
    case 'regex':
      return { mode: 'regex', spec: { pattern: '', group: 1, flags: '' } }
    case 'prefix_suffix':
      return { mode: 'prefix_suffix', spec: { prefix: '', suffix: '', include_boundary: false, case_sensitive: false } }
    case 'json_path':
      return { mode: 'json_path', spec: { path: '$.' } }
    case 'meta_attr':
      return { mode: 'meta_attr', spec: { attr_name: 'name', attr_value: '', content_key: 'content' } }
    case 'header_field':
      return { mode: 'header_field', spec: { header_name: '' } }
    case 'follow_url':
      return {
        mode: 'follow_url',
        spec: {
          transit: { mode: 'css', spec: { selector: '', attr: 'href' } },
          transit_layer: 'html',
          transit_script_index: null,
          target_layer: 'html',
          target_script_index: null,
          extract: { mode: 'css', spec: { selector: '', attr: 'href' } },
        },
      }
    case 'script':
      // [feature 046] JS 沙箱默认模板：直接返回原值（提示用户改造）
      return {
        mode: 'script',
        spec: {
          body: '// ctx.value: 6 模式提取的原始值（未匹配时为空串）\n// ctx.fields: 同作用域兄弟字段（US2）\n// ctx.url: 当前详情页 URL\nreturn ctx.value\n',
          api_version: 'v1',
        },
      }
  }
}

/** 单字段编辑表单 + 验证 + 持久化（US1 T030） */
export default function FieldNodeEditor({
  open,
  taskId,
  parentNodeId,
  scope,
  initialUrl,
  userAgent,
  proxy,
  initial,
  creationPreset,
  siblingFieldNames,
  onSaved,
  onCancel,
}: FieldNodeEditorProps) {
  const isEdit = !!initial
  const [name, setName] = useState('')
  const [fieldType, setFieldType] = useState<FieldType>('string')
  const [sourceLayer, setSourceLayer] = useState<SourceLayer>('html')
  const [extractorMode, setExtractorMode] = useState<ExtractorMode>('css')
  const [rule, setRule] = useState<FieldRule>(() => defaultRule('css'))
  const [postProcessors, setPostProcessors] = useState<PostProcessor[]>([])
  const [scriptIndex, setScriptIndex] = useState<number | null>(null)
  const [isActive, setIsActive] = useState(true)
  const [refreshOnRead, setRefreshOnRead] = useState(false)
  const [probeUrl, setProbeUrl] = useState(initialUrl)
  const [probing, setProbing] = useState(false)
  const [probeResult, setProbeResult] = useState<ProbeResponse | null>(null)
  const [probeError, setProbeError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  // 预置字段库（name 下拉候选 + 选中后联动 display_name / field_type / suggested_extractor）
  const [library, setLibrary] = useState<FieldLibraryCategory[]>([])
  useEffect(() => {
    if (!open) return
    crawlerApi
      .getFieldLibrary()
      .then((res) => {
        if (res.success && res.data) setLibrary(res.data)
      })
      .catch(() => {
        /* 静默失败：name 仍可手动输入 */
      })
  }, [open])

  /** 扁平化的预置字段：key → { display_name, field_type, suggested_extractor } */
  const libraryFlat = useMemo(() => {
    const map = new Map<string, { display_name: string; field_type: FieldType; suggested?: string }>()
    for (const cat of library) {
      for (const e of cat.entries) {
        map.set(e.key, {
          display_name: e.display_name,
          field_type: e.field_type as FieldType,
          suggested: e.suggested_extractor ?? undefined,
        })
      }
    }
    return map
  }, [library])

  /** 字段名下拉候选：预置字段 + 容器/分页等系统类型 */
  const nameOptions = useMemo(() => {
    const fromLib = library.flatMap((cat) =>
      cat.entries.map((e) => ({
        value: e.key,
        label: `${e.display_name} (${e.key})`,
      })),
    )
    // 系统级容器/分页字段（不在预置库里但常用）
    const systemNames = [
      { value: 'link_card', label: '链接卡片 (link_card)' },
      { value: 'pagination', label: '分页指针 (pagination)' },
    ]
    // 去重（预置库可能含同名）
    const seen = new Set<string>()
    const merged = [...systemNames, ...fromLib].filter((o) => {
      if (seen.has(o.value)) return false
      seen.add(o.value)
      return true
    })
    return merged
  }, [library])

  /** 显示名自动跟随字段名：预置库中文优先，系统级容器/分页次之，兜底用 name 本身。
   *  快捷创建场景：name 仍是预设值时优先用预设中文显示名（如 description → 描述） */
  const derivedDisplayName = useMemo(() => {
    if (!name) return ''
    if (name === 'link_card') return '链接卡片'
    if (name === 'pagination') return '分页'
    if (creationPreset?.suggested_name && name === creationPreset.suggested_name && creationPreset.suggested_display_name) {
      return creationPreset.suggested_display_name
    }
    const meta = libraryFlat.get(name)
    if (meta) return meta.display_name
    return name
  }, [name, libraryFlat, creationPreset])

  /** name 选中时联动（仅普通新建模式 + 用户从下拉选了预置字段时）。
   *  快捷创建模式（creationPreset 非 null）下：预设已填好全部配置，
   *  改 name 不触发联动，避免覆盖 extractor_mode/source_layer/rule（"幻化"现象）。
   */
  const handleNamePick = (val: string) => {
    setName(val)
    if (isEdit) return // 编辑模式不覆盖已有配置
    if (creationPreset) return // 快捷创建：锁住预设配置，只改 name
    // 系统级容器/分页字段：name 即类型
    if (val === 'link_card') {
      setFieldType('link_card')
      return
    }
    if (val === 'pagination') {
      setFieldType('pagination')
      return
    }
    const meta = libraryFlat.get(val)
    if (!meta) return
    setFieldType(meta.field_type)
    if (meta.suggested) {
      const m = meta.suggested as ExtractorMode
      if (EXTRACTOR_MODES.includes(m)) {
        setExtractorMode(m)
        setRule(defaultRule(m))
      }
    }
  }

  // 当 initial 变化时重置表单
  useEffect(() => {
    if (!open) return
    if (initial) {
      setName(initial.name)
      setFieldType(initial.field_type)
      setSourceLayer(initial.source_layer)
      setExtractorMode(initial.extractor_mode)
      setRule(initial.rule)
      setPostProcessors(initial.post_processors ?? [])
      setScriptIndex(initial.script_index ?? null)
      setIsActive(initial.is_active ?? true)
      setRefreshOnRead(initial.refresh_on_read ?? false)
    } else if (creationPreset) {
      // 新建模式 + 行内快捷创建：应用预填配置（让用户看到「为什么这么填」）
      setName(creationPreset.suggested_name ?? '')
      setFieldType(creationPreset.field_type ?? 'string')
      setSourceLayer(creationPreset.source_layer)
      setExtractorMode(creationPreset.extractor_mode)
      setRule(creationPreset.rule)
      setPostProcessors([])
      setScriptIndex(creationPreset.script_index ?? null)
      setIsActive(true)
      setRefreshOnRead(false)
    } else {
      setName('')
      setFieldType('string')
      setSourceLayer('html')
      setExtractorMode('css')
      setRule(defaultRule('css'))
      setPostProcessors([])
      setScriptIndex(null)
      setIsActive(true)
      setRefreshOnRead(false)
    }
    setProbeUrl(initialUrl)
    setProbeResult(null)
    setProbeError(null)
    setSubmitError(null)
  }, [open, initial, initialUrl, creationPreset])

  // 切换 extractor_mode 时重置 rule（保持 mode 与 rule 一致）
  // 并按 mode 推断默认 source_layer（json_path→script / meta_attr→meta / header_field→header）
  useEffect(() => {
    if (rule.mode !== extractorMode) {
      setRule(defaultRule(extractorMode))
    }
    const impliedLayer: Record<ExtractorMode, SourceLayer | null> = {
      css: null,
      regex: null,
      prefix_suffix: null,
      json_path: 'script',
      meta_attr: 'meta',
      header_field: 'header',
      // follow_url 的 source_layer 仅影响 UI 显示，真正起作用的是 rule.transit_layer
      follow_url: null,
      // [feature 046] script 模式不依赖 source_layer（沙箱求值），保持 null
      script: null,
    }
    const target = impliedLayer[extractorMode]
    if (target && sourceLayer !== target) {
      setSourceLayer(target)
    }
    // json_path 默认 script_index=0（首次切换时）
    if (extractorMode === 'json_path' && scriptIndex === null) {
      setScriptIndex(0)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [extractorMode])

  const nameValid = NAME_REGEX.test(name)
  const ruleValid = useMemo(() => isRuleValid(rule), [rule])
  const canSubmit = nameValid && ruleValid

  async function handleProbe() {
    if (!probeUrl) {
      setProbeError('请填写 URL')
      return
    }
    setProbing(true)
    setProbeResult(null)
    setProbeError(null)
    try {
      const res = await crawlerApi.runFieldProbe({
        url: probeUrl,
        user_agent: userAgent,
        proxy,
        source_layer: sourceLayer,
        rule,
        post_processors: postProcessors,
        script_index: scriptIndex,
        parent_hits: [],
        require_parent: false,
        // US2: 当编辑子字段时传 parent_node_id，由 handler 查表填充 parent_field
        parent_node_id: parentNodeId ?? null,
        per_parent_sample_limit: parentNodeId ? 3 : null,
      })
      if (res.success && res.data) {
        setProbeResult(res.data)
      } else {
        setProbeError(res.message ?? '验证失败')
      }
    } catch (e: unknown) {
      const err = e as { response?: { data?: { message?: string } }; message?: string }
      setProbeError(err.response?.data?.message ?? err.message ?? '验证失败')
    } finally {
      setProbing(false)
    }
  }

  async function handleSubmit() {
    if (!canSubmit) return
    setSubmitting(true)
    setSubmitError(null)
    const body: CreateFieldNodeBody = {
      parent_id: parentNodeId ?? null,
      scope,
      name,
      display_name: derivedDisplayName,
      field_type: fieldType,
      source_layer: sourceLayer,
      extractor_mode: extractorMode,
      rule,
      post_processors: postProcessors,
      script_index: scriptIndex,
      is_active: isActive,
      // [feature 046] 仅 script 模式发送 refresh_on_read（其他模式后端会拒绝 true）
      refresh_on_read: extractorMode === 'script' ? refreshOnRead : false,
    }
    try {
      if (isEdit && initial?.id) {
        await crawlerApi.updateFieldNode(taskId, initial.id, body)
      } else {
        await crawlerApi.createFieldNode(taskId, body)
      }
      onSaved()
    } catch (e: unknown) {
      const err = e as { response?: { data?: { message?: string } }; message?: string }
      setSubmitError(err.response?.data?.message ?? err.message ?? '保存失败')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open={open}
      title={isEdit ? `编辑字段：${initial?.name}` : '新增字段'}
      width={1080}
      onCancel={onCancel}
      destroyOnClose
      footer={[
        <Button key="cancel" onClick={onCancel}>
          取消
        </Button>,
        <Button key="probe" loading={probing} onClick={handleProbe} disabled={!probeUrl}>
          验证规则
        </Button>,
        <Button
          key="save"
          type="primary"
          loading={submitting}
          onClick={handleSubmit}
          disabled={!canSubmit}
        >
          {isEdit ? '更新' : '确认'}
        </Button>,
      ]}
    >
      <Form layout="vertical">
        {submitError && (
          <Alert
            type="error"
            showIcon
            message="保存失败"
            description={submitError}
            style={{ marginBottom: 12 }}
          />
        )}
        <Row gutter={16}>
          {/* 左：验证 URL + 验证结果（命中样本可滚动） */}
          <Col span={12}>
            <Card
              size="small"
              title={<Text strong>验证结果</Text>}
              styles={{ body: { padding: 12, maxHeight: '62vh', overflow: 'auto' } }}
            >
              <Form.Item
                label="验证 URL（覆盖任务 list_urls 第一个）"
                style={{ marginBottom: 12 }}
              >
                <Input value={probeUrl} onChange={(e) => setProbeUrl(e.target.value)} />
              </Form.Item>
              {probeError && (
                <Alert
                  type="error"
                  showIcon
                  message="验证失败"
                  description={probeError}
                  style={{ marginBottom: 12 }}
                />
              )}
              {probeResult && (
                <Alert
                  type="success"
                  showIcon
                  style={{ marginBottom: 12 }}
                  message={`命中 ${probeResult.hit_count} 条（耗时 ${probeResult.duration_ms} ms）`}
                  description={
                    probeResult.per_parent && probeResult.per_parent.length > 0 ? (
                      <PerParentResultList perParent={probeResult.per_parent} />
                    ) : (
                      <List
                        size="small"
                        dataSource={probeResult.samples.slice(0, 10)}
                        renderItem={(s, i) => (
                          <List.Item>
                            <Space direction="vertical" size={0} style={{ width: '100%' }}>
                              <Space>
                                <Tag color="blue">#{i}</Tag>
                                <Text type="secondary" code>
                                  {s.source_fragment}
                                </Text>
                                {s.location && (
                                  <Text type="secondary" style={{ fontSize: 11 }}>
                                    @ {s.location}
                                  </Text>
                                )}
                              </Space>
                              <Paragraph
                                style={{ margin: 0, wordBreak: 'break-all' }}
                                copyable={{ text: s.value }}
                              >
                                {s.value.length > 200 ? s.value.slice(0, 200) + '…' : s.value}
                              </Paragraph>
                            </Space>
                          </List.Item>
                        )}
                      />
                    )
                  }
                />
              )}
              {!probeError && !probeResult && (
                <div style={{ padding: 28, textAlign: 'center' }}>
                  <Text type="secondary">
                    配好右侧规则后点击底部「验证规则」按钮，命中样本会显示在此处
                  </Text>
                </div>
              )}
            </Card>
          </Col>

          {/* 右：配置项 */}
          <Col span={12}>
            <Row gutter={12}>
              <Col span={16}>
                <Form.Item
                  label="字段名"
                  required
                  validateStatus={name && !nameValid ? 'error' : ''}
                  help={
                    name && !nameValid ? (
                      '小写字母开头，1-32 字符，仅小写字母/数字/下划线（后端 JSON key 必须英文）'
                    ) : (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        按中文名或英文 key 搜索；选择预置字段会自动带出类型与匹配模式
                      </Text>
                    )
                  }
                >
                  <Select
                    showSearch
                    value={name || undefined}
                    onChange={(val: string) => handleNamePick(val)}
                    options={nameOptions}
                    optionFilterProp="label"
                    placeholder="搜索字段中文名或英文 key"
                    allowClear
                    style={{ width: '100%' }}
                  />
                </Form.Item>
              </Col>
              <Col span={8}>
                <Form.Item
                  label="字段类型"
                  tooltip="决定后端如何处理该字段提取出的值（是否绝对化 URL、抓详情、上传图床、翻页）"
                >
                  <Select
                    value={fieldType}
                    onChange={(v) => setFieldType(v)}
                    optionRender={(option) => {
                      const t = option.value as FieldType
                      const hint = FIELD_TYPE_HINTS[t]
                      return (
                        <Space direction="vertical" size={0} style={{ padding: '4px 0' }}>
                          <Text strong style={{ fontSize: 13 }}>{FIELD_TYPE_LABELS[t]}</Text>
                          {hint && (
                            <Text type="secondary" style={{ fontSize: 11 }}>{hint}</Text>
                          )}
                        </Space>
                      )
                    }}
                  >
                    {FIELD_TYPES.map((t) => (
                      <Select.Option key={t} value={t}>
                        {FIELD_TYPE_LABELS[t]}
                      </Select.Option>
                    ))}
                  </Select>
                </Form.Item>
              </Col>
            </Row>

            <Row gutter={12}>
              <Col span={8}>
                <Form.Item label="作用域">
                  <Select value={scope} disabled>
                    <Select.Option value={scope}>{scope}</Select.Option>
                  </Select>
                </Form.Item>
              </Col>
              <Col span={8}>
                <Form.Item label="源码层">
                  <Select value={sourceLayer} onChange={(v) => setSourceLayer(v)}>
                    {SOURCE_LAYERS.map((s) => (
                      <Select.Option key={s} value={s}>
                        {SOURCE_LAYER_LABELS[s]}
                      </Select.Option>
                    ))}
                  </Select>
                </Form.Item>
              </Col>
              <Col span={8}>
                <Form.Item label="匹配模式">
                  <Select value={extractorMode} onChange={(v) => setExtractorMode(v)}>
                    {EXTRACTOR_MODES.map((m) => (
                      <Select.Option key={m} value={m}>
                        {EXTRACTOR_MODE_LABELS[m]}
                      </Select.Option>
                    ))}
                  </Select>
                </Form.Item>
              </Col>
            </Row>

            {(sourceLayer === 'script' || extractorMode === 'json_path') && (
              <Form.Item
                label="脚本块索引（与左侧 Script tab 中 #N 对应）"
                help={
                  extractorMode === 'json_path'
                    ? 'json_path 模式：脚本块需含合法 JSON（或 window.__DATA__={...} 形式）'
                    : undefined
                }
              >
                <InputNumber
                  value={scriptIndex ?? undefined}
                  onChange={(v) => setScriptIndex(v ?? null)}
                  min={0}
                  placeholder="如 0"
                  style={{ width: 200 }}
                />
              </Form.Item>
            )}

            <RuleEditor
              rule={rule}
              onChange={setRule}
              siblingFieldNames={(siblingFieldNames ?? []).filter((n) => n !== name)}
            />

            <PostProcessorsEditor value={postProcessors} onChange={setPostProcessors} />

            <Form.Item label="启用">
              <Switch checked={isActive} onChange={(v) => setIsActive(v)} />
            </Form.Item>

            {/* [feature 046] refresh_on_read 仅在 script 模式下启用 */}
            <Form.Item
              label="按需刷新"
              tooltip="仅 script 模式可用。开启后：消费性读取（推送/资源提取）命中此字段时按需重跑脚本；管理性读取（列表/详情/字段命中率面板）始终直接读库。适用于时效性数据（如签名 URL、倒计时）"
            >
              <Switch
                checked={refreshOnRead}
                onChange={setRefreshOnRead}
                disabled={extractorMode !== 'script'}
              />
              {extractorMode !== 'script' && (
                <Text type="secondary" style={{ marginLeft: 8, fontSize: 11 }}>
                  （仅 script 模式可开启）
                </Text>
              )}
            </Form.Item>
          </Col>
        </Row>
      </Form>
    </Modal>
  )
}

// ===================== Rule Editor =====================

function RuleEditor({
  rule,
  onChange,
  siblingFieldNames,
}: {
  rule: FieldRule
  onChange: (r: FieldRule) => void
  siblingFieldNames?: string[]
}) {
  switch (rule.mode) {
    case 'css':
      return (
        <>
          <Form.Item
            label="CSS 选择器（selector）"
            tooltip="匹配页面上一个或多个元素，每个命中的元素提取一个值"
          >
            <Input
              value={rule.spec.selector}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, selector: e.target.value } })
              }
              placeholder="如 .post-title 或 a.bg-white.card-hover"
            />
          </Form.Item>
          <Form.Item
            label="提取内容（attr）"
            tooltip="从每个命中的元素里取什么：text=纯文本｜html=含标签｜href=链接URL｜src=图片URL｜或任意 HTML 属性名（如懒加载图片常用的 data-src）"
            help="取链接填 href，取文字填 text，取图片填 src；下拉可搜索或直接输入任意属性名（data-src 等）后回车保存"
          >
            <AutoComplete
              value={rule.spec.attr}
              onChange={(val) =>
                onChange({ ...rule, spec: { ...rule.spec, attr: val } })
              }
              options={ATTR_OPTIONS}
              filterOption={(input, option) =>
                (option?.value ?? '').toLowerCase().includes(input.toLowerCase())
              }
              placeholder="如 href（取链接）/ text（取文字）/ src 或 data-src（取图片）"
              style={{ width: '100%' }}
              allowClear
            />
          </Form.Item>
        </>
      )
    case 'regex':
      return (
        <Form.Item label="正则规则">
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="pattern"
              value={rule.spec.pattern}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, pattern: e.target.value } })
              }
              placeholder="如 发布时间：(\S+)"
            />
            <Space>
              <InputNumber
                addonBefore="group"
                value={rule.spec.group}
                onChange={(v) =>
                  onChange({ ...rule, spec: { ...rule.spec, group: Number(v ?? 0) } })
                }
                min={0}
              />
              <Input
                addonBefore="flags"
                value={rule.spec.flags ?? ''}
                onChange={(e) =>
                  onChange({ ...rule, spec: { ...rule.spec, flags: e.target.value } })
                }
                placeholder="如 i (大小写不敏感)"
                style={{ width: 200 }}
              />
            </Space>
          </Space>
        </Form.Item>
      )
    case 'prefix_suffix':
      return (
        <Form.Item label="前后缀规则">
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="prefix"
              value={rule.spec.prefix}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, prefix: e.target.value } })
              }
              placeholder="如 <title>"
            />
            <Input
              addonBefore="suffix"
              value={rule.spec.suffix}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, suffix: e.target.value } })
              }
              placeholder="如 </title>"
            />
            <Space>
              <Switch
                checkedChildren="含边界"
                unCheckedChildren="不含边界"
                checked={rule.spec.include_boundary ?? false}
                onChange={(v) =>
                  onChange({ ...rule, spec: { ...rule.spec, include_boundary: v } })
                }
              />
              <Switch
                checkedChildren="区分大小写"
                unCheckedChildren="不区分"
                checked={rule.spec.case_sensitive ?? false}
                onChange={(v) =>
                  onChange({ ...rule, spec: { ...rule.spec, case_sensitive: v } })
                }
              />
            </Space>
          </Space>
        </Form.Item>
      )
    case 'json_path':
      return (
        <Form.Item label="JSON Path 规则">
          <Input
            addonBefore="path"
            value={rule.spec.path}
            onChange={(e) => onChange({ ...rule, spec: { path: e.target.value } })}
            placeholder="如 $.data.list[*].title"
          />
        </Form.Item>
      )
    case 'meta_attr':
      return (
        <Form.Item label="Meta 属性规则">
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="attr_name"
              value={rule.spec.attr_name}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, attr_name: e.target.value } })
              }
              placeholder="如 name | property | http-equiv"
            />
            <Input
              addonBefore="attr_value"
              value={rule.spec.attr_value}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, attr_value: e.target.value } })
              }
              placeholder="如 description | og:title"
            />
            <Input
              addonBefore="content_key"
              value={rule.spec.content_key ?? 'content'}
              onChange={(e) =>
                onChange({ ...rule, spec: { ...rule.spec, content_key: e.target.value } })
              }
              placeholder="默认 content"
            />
          </Space>
        </Form.Item>
      )
    case 'header_field':
      return (
        <Form.Item label="HTTP Header 规则">
          <Input
            addonBefore="header_name"
            value={rule.spec.header_name}
            onChange={(e) =>
              onChange({ ...rule, spec: { ...rule.spec, header_name: e.target.value } })
            }
            placeholder="如 X-Total-Count | Content-Type"
          />
        </Form.Item>
      )
    case 'follow_url':
      return <FollowUrlRuleEditor value={rule} onChange={onChange} />
    case 'script':
      return (
        <ScriptRuleEditor
          value={rule}
          onChange={onChange}
          siblingFieldNames={siblingFieldNames ?? []}
        />
      )
  }
}

/** 简单的子规则必填校验（SubRule 6 同步模式，不含 follow_url） */
function isSubRuleValid(sub: SubRule): boolean {
  switch (sub.mode) {
    case 'css':
      return sub.spec.selector.trim().length > 0
    case 'regex':
      return sub.spec.pattern.trim().length > 0
    case 'prefix_suffix':
      return sub.spec.prefix.length > 0 && sub.spec.suffix.length > 0
    case 'json_path':
      return sub.spec.path.trim().startsWith('$')
    case 'meta_attr':
      return sub.spec.attr_name.trim().length > 0 && sub.spec.attr_value.trim().length > 0
    case 'header_field':
      return sub.spec.header_name.trim().length > 0
  }
}

/** 简单的规则必填校验 */
function isRuleValid(rule: FieldRule): boolean {
  switch (rule.mode) {
    case 'css':
      return rule.spec.selector.trim().length > 0
    case 'regex':
      return rule.spec.pattern.trim().length > 0
    case 'prefix_suffix':
      return rule.spec.prefix.length > 0 && rule.spec.suffix.length > 0
    case 'json_path':
      return rule.spec.path.trim().startsWith('$')
    case 'meta_attr':
      return rule.spec.attr_name.trim().length > 0 && rule.spec.attr_value.trim().length > 0
    case 'header_field':
      return rule.spec.header_name.trim().length > 0
    case 'follow_url':
      return isSubRuleValid(rule.spec.transit) && isSubRuleValid(rule.spec.extract)
    case 'script':
      // [feature 046] body 非空 + 长度 ≤ 64KB（与后端 max_body_size 对齐）
      return rule.spec.body.trim().length > 0 && rule.spec.body.length <= 65536
  }
}

// ===================== Script Rule Editor (feature 046) =====================

/** [feature 046] JS 沙箱脚本编辑器：textarea + 入参签名提示 */
function ScriptRuleEditor({
  value,
  onChange,
  siblingFieldNames = [],
}: {
  value: ScriptRule
  onChange: (r: ScriptRule) => void
  /** [US2] 当前作用域已存在的兄弟字段名（点击插入到 body 末尾） */
  siblingFieldNames?: string[]
}) {
  const body = value.spec.body
  const oversized = body.length > 65536
  // [T057 FR-004] dry-run 语法检查结果
  const [dryRun, setDryRun] = useState<
    | { status: 'ok' | 'error'; message: string; line?: number; column?: number }
    | null
  >(null)
  const [dryRunLoading, setDryRunLoading] = useState(false)

  const appendSnippet = (snippet: string) => {
    onChange({ ...value, spec: { ...value.spec, body: body + snippet } })
  }

  // [T057] 前端语法 dry-run：用浏览器 `new Function('ctx', body)` 构造一次
  // 不真正执行，只验证语法可解析（与后端 rquickjs ES2020 子集语义不完全一致，
  // 但能捕获大部分 SyntaxError，作为保存前的早期反馈）
  const runDryRun = () => {
    if (!body.trim()) {
      setDryRun({ status: 'error', message: 'body 不能为空' })
      return
    }
    setDryRunLoading(true)
    // setTimeout 0 让按钮 loading 状态先渲染
    setTimeout(() => {
      try {
        // eslint-disable-next-line no-new-func
        new Function('ctx', body)
        setDryRun({ status: 'ok', message: '语法检查通过（注意：实际语义以后端 rquickjs 求值为准）' })
      } catch (e: any) {
        // SyntaxError 通常含行号/列号信息（V8 格式：<msg> (<function>:<line>:<col>)）
        const msg = e?.message ?? String(e)
        const stack = e?.stack ?? ''
        const m = stack.match(/<anonymous>:(\d+):(\d+)/)
        setDryRun({
          status: 'error',
          message: msg,
          line: m ? Number(m[1]) : undefined,
          column: m ? Number(m[2]) : undefined,
        })
      } finally {
        setDryRunLoading(false)
      }
    }, 0)
  }

  return (
    <>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 12 }}
        message="JS 沙箱（rquickjs）求值"
        description={
          <div style={{ fontSize: 12 }}>
            <div>
              脚本被包裹为 <Text code>(function(ctx)&#123; ... &#125;)</Text> 求值。必须{' '}
              <Text code>return</Text> 字符串/数字/布尔，否则视为 TypeError。
            </div>
            <div style={{ marginTop: 4 }}>
              <Text code>ctx.value</Text> 6 模式提取的原始值（未匹配时为空串） ·{' '}
              <Text code>ctx.fields.{'<name>'}</Text> 同作用域兄弟字段 ·{' '}
              <Text code>ctx.url</Text> 当前详情页 URL
            </div>
            <div style={{ marginTop: 4 }}>
              限制：≤64KB · 100ms 超时 · 内存 16MB · 禁用 Function/eval/process/setTimeout ·
              NetworkError 在 ctx.fetch（US3）场景下触发
            </div>
            {siblingFieldNames.length > 0 && (
              <div style={{ marginTop: 6 }}>
                <Text type="secondary">当前作用域可用的兄弟字段（点击插入）：</Text>{' '}
                <Space size={4} wrap>
                  {siblingFieldNames.map((n) => (
                    <Tag
                      key={n}
                      color="blue"
                      style={{ cursor: 'pointer', marginInlineEnd: 0 }}
                      onClick={() => appendSnippet(`ctx.fields.${n}`)}
                    >
                      ctx.fields.{n}
                    </Tag>
                  ))}
                </Space>
                <div style={{ marginTop: 4 }}>
                  <Text type="warning" style={{ fontSize: 11 }}>
                    注意：「验证规则」按钮只跑当前字段，ctx.fields 在验证时为空对象 —
                    若脚本依赖兄弟字段（如 return ctx.fields.X），验证会返回 type_error；
                    请改用任务「test_run」端到端验证，或加 `|| ctx.value` 兜底。
                  </Text>
                </div>
              </div>
            )}
          </div>
        }
      />
      <Form.Item
        label="脚本函数体（body）"
        validateStatus={oversized ? 'error' : undefined}
        help={oversized ? `body 长度 ${body.length} 超过 64KB 上限` : `${body.length} / 65536 字节`}
      >
        <Input.TextArea
          value={body}
          onChange={(e) => {
            onChange({ ...value, spec: { ...value.spec, body: e.target.value } })
            if (dryRun) setDryRun(null) // 编辑后清空 dry-run 结果
          }}
          autoSize={{ minRows: 8, maxRows: 24 }}
          style={{ fontFamily: 'monospace', fontSize: 12 }}
          placeholder="// 例：return ctx.value.toUpperCase()&#10;// 或解析兄弟字段：return ctx.fields.pan_type === 'quark' ? ctx.value + '#quark' : ctx.value"
        />
      </Form.Item>
      <Space direction="vertical" size={4} style={{ width: '100%', marginBottom: 12 }}>
        <Space>
          <Button
            size="small"
            icon={<SafetyCertificateOutlined />}
            loading={dryRunLoading}
            onClick={runDryRun}
            disabled={oversized}
          >
            语法 dry-run（前端）
          </Button>
          <Text type="secondary" style={{ fontSize: 11 }}>
            保存前用浏览器 V8 引擎快速验证语法（与后端 rquickjs ES2020 子集略有差异）
          </Text>
        </Space>
        {dryRun && (
          <Alert
            type={dryRun.status === 'ok' ? 'success' : 'error'}
            showIcon
            style={{ padding: '4px 12px', fontSize: 12 }}
            message={
              <span>
                {dryRun.line != null && dryRun.column != null && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    行 {dryRun.line - 1}（body 内）:{dryRun.column} —{' '}
                  </Text>
                )}
                {dryRun.message}
              </span>
            }
          />
        )}
      </Space>
    </>
  )
}

// ===================== PostProcessors Editor =====================

function PostProcessorsEditor({
  value,
  onChange,
}: {
  value: PostProcessor[]
  onChange: (v: PostProcessor[]) => void
}) {
  return (
    <Form.Item label="后处理链（按顺序执行）">
      <Space direction="vertical" style={{ width: '100%' }} size={4}>
        {value.map((p, i) => (
          <Space key={i} align="center">
            <Text type="secondary" style={{ fontSize: 11 }}>
              #{i + 1}
            </Text>
            <Select
              value={p.op}
              onChange={(op) => {
                const next = [...value]
                next[i] = { op }
                onChange(next)
              }}
              style={{ width: 180 }}
              options={POST_PROCESSOR_OPS.map((op) => ({ value: op, label: POST_PROCESSOR_OP_LABELS[op] }))}
            />
            <Button
              type="text"
              icon={<MinusCircleOutlined />}
              onClick={() => onChange(value.filter((_, idx) => idx !== i))}
            />
          </Space>
        ))}
        <Button
          type="dashed"
          icon={<PlusOutlined />}
          onClick={() => onChange([...value, { op: 'trim' }])}
          size="small"
        >
          添加后处理步骤
        </Button>
      </Space>
    </Form.Item>
  )
}

// ===================== US2: 按父命中渲染验证结果 =====================

/**
 * PerParentResultList — 子字段验证时，按父命中分组渲染。
 *
 * 每条父命中一张卡片：标注子字段在该作用域下的值或"未命中"。
 * 设计参考 spec.md US2 acceptance 4："验证返回'每条父命中 → 对应子字段值'的结构化结果"。
 */
function PerParentResultList({ perParent }: { perParent: PerParentSample[] }) {
  const hitCount = perParent.filter((p) => p.child_hit).length
  return (
    <Space direction="vertical" size={6} style={{ width: '100%' }}>
      <Space size={4}>
        <Tag color="blue">父命中 {perParent.length}</Tag>
        <Tag color="green">子命中 {hitCount}</Tag>
        {hitCount < perParent.length && (
          <Tag color="orange">未命中 {perParent.length - hitCount}</Tag>
        )}
      </Space>
      {perParent.map((p) => (
        <PerParentCard key={p.parent_index} item={p} />
      ))}
    </Space>
  )
}

function PerParentCard({ item }: { item: PerParentSample }) {
  const childSamples = item.child_samples ?? []
  return (
    <div
      style={{
        padding: 8,
        border: `1px solid ${item.child_hit ? '#d9f7be' : '#ffccc7'}`,
        borderRadius: 6,
        background: item.child_hit ? '#f6ffed' : '#fff2f0',
      }}
    >
      <Space size={6} wrap style={{ marginBottom: 4 }}>
        <Tag color="cyan">父 #{item.parent_index}</Tag>
        {item.child_hit ? (
          <Tag color="green">命中</Tag>
        ) : (
          <Tag color="red">未命中</Tag>
        )}
      </Space>
      <div style={{ marginBottom: 6 }}>
        <Text type="secondary" style={{ fontSize: 11 }}>
          父作用域片段：
        </Text>
        <Text
          type="secondary"
          style={{ fontSize: 11, wordBreak: 'break-all', marginLeft: 4 }}
        >
          {item.parent_fragment.length > 120
            ? item.parent_fragment.slice(0, 120) + '…'
            : item.parent_fragment}
        </Text>
      </div>
      {item.child_hit ? (
        <Space direction="vertical" size={4} style={{ width: '100%' }}>
          {childSamples.map((s, i) => (
            <Space key={i} direction="vertical" size={0} style={{ width: '100%' }}>
              <Space>
                <Tag color="blue">子 #{i}</Tag>
                <Text type="secondary" code style={{ fontSize: 11 }}>
                  {s.source_fragment}
                </Text>
                {s.location && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    @ {s.location}
                  </Text>
                )}
              </Space>
              <Paragraph
                style={{ margin: 0, wordBreak: 'break-all' }}
                copyable={{ text: s.value }}
              >
                {s.value.length > 200 ? s.value.slice(0, 200) + '…' : s.value}
              </Paragraph>
            </Space>
          ))}
        </Space>
      ) : (
        <Text type="secondary" italic>
          子字段在此父作用域下未命中（检查规则是否在该片段内有效）
        </Text>
      )}
    </div>
  )
}
