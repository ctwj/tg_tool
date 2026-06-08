# Implementation Plan: 资源管理查看提取对比

**Branch**: `016-resource-extraction-view` | **Date**: 2026-06-08 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/016-resource-extraction-view/spec.md`

## Summary

在资源管理页面的每行操作列添加"查看"按钮，点击弹出与采集器资源提取弹窗风格一致的只读左右分栏对比窗口：左侧显示原始采集消息内容，右侧显示已提取的资源详情。需要新增一个后端 API 接口，根据资源 ID 联查采集历史返回原始消息。

## Technical Context

**Language/Version**: Rust 1.75+ (后端), TypeScript + React (前端)
**Primary Dependencies**: axum 0.8, sqlx 0.8, Ant Design 5.x (前端)
**Storage**: SQLite / PostgreSQL (通过 DbPool 双轨)
**Testing**: cargo test (Rust), tower::ServiceExt (集成测试)
**Target Platform**: Web 服务 (axum HTTP API + React SPA)
**Project Type**: Web application (前后端分离，前端嵌入后端)
**Performance Goals**: 弹窗加载 < 3 秒（单条资源 + 单条历史联查）
**Constraints**: 必须同时兼容 SQLite 和 PostgreSQL
**Scale/Scope**: 单条记录查看，无批量操作

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| 原则 | 状态 | 说明 |
|------|------|------|
| I. 测试驱动开发 | ✅ 计划遵循 | 新增 API handler + service 函数，先写测试再实现 |
| II. 模块化设计 | ✅ 计划遵循 | handler 仅处理 HTTP，service 处理联查逻辑 |
| III. 错误处理与可观测性 | ✅ 计划遵循 | 使用 AppError，采集历史不存在时友好提示 |
| IV. 数据库兼容性 | ✅ 计划遵循 | SQLite ? 和 PostgreSQL $N 双轨占位符 |
| V. 简洁优先 | ✅ 计划遵循 | 仅新增一个只读查询接口，不引入新依赖 |

## Project Structure

### Documentation (this feature)

```text
specs/016-resource-extraction-view/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
├── handlers/
│   └── resource.rs              # 新增 get_resource_detail handler
├── services/
│   └── resource.rs              # 新增 get_resource_with_raw service 函数
├── models/
│   ├── extracted_resource.rs    # 现有，无需修改
│   └── collector_history.rs     # 现有，无需修改
└── routes.rs                    # 新增路由注册

web/src/
├── pages/
│   ├── Resources.tsx            # 添加查看按钮 + 对比弹窗
│   └── CollectorHistory.tsx     # 参考其弹窗样式（不修改）
├── types/
│   └── index.ts                 # 新增 ResourceDetailResponse 类型
└── api/
    └── resources.ts             # 新建资源 API 封装（可选）
```

**Structure Decision**: 遵循现有项目分层：handler → service → model。前端在现有 Resources.tsx 中扩展，复用 CollectorHistory.tsx 的弹窗样式模式。

## Complexity Tracking

无 Constitution 违规，无需记录。
