# Research: 资源管理查看提取对比

**Date**: 2026-06-08
**Feature**: 016-resource-extraction-view

## Decision 1: API 接口设计

**Decision**: 新增 `GET /api/resources/{id}/detail` 接口，返回资源详情 + 关联的原始消息内容

**Rationale**:
- 现有 `GET /api/resources/{id}` 仅返回 ExtractedResource 本身，不包含原始消息
- 新接口通过 `collector_history_id` 联查 `collector_histories` 表获取 `raw_data`
- 使用独立路径 `/detail` 而非扩展现有接口，避免破坏现有 API 契约
- 返回结构包含 `resource`（提取结果）和 `raw_message`（原始文本），前端可直接使用

**Alternatives considered**:
- 扩展现有 `GET /resources/{id}` 增加 `raw_data` 字段 → 会破坏现有返回结构，影响其他调用方
- 前端分别调用两个接口（资源 + 历史记录）→ 需要额外暴露采集历史查询接口，且增加前端复杂度

## Decision 2: 前端弹窗实现策略

**Decision**: 在 `Resources.tsx` 中直接实现对比弹窗，参考 `CollectorHistory.tsx` 的样式结构

**Rationale**:
- CollectorHistory.tsx 的提取弹窗（第 239-587 行）已经是成熟的左右分栏布局
- 查看弹窗是其简化版本（只读、无提取操作），代码量更少
- 在 Resources.tsx 中内联实现，避免跨页面组件抽取的复杂度
- 保持 YAGNI 原则，不做过度抽象

**Alternatives considered**:
- 抽取为独立共享组件 → 当前仅两个页面使用，且 CollectorHistory 的弹窗有复杂的状态管理（提取模式选择、dry_run 等），抽取成本高于收益
- 复制 CollectorHistory 弹窗代码后精简 → 这正是计划方案，去掉提取逻辑只保留展示

## Decision 3: 数据库查询策略

**Decision**: 使用单次 JOIN 查询获取资源 + 原始消息

**Rationale**:
- 一次查询即可获取所有数据，避免两次网络往返
- LEFT JOIN 保证即使采集历史不存在，资源数据仍能返回
- 查询简单，无性能风险

**SQL 模式**:
```sql
SELECT er.*, ch.raw_data, ch.remote_id
FROM extracted_resources er
LEFT JOIN collector_histories ch ON er.collector_history_id = ch.id
WHERE er.id = ?
```

**Alternatives considered**:
- 分两次查询（先查资源，再查历史）→ 增加网络往返和前端复杂度
- 在 ExtractedResource 模型中冗余存储 raw_data → 数据重复，违背数据规范化

## Decision 4: raw_data 解析策略

**Decision**: 后端解析 raw_data JSON 提取 text 字段和 media_type，前端直接展示

**Rationale**:
- raw_data 是 JSON 格式，包含 `text`、`media_type`、`photo_id` 等字段
- 与现有 extract_single_record 逻辑一致（resource.rs 第 289-306 行），复用相同的解析模式
- 后端返回 `raw_text`（纯文本）和 `raw_json`（原始 JSON），前端按需展示

**Alternatives considered**:
- 前端直接解析 raw_data → 前后端都需要解析逻辑，违反 DRY
- 仅返回原始 JSON 由前端处理 → 前端需要了解 raw_data 内部结构
