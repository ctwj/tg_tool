# Tasks: 调度可视化面板

**Input**: Design documents from `/specs/018-scheduler-dashboard/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: 数据库迁移 + 基础模型

- [x] T001 [P] 新建迁移 009_extract_histories_sqlite.sql：extract_histories 表（id/status/total_scanned/extracted/skipped/errors/message/executed_at）+ 索引 in `migrations/009_extract_histories_sqlite.sql`
- [x] T002 [P] 新建迁移 009_extract_histories_postgres.sql：同表结构的 PostgreSQL 版本（BIGSERIAL/TIMESTAMP）in `migrations/009_extract_histories_postgres.sql`
- [x] T003 [P] 新建 ExtractHistory 模型结构体（含 sqlx::FromRow derive）in `src/models/extract_history.rs`
- [x] T004 [P] 在 models/mod.rs 注册 pub mod extract_history in `src/models/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: 后端提取历史持久化 + 调度状态修正 — 所有前端功能依赖此阶段

**⚠️ CRITICAL**: US2/US3 的历史数据写入和 US1 的状态展示依赖此阶段完成

### Tests (TDD — 先写测试)

- [x] T005 [P] 编写集成测试 test_extract_histories_empty：空表查询返回空列表 in `tests/api_integration.rs`
- [x] T006 [P] 编写集成测试 test_extract_histories_list：插入记录后分页查询返回正确数据 in `tests/api_integration.rs`
- [x] T007 [P] 编写集成测试 test_extract_histories_stats：统计 total/success/failed/last_extracted 聚合正确 in `tests/api_integration.rs`
- [x] T008 [P] 编写集成测试 test_status_next_run_after_restart：验证修正后的 next_run 计算（last_run_at=None 时基于 started_at） in `tests/api_integration.rs`

### Implementation

- [x] T009 [P] 实现 extract_history service：insert(db, status, scanned, extracted, skipped, errors, message) + list(db, page, page_size) + stats(db) in `src/services/extract_history.rs`
- [x] T010 [P] 在 services/mod.rs 注册 pub mod extract_history in `src/services/mod.rs`
- [x] T011 修改 scheduler.rs：SchedulerState 和 ExtractSchedulerState 新增 started_at: Instant 字段，tick 循环启动时记录 in `src/services/scheduler.rs`
- [x] T012 修改 scheduler.rs extract tick：提取执行后（成功/失败均）调用 extract_history::insert 写入历史，写入失败仅 warn 不中断 in `src/services/scheduler.rs`
- [x] T013 修改 misc.rs 的 /status 接口：next_run 计算改用 last_run_at.or(started_at) 作为基准；响应新增 extract_interval_minutes 和 push_interval_minutes 字段 in `src/handlers/misc.rs`
- [x] T014 [P] 新建 handler list_extract_histories：分页查询提取历史 in `src/handlers/scheduler.rs`
- [x] T015 [P] 新建 handler get_extract_histories_stats：统计聚合 in `src/handlers/scheduler.rs`
- [x] T016 [P] 在 handlers/mod.rs 注册 pub mod scheduler in `src/handlers/mod.rs`
- [x] T017 注册路由 GET /extract-histories 和 GET /extract-histories/stats（admin 路由组） in `src/routes.rs`

**Checkpoint**: `cargo test test_extract_histories` 和 `cargo test test_status_next_run` 全部通过，API 可用

---

## Phase 3: User Story 1 - 调度任务总览状态 (Priority: P1) 🎯 MVP

**Goal**: 用户打开调度面板，看到推送/提取调度的运行状态、间隔、下次执行时间（含倒计时自动刷新）

**Independent Test**: 打开 /scheduler 页面，看到两个调度状态卡片，倒计时自动更新

### Implementation for User Story 1

- [x] T018 [US1] 新增前端类型：SchedulerStatus（push/extract 的 running/next_run/interval_minutes）in `web/src/types/index.ts`
- [x] T019 [US1] 新建 Scheduler.tsx 页面骨架：页面标题 + 调度状态区占位 in `web/src/pages/Scheduler.tsx`
- [x] T020 [US1] 实现调度状态卡片：调用 GET /status 获取 schedulers 数据，每 30 秒轮询，展示推送/提取两个卡片（运行状态 Tag + 间隔 + 下次执行时间绝对值 + 倒计时） in `web/src/pages/Scheduler.tsx`
- [x] T021 [US1] 实现倒计时逻辑：next_run 字符串解析为 Date，每秒计算剩余时间并格式化（X 分 Y 秒后） in `web/src/pages/Scheduler.tsx`
- [x] T022 [US1] 在 App.tsx 注册 /scheduler 路由 in `web/src/App.tsx`
- [x] T023 [US1] 在 Layout.tsx 左侧导航新增"调度监控"菜单项（推送菜单之后） in `web/src/components/Layout.tsx`

**Checkpoint**: 打开 /scheduler 页面，调度状态卡片正确显示，倒计时实时更新

---

## Phase 4: User Story 2 - 推送执行历史与成功率 (Priority: P2)

**Goal**: 调度面板展示推送历史列表 + 统计卡片（总次数/成功/失败/成功率）

**Independent Test**: 有推送历史时，面板推送历史区显示统计卡片和分页列表

### Implementation for User Story 2

- [x] T024 [US2] 实现推送统计卡片：调用 GET /push/stats，展示总次数/成功/失败/成功率（百分比） in `web/src/pages/Scheduler.tsx`
- [x] T025 [US2] 实现推送历史表格：调用 GET /push/histories 分页查询，列含批次ID/目标/状态/数据量/消息/错误信息/时间，失败状态红色 Tag in `web/src/pages/Scheduler.tsx`
- [x] T026 [US2] 实现推送历史分页：Pagination 组件联动 page 参数重新查询 in `web/src/pages/Scheduler.tsx`

**Checkpoint**: 推送历史区正确显示统计和分页列表，失败记录红色高亮

---

## Phase 5: User Story 3 - 提取执行历史 (Priority: P3)

**Goal**: 调度面板展示提取历史列表（扫描数/提取数/跳过数/错误数/时间）

**Independent Test**: 触发提取后，面板提取历史区显示对应记录

### Implementation for User Story 3

- [x] T027 [US3] 实现提取历史表格：调用 GET /extract-histories 分页查询，列含状态/扫描数/提取数/跳过数/错误数/消息/时间 in `web/src/pages/Scheduler.tsx`
- [x] T028 [US3] 实现提取历史分页：Pagination 组件联动 in `web/src/pages/Scheduler.tsx`
- [x] T029 [US3] 实现提取统计卡片（可选）：调用 GET /extract-histories/stats，展示总执行/成功/失败/最近提取数 in `web/src/pages/Scheduler.tsx`

**Checkpoint**: 提取历史区正确显示记录和分页

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: 代码质量和最终验证

- [x] T030 运行 cargo test 确认所有测试通过
- [x] T031 运行 cargo clippy 确认无警告
- [x] T032 运行 cargo fmt 确认格式正确
- [x] T033 运行 npx tsc --noEmit 确认前端类型检查通过
- [x] T034 启动服务手动验证：调度状态卡片倒计时 + 推送/提取历史列表

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: 无依赖，迁移和模型可并行
- **Foundational (Phase 2)**: 依赖 T003-T004 模型；T005-T008 测试可并行编写
- **US1 (Phase 3)**: 依赖 Phase 2 完成（status 接口修正 + extract_histories 接口可用）
- **US2 (Phase 4)**: 依赖 US1 完成（页面骨架存在）；复用现有 push 接口
- **US3 (Phase 5)**: 依赖 Phase 2 完成（extract-histories 接口）+ US1 完成（页面骨架）
- **Polish (Phase 6)**: 依赖所有用户故事完成

### User Story Dependencies

- **US1 (P1)**: 依赖 Foundational — 无其他故事依赖
- **US2 (P2)**: 依赖 US1（页面骨架）— 可与 US3 并行
- **US3 (P3)**: 依赖 US1（页面骨架）+ Phase 2（接口）— 可与 US2 并行

### Within Each User Story

- 测试（Phase 2）先写，确保失败后再实现
- 模型 → service → handler → 路由
- 后端完成后再开发前端
- Story 完成后可独立验证

### Parallel Opportunities

- T001/T002/T003/T004 可并行（不同文件）
- T005/T006/T007/T008 可并行（不同测试函数）
- T009/T010/T014/T015/T016 可并行（不同文件）
- T024/T025/T026 与 T027/T028/T029 可并行（US2 与 US3 独立区块）

---

## Parallel Example: Phase 2

```bash
# 并行编写所有测试和独立模块：
Task T005/T006/T007/T008: "集成测试 in tests/api_integration.rs"
Task T009/T010: "extract_history service + mod 注册"
Task T014/T015/T016: "scheduler handler + mod 注册"

# 然后串行实现有依赖的部分：
Task T011 → T012（scheduler 修改有顺序依赖）
Task T013（misc.rs 修正）
Task T017（路由注册，依赖 handler 完成）
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: 迁移 + 模型
2. Complete Phase 2: 后端 API + 测试 + scheduler 修正
3. Complete Phase 3: 调度状态卡片页面
4. **STOP and VALIDATE**: 打开 /scheduler 看到调度状态卡片正确显示
5. US2/US3 增量添加历史区

### Task Summary

| Phase | Tasks | 说明 |
|-------|-------|------|
| Setup | T001-T004 (4) | 迁移 + 模型 |
| Foundational | T005-T017 (13) | 后端 API + scheduler 修正 + 测试 |
| US1 | T018-T023 (6) | 调度状态卡片页面 🎯 MVP |
| US2 | T024-T026 (3) | 推送历史区 |
| US3 | T027-T029 (3) | 提取历史区 |
| Polish | T030-T034 (5) | 质量验证 |
| **Total** | **34** | |

### MVP Scope

Phase 1 + 2 + 3 = 23 个任务，交付可用的调度状态可视化。

---

## Notes

- Constitution I (TDD): T005-T008 测试先写，Phase 2 实现必须让测试通过
- Constitution III (可观测性): T012 提取失败也写历史，弥补当前 scheduler 仅 warn 的缺口
- Constitution IV (DB 兼容): T001/T002 双版本迁移，T009 service 用 `?`/`$N` 双占位符
- Constitution V (YAGNI): 复用 /status 和 push_histories，不重复造接口；轮询代替 WebSocket
- 关键修复：T011/T013 修正 next_run 重启计算 bug（spec FR-009 + SC-004）
