# Quickstart: 资源管理查看提取对比

**Date**: 2026-06-08
**Feature**: 016-resource-extraction-view

## 开发步骤

### 1. 后端：新增 service 函数

文件：`src/services/resource.rs`

新增 `get_resource_with_raw(db, id)` 函数：
- LEFT JOIN extracted_resources + collector_histories
- 解析 raw_data JSON 提取 text 字段
- 返回 resource + raw_text + has_history

### 2. 后端：新增 handler

文件：`src/handlers/resource.rs`

新增 `get_resource_detail` handler：
- 调用 service::get_resource_with_raw
- 返回 JSON 响应

### 3. 后端：注册路由

文件：`src/routes.rs`

在 resources 路由组新增：
```rust
.route("/resources/{id}/detail", get(handlers::resource::get_resource_detail))
```

### 4. 后端：编写测试

文件：`tests/api_integration.rs`

新增集成测试：
- `test_get_resource_detail_success` — 正常查看
- `test_get_resource_detail_no_history` — 采集历史不存在
- `test_get_resource_detail_not_found` — 资源不存在

### 5. 前端：添加查看按钮和弹窗

文件：`web/src/pages/Resources.tsx`

- 在操作列添加"查看"按钮（EyeOutlined 图标）
- 新增 state：`viewModalOpen`, `viewDetail`, `viewLoading`
- 实现 `openViewModal(record)` 函数，调用 API
- 实现对比弹窗 JSX（参考 CollectorHistory.tsx 第 239-587 行的左右分栏布局）

### 6. 前端：添加类型定义

文件：`web/src/types/index.ts`

新增 `ResourceDetailResponse` 类型。

## 验证方法

```bash
# 1. 运行后端测试
cargo test test_get_resource_detail

# 2. 启动服务
cargo run

# 3. 在浏览器中打开资源管理页面，点击任意资源的"查看"按钮
# 验证弹窗左右分栏正确显示原始消息和提取结果
```
