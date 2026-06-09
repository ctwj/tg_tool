# API Contract: 调度可视化面板

**Date**: 2026-06-09
**Feature**: 018-scheduler-dashboard

## 修改的接口

### GET /api/status（修正）

修正 schedulers 块中 `extract_next_run` 和 `push_next_run` 的计算，解决重启后 `last_run_at = None` 导致的倒计时偏差。

**响应**（仅展示变更部分）:
```json
{
  "success": true,
  "data": {
    "schedulers": {
      "extract_running": true,
      "extract_next_run": "2026-06-09 10:30:00",
      "extract_interval_minutes": 30,
      "push_running": true,
      "push_next_run": "2026-06-09 10:45:00",
      "push_interval_minutes": 30,
      "forward_running": true,
      "forward_interval_secs": 2
    }
  }
}
```

**新增字段**: `extract_interval_minutes`、`push_interval_minutes`（前端展示间隔需要）。

---

## 新增接口

### GET /api/extract-histories

查询提取历史记录（分页）。

**认证**: Bearer Token (Admin 角色及以上)

**查询参数**:

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `page` | `i64` | 1 | 页码 |
| `page_size` | `i64` | 20 | 每页条数（clamp 1..100） |

**成功响应** (200):

```json
{
  "success": true,
  "data": {
    "list": [
      {
        "id": 1,
        "status": "success",
        "total_scanned": 100,
        "extracted": 42,
        "skipped": 55,
        "errors": 3,
        "message": null,
        "executed_at": "2026-06-09 10:30:00"
      }
    ],
    "pagination": { "page": 1, "page_size": 20, "total": 50 }
  }
}
```

### GET /api/extract-histories/stats

提取历史统计汇总。

**成功响应** (200):

```json
{
  "success": true,
  "data": {
    "total": 50,
    "success": 47,
    "failed": 3,
    "last_extracted": 42
  }
}
```

---

## 现有相关接口（复用，不修改）

| 接口 | 方法 | 说明 |
|------|------|------|
| `/api/push/histories` | GET | 推送历史分页（已有） |
| `/api/push/stats` | GET | 推送统计汇总（已有） |
| `/api/push/retry` | POST | 重试失败推送（已有，面板可提供入口） |

## 错误响应

**未认证** (401): `{ "success": false, "error": "未认证" }`
**权限不足** (403): `{ "success": false, "error": "权限不足" }`
