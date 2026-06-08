# Tasks: 资源管理查看提取对比

**Input**: Design documents from `/specs/016-resource-extraction-view/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: 准备前端类型定义，为后续任务提供类型基础

- [x] T001 [P] 新增 ResourceDetailResponse 类型定义，包含 resource、raw_text、raw_data、media_type、has_history 字段 in `web/src/types/index.ts`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: 后端只读查询接口 — 所有前端功能依赖此 API

**⚠️ CRITICAL**: 前端弹窗开发必须在此阶段完成后才能开始

### Tests (TDD — 先写测试)

- [x] T002 [P] 编写集成测试 test_get_resource_detail_success：正常返回资源详情 + 原始消息 in `tests/api_integration.rs`
- [x] T003 [P] 编写集成测试 test_get_resource_detail_no_history：采集历史不存在时 has_history=false in `tests/api_integration.rs`
- [x] T004 [P] 编写集成测试 test_get_resource_detail_not_found：资源 ID 不存在返回 404 in `tests/api_integration.rs`

### Implementation

- [x] T005 实现 get_resource_with_raw service 函数：LEFT JOIN extracted_resources + collector_histories，解析 raw_data JSON 提取 text 字段，返回 ResourceDetailResponse in `src/services/resource.rs`
- [x] T006 实现 get_resource_detail handler：调用 service，返回 JSON 响应 in `src/handlers/resource.rs`
- [x] T007 注册新路由 GET /resources/{id}/detail in `src/routes.rs`

**Checkpoint**: `cargo test test_get_resource_detail` 全部通过，API 可用

---

## Phase 3: User Story 1 - 查看资源提取结果对比 (Priority: P1) 🎯 MVP

**Goal**: 用户点击资源列表的"查看"按钮，弹出左右分栏对比窗口，验证提取链接是否正确

**Independent Test**: 在资源页面点击任意资源的"查看"按钮，弹窗正确显示原始消息和提取结果

### Implementation for User Story 1

- [x] T008 [US1] 在操作列添加"查看"按钮（EyeOutlined 图标），宽度从 120 调整为 150 in `web/src/pages/Resources.tsx`
- [x] T009 [US1] 新增 state 管理：viewModalOpen、viewDetail、viewLoading，实现 openViewModal(record) 调用 GET /resources/{id}/detail API in `web/src/pages/Resources.tsx`
- [x] T010 [US1] 实现查看弹窗 JSX — 左右分栏布局（参考 CollectorHistory.tsx 第 239-587 行样式）：左侧显示 raw_text（whiteSpace: pre-wrap），右侧显示提取结果卡片（标题、链接列表、描述、分类标签、标签、提取模式） in `web/src/pages/Resources.tsx`
- [x] T011 [US1] 添加边界状态处理：has_history=false 时左侧显示"原始消息不可用"提示（Alert 组件），url 为空时右侧链接区域显示"无链接" in `web/src/pages/Resources.tsx`

**Checkpoint**: 资源页面"查看"按钮功能完整，弹窗左右对比正常工作

---

## Phase 4: User Story 2 - 快速关闭查看窗口 (Priority: P2)

**Goal**: 用户可通过关闭按钮、ESC 键、点击遮罩三种方式关闭查看弹窗

**Independent Test**: 打开查看弹窗后，三种关闭方式均有效

### Implementation for User Story 2

- [x] T012 [US2] 确认 Modal 组件已配置 onCancel（点击遮罩/ESC 关闭）和 footer={null}（隐藏底部按钮区） in `web/src/pages/Resources.tsx`

**Checkpoint**: 三种关闭方式均可正常工作

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: 代码质量和最终验证

- [x] T013 运行 `cargo test` 确认所有测试通过
- [x] T014 运行 `cargo clippy` 确认无警告
- [x] T015 启动服务验证前端弹窗完整功能

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: 无依赖，可立即开始
- **Foundational (Phase 2)**: T001 完成后可开始；测试 T002-T004 可并行编写
- **User Story 1 (Phase 3)**: 依赖 Phase 2 完成（API 可用后才能开发前端弹窗）
- **User Story 2 (Phase 4)**: 依赖 Phase 3 完成（弹窗存在后才能配置关闭方式）
- **Polish (Phase 5)**: 依赖所有用户故事完成

### User Story Dependencies

- **User Story 1 (P1)**: 依赖 Foundational — 无其他故事依赖
- **User Story 2 (P2)**: 依赖 US1（弹窗组件存在才能配置关闭方式）— 实际上 US2 是 Modal 组件的内置行为，可在 T010 中一并实现

### Parallel Opportunities

- T001、T002、T003、T004 可并行（不同文件/测试函数）
- T005、T006、T007 必须串行（service → handler → route）
- T008-T011 可部分并行（T009 依赖 T008 的按钮，T010-T011 依赖 T009 的 state）

---

## Parallel Example: Phase 2

```bash
# 并行编写所有测试：
Task T002: "test_get_resource_detail_success in tests/api_integration.rs"
Task T003: "test_get_resource_detail_no_history in tests/api_integration.rs"
Task T004: "test_get_resource_detail_not_found in tests/api_integration.rs"
Task T001: "ResourceDetailResponse 类型 in web/src/types/index.ts"

# 然后串行实现后端：
Task T005 → T006 → T007
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: 类型定义
2. Complete Phase 2: 后端 API + 测试
3. Complete Phase 3: 前端查看弹窗
4. **STOP and VALIDATE**: 点击"查看"按钮，确认弹窗显示正确
5. US2（关闭方式）通常在 T010 中自然实现，可快速验证

### Task Summary

| Phase | Tasks | 说明 |
|-------|-------|------|
| Setup | T001 (1) | 前端类型定义 |
| Foundational | T002-T007 (6) | 后端 API + 测试 |
| US1 | T008-T011 (4) | 查看按钮 + 对比弹窗 |
| US2 | T012 (1) | 关闭方式确认 |
| Polish | T013-T015 (3) | 质量验证 |
| **Total** | **15** | |

---

## Notes

- Constitution I (TDD): T002-T004 测试先写，确保失败后再实现 T005-T007
- Constitution IV (DB 兼容): T005 必须包含 SQLite/PostgreSQL 双轨 SQL
- Constitution V (YAGNI): 不抽取共享弹窗组件，在 Resources.tsx 中内联实现
- 前端弹窗参考 CollectorHistory.tsx 的左右分栏样式，但不复制其提取逻辑（测试/正式模式按钮等）
