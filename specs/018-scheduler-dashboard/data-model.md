# Data Model: 调度可视化面板

**Date**: 2026-06-09
**Feature**: 018-scheduler-dashboard

## 新增实体

### ExtractHistory（新建表 + 模型）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `i64` | 主键，自增 |
| `status` | `String` | 执行状态：`success` / `failed` |
| `total_scanned` | `i64` | 扫描的采集历史数 |
| `extracted` | `i64` | 成功提取的资源数 |
| `skipped` | `i64` | 跳过数（含去重） |
| `errors` | `i64` | 错误数 |
| `message` | `Option<String>` | 错误信息（status=failed 时） |
| `executed_at` | `NaiveDateTime` | 执行时间 |

**索引**: `idx_extract_histories_executed_at`（按时间倒序查询优化）

**Rust 模型** (`src/models/extract_history.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExtractHistory {
    pub id: i64,
    pub status: String,
    pub total_scanned: i64,
    pub extracted: i64,
    pub skipped: i64,
    pub errors: i64,
    pub message: Option<String>,
    pub executed_at: NaiveDateTime,
}
```

## 现有实体（复用，无修改）

### PushHistory

已有 `push_histories` 表，字段：`id, batch_id, target, status, data_count, message, error_msg, pushed_at`。面板直接复用 `/api/push/histories` 和 `/api/push/stats` 接口。

### SchedulerState / ExtractSchedulerState（内存状态）

已有，需新增 `started_at: Instant` 字段（research Decision 2）用于修正 next_run 计算。

## 新增 DTO

### ExtractHistoryListResponse（API 返回）

| 字段 | 类型 | 说明 |
|------|------|------|
| `list` | `Vec<ExtractHistory>` | 提取历史列表 |
| `pagination` | `PaginationInfo` | 分页信息 |

### ExtractHistoryStats（API 返回，可选聚合）

| 字段 | 类型 | 说明 |
|------|------|------|
| `total` | `i64` | 总执行次数 |
| `success` | `i64` | 成功次数 |
| `failed` | `i64` | 失败次数 |
| `last_extracted` | `i64` | 最近一次成功提取的资源数 |

## 数据流向

```text
[调度状态区]
  /api/status (misc.rs, 修正后)
    → schedulers.push_running / push_next_run
    → schedulers.extract_running / extract_next_run
    → 前端 Scheduler.tsx 每 30 秒轮询
    → 状态卡片（运行状态 + 间隔 + 下次执行时间倒计时）

[推送历史区]
  /api/push/histories (已有)
  /api/push/stats (已有)
    → 前端分页表格 + 统计卡片

[提取历史区]
  scheduler.rs extract tick → extract_history::insert
  /api/extract-histories (新增)
    → 前端分页表格 + 统计卡片
```

## 状态转换

ExtractHistory.status:
- `success` ← 提取执行成功（trigger_extraction 返回 Ok）
- `failed` ← 提取执行失败（trigger_extraction 返回 Err）

无其他状态转换，记录写入后不可变。
