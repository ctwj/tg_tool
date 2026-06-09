# Quickstart: 调度可视化面板

**Date**: 2026-06-09
**Feature**: 018-scheduler-dashboard

## 开发步骤

### 1. 后端：数据库迁移（新建 extract_histories 表）

文件：`migrations/009_extract_histories_sqlite.sql` 和 `009_extract_histories_postgres.sql`

```sql
CREATE TABLE extract_histories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,  -- Postgres: BIGSERIAL
    status VARCHAR(20) NOT NULL,
    total_scanned BIGINT DEFAULT 0,
    extracted BIGINT DEFAULT 0,
    skipped BIGINT DEFAULT 0,
    errors BIGINT DEFAULT 0,
    message TEXT,
    executed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_extract_histories_executed_at ON extract_histories(executed_at);
```

### 2. 后端：模型 + service

- `src/models/extract_history.rs` — ExtractHistory 结构体
- `src/services/extract_history.rs` — `insert(db, ...)` 和 `list(db, page, page_size)` + `stats(db)`
- 在 `mod.rs` 注册模块

### 3. 后端：scheduler 写入提取历史

文件：`src/services/scheduler.rs` extract tick（约 L234）

提取执行后（成功/失败均记录），调用 `extract_history::insert`。

### 4. 后端：修正 next_run 计算

文件：`src/services/scheduler.rs` + `src/handlers/misc.rs`

- SchedulerState / ExtractSchedulerState 新增 `started_at: Instant`
- tick 循环启动时记录 started_at
- misc.rs 的 next_run 计算改用 `last_run_at.or(started_at)` 作为基准

### 5. 后端：新增 handler + 路由

文件：`src/handlers/scheduler.rs`

- `list_extract_histories` handler（分页）
- `get_extract_histories_stats` handler（统计）
- routes.rs 注册 `GET /extract-histories` 和 `GET /extract-histories/stats`

### 6. 后端：集成测试

文件：`tests/api_integration.rs`

- `test_extract_histories_empty` — 空表查询
- `test_extract_histories_list` — 插入后分页查询
- `test_extract_histories_stats` — 统计聚合正确
- `test_status_next_run_after_restart` — 验证修正后的 next_run 计算

### 7. 前端：类型定义 + 页面

- `web/src/types/index.ts` — 新增 ExtractHistory、ExtractHistoryStats、SchedulerStatus 类型
- `web/src/pages/Scheduler.tsx` — 新建页面（状态卡片 + 推送历史 + 提取历史）

### 8. 前端：路由 + 导航

- `web/src/App.tsx` — 新增 `/scheduler` 路由
- `web/src/components/Layout.tsx` — 新增菜单项

## 验证方法

```bash
# 1. 运行后端测试
cargo test test_extract_histories
cargo test test_status_next_run

# 2. 启动服务
cargo run

# 3. 触发一次提取，验证历史写入
curl -X POST http://localhost:3000/api/resources/extract -H "Authorization: Bearer <token>" -d '{"batch_size":100}'

# 4. 打开 http://localhost:3000/scheduler
# 验证：调度状态卡片显示、推送/提取历史列表正确
```
