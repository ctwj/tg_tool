# Implementation Plan: 调度可视化面板

**Branch**: `018-scheduler-dashboard` | **Date**: 2026-06-09 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/018-scheduler-dashboard/spec.md`

## Summary

新增独立的调度可视化页面，集中展示推送/提取定时任务的运行状态、下次执行时间和执行历史。后端需新建 `extract_histories` 表持久化提取批次结果，修正 `/api/status` 中调度器重启后下次执行时间计算偏差，新增提取历史查询接口；前端新建 Scheduler 页面，复用 `/status` 轮询调度状态，分页展示推送和提取历史。

## Technical Context

**Language/Version**: Rust 1.75+ (后端), TypeScript + React (前端)
**Primary Dependencies**: axum 0.8, sqlx 0.8, Ant Design 5.x (前端)
**Storage**: SQLite / PostgreSQL (通过 DbPool 双轨)
**Testing**: cargo test (Rust), tower::ServiceExt (集成测试)
**Target Platform**: Web 服务 (axum HTTP API + React SPA)
**Project Type**: Web application (前后端分离，前端嵌入后端)
**Performance Goals**: 面板加载 < 2 秒；状态轮询间隔 30 秒
**Constraints**: 必须同时兼容 SQLite 和 PostgreSQL；迁移文件双版本
**Scale/Scope**: 单页可视化，2 个调度任务，2 类历史记录

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| 原则 | 状态 | 说明 |
|------|------|------|
| I. 测试驱动开发 | ✅ 计划遵循 | 新增提取历史 service/handler 先写测试；status 接口修正需补回归测试 |
| II. 模块化设计 | ✅ 计划遵循 | handler 仅处理 HTTP，service 处理查询/聚合；提取历史写入由 scheduler 调用 service |
| III. 错误处理与可观测性 | ✅ 计划遵循 | 提取历史记录失败/成功均持久化，弥补当前 scheduler 仅 warn 的可观测性缺口 |
| IV. 数据库兼容性 | ✅ 计划遵循 | 新建 009 迁移双版本（sqlite + postgres）；查询用 `?`/`$N` 双占位符 |
| V. 简洁优先 | ✅ 计划遵循 | 复用现有 /status 接口和 push_histories 表，仅新增必要的 extract_histories 表和查询接口 |

## Project Structure

### Documentation (this feature)

```text
specs/018-scheduler-dashboard/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
migrations/
├── 009_extract_histories_sqlite.sql     # 新建：提取历史表（SQLite）
└── 009_extract_histories_postgres.sql   # 新建：提取历史表（PostgreSQL）

src/
├── models/
│   ├── extract_history.rs               # 新建：ExtractHistory 模型
│   └── mod.rs                           # 新增 pub mod extract_history
├── services/
│   ├── scheduler.rs                     # 修改：提取成功后写入 extract_histories
│   ├── extract_history.rs               # 新建：查询/插入提取历史
│   └── mod.rs                           # 新增 pub mod extract_history
├── handlers/
│   ├── scheduler.rs                     # 新建：list_extract_histories handler
│   ├── misc.rs                          # 修改：修正 next_run 计算（重启场景）
│   └── mod.rs                           # 新增 pub mod scheduler
├── routes.rs                            # 新增路由
└── tests/...                            # 新增集成测试

web/src/
├── pages/
│   └── Scheduler.tsx                    # 新建：调度可视化页面
├── App.tsx                              # 新增路由 /scheduler
├── components/Layout.tsx                # 新增菜单项
└── types/index.ts                       # 新增类型定义
```

**Structure Decision**: 遵循现有分层 handler → service → model。提取历史独立为 `extract_history` 模块（与 push_history 对称）。前端新建独立 Scheduler 页面，纳入主导航。

## Complexity Tracking

无 Constitution 违规，无需记录。
