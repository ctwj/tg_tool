# Research: 调度可视化面板

**Date**: 2026-06-09
**Feature**: 018-scheduler-dashboard

## Decision 1: 提取历史持久化方案

**Decision**: 新建 `extract_histories` 表，字段对齐 `ExtractionResult` 结构

**Rationale**:
- 当前提取结果 `{total_scanned, extracted, skipped, errors}` 仅输出到日志，重启即丢失
- scheduler.rs 的 extract tick（L223-257）在执行成功后仅更新 `last_run_at`，无持久化
- 表结构对齐 push_histories 的设计模式，保持一致性：含 status、message、executed_at

**表结构设计**:
```sql
CREATE TABLE extract_histories (
    id           BIGSERIAL PRIMARY KEY,
    status       VARCHAR(20) NOT NULL,        -- 'success' / 'failed'
    total_scanned BIGINT DEFAULT 0,
    extracted    BIGINT DEFAULT 0,
    skipped      BIGINT DEFAULT 0,
    errors       BIGINT DEFAULT 0,
    message      TEXT,
    executed_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

**Alternatives considered**:
- 复用 push_histories 表 → 字段语义不符（batch_id/target 是推送概念），强行复用会污染
- 不持久化，用 extracted_resources.created_at 间接推算 → 无法表达"扫描了但未提取"的批次，也无法记录失败批次

## Decision 2: 下次执行时间计算修正（重启场景）

**Decision**: scheduler 状态新增 `started_at: Instant` 字段，next_run 基于 `started_at` 计算

**Rationale**:
- 当前 bug：`last_run_at` 仅在执行成功后更新（scheduler.rs L91-94）。重启后 `last_run_at = None` → `elapsed = 0` → next_run 显示完整间隔（高估剩余时间）
- 真实情况：调度循环用 `tokio::sleep(interval)` 启动后立即计时，实际剩余 = interval - (now - started_at)
- 修正：tick 循环启动时记录 `started_at`，next_run = started_at + interval（取最近一个未来时刻）

**修正后的计算公式**:
```rust
// 启动后到首次执行：基于 started_at
let baseline = sched.last_run_at.or(sched.started_at);
let elapsed = baseline.elapsed().as_secs();
let next_secs = (sched.interval_minutes * 60).saturating_sub(elapsed % (sched.interval_minutes * 60));
```

**Alternatives considered**:
- 持久化 next_run 到数据库 → 过度设计，调度本就是内存状态
- 不修正，接受偏差 → 违反 SC-004（倒计时准确率 100%）

## Decision 3: 调度状态数据源

**Decision**: 前端复用现有 `GET /api/status` 接口的 `schedulers` 块，轮询间隔 30 秒

**Rationale**:
- `/status`（misc.rs L188-195）已返回 `extract_running/extract_next_run/push_running/push_next_run/forward_running/forward_interval_secs`
- 无需新建接口，仅修正 next_run 计算（Decision 2）即可
- 轮询 30 秒：平衡实时性与服务器负载，倒计时显示误差 ≤ 30 秒可接受（SC-004 要求 < 30 秒偏差）

**Alternatives considered**:
- WebSocket 实时推送 → spec 明确列为未来增强方向，当前 YAGNI
- 新建专用 /scheduler/status 接口 → 与 /status 重复，违反 DRY

## Decision 4: 提取历史写入时机

**Decision**: 在 scheduler.rs 的 extract tick 中，提取执行后（无论成功/失败）写入 extract_histories

**Rationale**:
- 提取结果在 `trigger_extraction` 返回 `ExtractionResult`，scheduler tick 拿到后写库
- 失败时也要记录（status=failed + error message），满足"调度失败可见"的核心目标
- 写入失败不应中断调度循环，用 `if let Err(e) = ... { tracing::warn! }` 容错

**写入位置**（scheduler.rs extract tick，约 L234 附近）:
```rust
let result = resource::trigger_extraction(&app_state, 1000).await;
let (status, scanned, extracted, skipped, errors, msg) = match &result {
    Ok(r) => ("success", r.total_scanned, r.extracted, r.skipped, r.errors, None),
    Err(e) => ("failed", 0, 0, 0, 0, Some(e.to_string())),
};
if let Err(e) = extract_history::insert(&app_state.db, ...).await {
    tracing::warn!("写入提取历史失败: {e}");
}
```

**Alternatives considered**:
- 在 trigger_extraction 内部写 → 违反单一职责，提取逻辑不应关心历史记录
- 用 tracing 日志 + 日志分析 → 运维成本高，无法在面板直接展示

## Decision 5: 前端页面集成位置

**Decision**: 新建独立路由 `/scheduler` 页面，纳入左侧主导航菜单

**Rationale**:
- Dashboard 已用于全局概览（客户端/规则/采集器统计），调度状态卡片塞入会拥挤
- 独立页面便于聚焦展示调度状态 + 两类历史，符合"调度运维面板"定位
- 导航菜单位于"推送"之后，与推送配置页形成"配置→监控"的工作流

**Alternatives considered**:
- 作为 Push.tsx 的 Tab → Push 页已 698 行，再加调度面板过于臃肿
- 作为 Dashboard 的扩展区块 → 与 Dashboard 的"全局概览"定位不符
