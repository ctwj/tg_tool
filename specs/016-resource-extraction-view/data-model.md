# Data Model: 资源管理查看提取对比

**Date**: 2026-06-08
**Feature**: 016-resource-extraction-view

## 现有实体（无修改）

### ExtractedResource

已有，无需修改。核心关联字段 `collector_history_id` 用于联查。

### CollectorHistory

已有，无需修改。`raw_data` 字段存储原始消息 JSON。

## 新增 DTO

### ResourceDetailResponse（API 返回结构）

| 字段 | 类型 | 说明 |
|------|------|------|
| `resource` | `ExtractedResource` | 已提取的资源完整信息 |
| `raw_text` | `Option<String>` | 从 raw_data 中提取的纯文本消息内容 |
| `raw_data` | `Option<String>` | 原始 JSON 完整内容（供高级查看） |
| `media_type` | `Option<String>` | 媒体类型（photo/video/document 等） |
| `has_history` | `bool` | 关联的采集历史是否存在 |

**说明**：
- `has_history = false` 时，前端左侧显示"原始消息不可用"
- `raw_text` 是从 `raw_data` JSON 中解析的 `text` 字段，与现有 `extract_single_record` 解析逻辑一致
- `raw_data` 保留原始 JSON 用于可能的扩展查看需求

## 数据流向

```text
用户点击"查看"
  → 前端调用 GET /api/resources/{id}/detail
    → handler::get_resource_detail
      → service::get_resource_with_raw
        → LEFT JOIN extracted_resources + collector_histories
        → 解析 raw_data JSON 提取 text/media_type
      → 返回 ResourceDetailResponse
    → 前端弹出对比弹窗
      → 左侧: raw_text (纯文本展示)
      → 右侧: resource 字段 (标题、链接、描述、分类、标签)
```

## 状态转换

无。本功能为纯只读查询，不涉及状态变更。
