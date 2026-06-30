/*!
 * follow_url 模式 rule 编辑器（feature 043-crawler-configurator 扩展）
 *
 * 两段式 UI：
 *   - 上半：transit 子规则（在当前 material 提取中转 URL）
 *   - 下半：extract 子规则（在二次请求响应上提取最终值）
 *
 * SubRuleEditor 独立实现 6 模式表单，不复用 FieldNodeEditor 的 RuleEditor，
 * 避免 follow_url 嵌套递归（SubRule 类型保证不会出现 follow_url）。
 */
import { Card, Form, Input, InputNumber, Select, Space, Switch, Typography } from 'antd'
import type { FollowUrlRule, SourceLayer, SubRule } from '../../types'

const { Text } = Typography

const SOURCE_LAYERS: SourceLayer[] = ['html', 'header', 'script', 'meta', 'url']

const SOURCE_LAYER_LABELS: Record<SourceLayer, string> = {
  html: 'HTML 源码',
  header: '响应头',
  script: '脚本块',
  meta: 'Meta 标签',
  url: 'URL 本身',
}

/** 6 模式选择（SubRule 不含 follow_url） */
const SUB_MODES: SubRule['mode'][] = [
  'css',
  'regex',
  'prefix_suffix',
  'json_path',
  'meta_attr',
  'header_field',
]

const SUB_MODE_LABELS: Record<SubRule['mode'], string> = {
  css: 'CSS 选择器',
  regex: '正则匹配',
  prefix_suffix: '前后缀匹配',
  json_path: 'JSON Path',
  meta_attr: 'Meta 属性',
  header_field: '响应头字段',
}

/** 默认 SubRule（按 mode） */
function defaultSubRule(mode: SubRule['mode']): SubRule {
  switch (mode) {
    case 'css':
      return { mode: 'css', spec: { selector: '', attr: 'href' } }
    case 'regex':
      return { mode: 'regex', spec: { pattern: '', group: 1, flags: '' } }
    case 'prefix_suffix':
      return {
        mode: 'prefix_suffix',
        spec: { prefix: '', suffix: '', include_boundary: false, case_sensitive: false },
      }
    case 'json_path':
      return { mode: 'json_path', spec: { path: '$.' } }
    case 'meta_attr':
      return { mode: 'meta_attr', spec: { attr_name: 'name', attr_value: '', content_key: 'content' } }
    case 'header_field':
      return { mode: 'header_field', spec: { header_name: '' } }
  }
}

/** 6 模式表单（独立于 RuleEditor，避免循环依赖） */
function SubRuleEditor({
  value,
  onChange,
}: {
  value: SubRule
  onChange: (s: SubRule) => void
}) {
  return (
    <>
      <Form.Item label="匹配模式" style={{ marginBottom: 12 }}>
        <Select
          value={value.mode}
          onChange={(m) => onChange(defaultSubRule(m))}
          options={SUB_MODES.map((m) => ({ value: m, label: SUB_MODE_LABELS[m] }))}
          style={{ width: 200 }}
        />
      </Form.Item>
      {value.mode === 'css' && (
        <>
          <Form.Item label="CSS 选择器">
            <Input
              value={value.spec.selector}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, selector: e.target.value } })}
              placeholder="如 a.download 或 .dl-btn"
            />
          </Form.Item>
          <Form.Item label="提取内容（attr）" style={{ marginBottom: 0 }}>
            <Input
              value={value.spec.attr}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, attr: e.target.value } })}
              placeholder="href（取链接）/ text（取文字）/ src（取图片）"
            />
          </Form.Item>
        </>
      )}
      {value.mode === 'regex' && (
        <Form.Item label="正则规则" style={{ marginBottom: 0 }}>
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="pattern"
              value={value.spec.pattern}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, pattern: e.target.value } })}
              placeholder="如 https?://pan\\.[a-z]+\\.[a-z]+/s/[\\w-]+"
            />
            <Space>
              <InputNumber
                addonBefore="group"
                value={value.spec.group}
                onChange={(v) => onChange({ ...value, spec: { ...value.spec, group: Number(v ?? 0) } })}
                min={0}
              />
              <Input
                addonBefore="flags"
                value={value.spec.flags ?? ''}
                onChange={(e) => onChange({ ...value, spec: { ...value.spec, flags: e.target.value } })}
                placeholder="如 i"
                style={{ width: 160 }}
              />
            </Space>
          </Space>
        </Form.Item>
      )}
      {value.mode === 'prefix_suffix' && (
        <Form.Item label="前后缀规则" style={{ marginBottom: 0 }}>
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="prefix"
              value={value.spec.prefix}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, prefix: e.target.value } })}
              placeholder="如 下载链接："
            />
            <Input
              addonBefore="suffix"
              value={value.spec.suffix}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, suffix: e.target.value } })}
              placeholder="如 </div>"
            />
            <Space>
              <Switch
                checkedChildren="含边界"
                unCheckedChildren="不含边界"
                checked={value.spec.include_boundary ?? false}
                onChange={(v) => onChange({ ...value, spec: { ...value.spec, include_boundary: v } })}
              />
              <Switch
                checkedChildren="区分大小写"
                unCheckedChildren="不区分"
                checked={value.spec.case_sensitive ?? false}
                onChange={(v) => onChange({ ...value, spec: { ...value.spec, case_sensitive: v } })}
              />
            </Space>
          </Space>
        </Form.Item>
      )}
      {value.mode === 'json_path' && (
        <Form.Item label="JSON Path" style={{ marginBottom: 0 }}>
          <Input
            addonBefore="path"
            value={value.spec.path}
            onChange={(e) => onChange({ ...value, spec: { path: e.target.value } })}
            placeholder="如 $.downloadUrl"
          />
        </Form.Item>
      )}
      {value.mode === 'meta_attr' && (
        <Form.Item label="Meta 属性" style={{ marginBottom: 0 }}>
          <Space direction="vertical" style={{ width: '100%' }}>
            <Input
              addonBefore="attr_name"
              value={value.spec.attr_name}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, attr_name: e.target.value } })}
              placeholder="如 name | property"
            />
            <Input
              addonBefore="attr_value"
              value={value.spec.attr_value}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, attr_value: e.target.value } })}
              placeholder="如 og:download"
            />
            <Input
              addonBefore="content_key"
              value={value.spec.content_key ?? 'content'}
              onChange={(e) => onChange({ ...value, spec: { ...value.spec, content_key: e.target.value } })}
              placeholder="默认 content"
            />
          </Space>
        </Form.Item>
      )}
      {value.mode === 'header_field' && (
        <Form.Item label="HTTP Header" style={{ marginBottom: 0 }}>
          <Input
            addonBefore="header_name"
            value={value.spec.header_name}
            onChange={(e) => onChange({ ...value, spec: { ...value.spec, header_name: e.target.value } })}
            placeholder="如 X-Download-Url"
          />
        </Form.Item>
      )}
    </>
  )
}

/** follow_url 模式主编辑器：transit + extract 两段 */
export default function FollowUrlRuleEditor({
  value,
  onChange,
}: {
  value: FollowUrlRule
  onChange: (r: FollowUrlRule) => void
}) {
  const spec = value.spec
  const patchSpec = (patch: Partial<FollowUrlRule['spec']>) =>
    onChange({ ...value, spec: { ...spec, ...patch } })

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Card
        size="small"
        title={<Text strong>① 抓中转 URL（transit）</Text>}
        extra={<Text type="secondary">在当前页提取下载入口 URL</Text>}
      >
        <Form layout="vertical" size="small">
          <Form.Item label="transit 作用层" style={{ marginBottom: 12 }}>
            <Select
              value={spec.transit_layer ?? 'html'}
              onChange={(v: SourceLayer) => patchSpec({ transit_layer: v })}
              options={SOURCE_LAYERS.map((l) => ({ value: l, label: SOURCE_LAYER_LABELS[l] }))}
              style={{ width: 160 }}
            />
          </Form.Item>
          {(spec.transit_layer ?? 'html') === 'script' && (
            <Form.Item label="transit script_index" style={{ marginBottom: 12 }}>
              <InputNumber
                value={spec.transit_script_index ?? null}
                onChange={(v) => patchSpec({ transit_script_index: v ?? null })}
                min={0}
                placeholder="script 块序号"
                style={{ width: 160 }}
              />
            </Form.Item>
          )}
          <SubRuleEditor
            value={spec.transit}
            onChange={(transit) => patchSpec({ transit })}
          />
        </Form>
      </Card>

      <Card
        size="small"
        title={<Text strong>② 二次请求后提取（extract）</Text>}
        extra={<Text type="secondary">访问中转 URL 后，在其响应上提取最终下载地址</Text>}
      >
        <Form layout="vertical" size="small">
          <Form.Item label="extract 作用层" style={{ marginBottom: 12 }}>
            <Select
              value={spec.target_layer ?? 'html'}
              onChange={(v: SourceLayer) => patchSpec({ target_layer: v })}
              options={SOURCE_LAYERS.map((l) => ({ value: l, label: SOURCE_LAYER_LABELS[l] }))}
              style={{ width: 160 }}
            />
          </Form.Item>
          {(spec.target_layer ?? 'html') === 'script' && (
            <Form.Item label="target script_index" style={{ marginBottom: 12 }}>
              <InputNumber
                value={spec.target_script_index ?? null}
                onChange={(v) => patchSpec({ target_script_index: v ?? null })}
                min={0}
                placeholder="script 块序号"
                style={{ width: 160 }}
              />
            </Form.Item>
          )}
          <SubRuleEditor
            value={spec.extract}
            onChange={(extract) => patchSpec({ extract })}
          />
        </Form>
      </Card>
    </Space>
  )
}
