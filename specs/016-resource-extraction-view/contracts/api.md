# API Contract: 资源管理查看提取对比

**Date**: 2026-06-08
**Feature**: 016-resource-extraction-view

## 新增接口

### GET /api/resources/{id}/detail

获取资源详情 + 关联的原始消息内容，用于查看提取对比。

**认证**: Bearer Token (Admin 角色及以上)

**路径参数**:

| 参数 | 类型 | 说明 |
|------|------|------|
| `id` | `i64` | 资源记录 ID |

**成功响应** (200):

```json
{
  "success": true,
  "data": {
    "resource": {
      "id": 1,
      "collector_history_id": 42,
      "title": "某部电影 [4K]",
      "url": "https://pan.quark.cn/s/abc123,https://pan.baidu.com/s/def456",
      "description": "4K 蓝光高清版本",
      "category": "quark",
      "tags": "电影,4K",
      "img": "photo_123456",
      "source": "tg",
      "extra": null,
      "extract_mode": "ai",
      "is_pushed": false,
      "is_edited": false,
      "created_at": "2026-06-08T10:30:00",
      "updated_at": "2026-06-08T10:30:00"
    },
    "raw_text": "名称：某部电影 [4K]\n描述：4K 蓝光高清版本\n链接：\n夸克网盘：https://pan.quark.cn/s/abc123\n百度网盘：https://pan.baidu.com/s/def456",
    "raw_data": "{\"text\":\"名称：某部电影...\",\"media_type\":\"photo\",\"photo_id\":\"123456\"}",
    "media_type": "photo",
    "has_history": true
  }
}
```

**采集历史不存在时的响应** (200):

```json
{
  "success": true,
  "data": {
    "resource": { "...同上..." },
    "raw_text": null,
    "raw_data": null,
    "media_type": null,
    "has_history": false
  }
}
```

**资源不存在** (404):

```json
{
  "success": false,
  "error": "资源不存在"
}
```

**未认证** (401):

```json
{
  "success": false,
  "error": "未认证"
}
```

**权限不足** (403):

```json
{
  "success": false,
  "error": "权限不足"
}
```

## 现有相关接口（不修改）

| 接口 | 方法 | 说明 |
|------|------|------|
| `/api/resources` | GET | 资源列表（现有，不修改） |
| `/api/resources/{id}` | GET | 单条资源（现有，不修改） |
| `/api/resources/{id}` | PUT | 编辑资源（现有，不修改） |
| `/api/resources/{id}` | DELETE | 删除资源（现有，不修改） |
