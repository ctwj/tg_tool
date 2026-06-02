# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram 消息转发工具 — Rust 全栈实现（已完成核心功能集成）。

**根目录** Rust 项目，技术栈：axum 0.8 + grammers-client 0.7 + sqlx 0.8 + tokio 1 + reqwest 0.12

核心功能：Telegram 账号管理（Client/Bot 认证）、消息采集（Collector）、转发规则（Chat/Webhook）、图片上传图床、消息推送调度

### 参考代码
- `demo/` 和 `telegram-forwarding/` — 原 Go 源码（仅供参考，不是活跃代码）

## Build & Run Commands

```bash
cargo build          # 编译
cargo run            # 运行（默认端口 3000）
cargo test           # 单元测试 + 集成测试
cargo clippy         # 静态检查
```

### 环境变量（.env 文件）

```env
DATABASE_URL=tg_store/tgtool.db    # SQLite 默认；设 SQL_DSN 切换 PostgreSQL
TG_APP_ID=your_api_id              # Telegram API 凭证
TG_APP_HASH=your_api_hash
PROXY_URL=socks5://127.0.0.1:1080 # 可选代理
PORT=3000
SESSION_SECRET=your-random-secret
```

## Architecture

```
src/
├── main.rs              # 服务启动：DB 初始化、TgManager 注入、客户端恢复
├── config.rs            # clap 配置（env/命令行参数）
├── state.rs             # AppState、TgClientEntry、DbPool、OptionCache
├── routes.rs            # public/protected 路由分离 + auth_guard 中间件
├── errors.rs            # 统一错误类型 AppError
├── models/              # 数据模型（client, rule, collector, user, message, ...）
├── handlers/            # HTTP handler 层（CRUD + 业务调用）
│   ├── client.rs        # 客户端管理：添加/删除/启停/认证/聊天列表
│   ├── auth.rs          # 用户注册/登录/Token
│   ├── rule.rs          # 转发规则 CRUD
│   ├── collector.rs     # 采集器 CRUD + 全量采集触发
│   ├── push.rs          # 推送触发/统计/调度配置
│   ├── user.rs          # 用户管理
│   ├── file.rs          # 文件上传/下载
│   ├── option.rs        # 系统配置
│   └── misc.rs          # 系统状态
├── services/            # 业务逻辑层
│   ├── tg_manager.rs    # 客户端生命周期管理（connect/disconnect/update loop）
│   ├── tg_auth.rs       # 认证状态机（phone→code→password→active, bot_sign_in）
│   ├── tg_api.rs        # Telegram API 高层封装（聊天列表、发消息）
│   ├── message_handler.rs # 消息分发（匹配规则+采集器）
│   ├── forwarder.rs     # Chat + Webhook 转发
│   ├── collector.rs     # 全量采集 + 图片上传图床
│   ├── push.rs          # 批量推送 + 消息分析管线
│   ├── scheduler.rs     # 定时推送调度器
│   └── crypto.rs        # JWT/密码哈希
├── middleware/
│   ├── auth.rs          # Bearer Token / Session Cookie 认证
│   ├── cors.rs          # CORS
│   └── rate_limit.rs    # 频率限制
└── db/                  # 数据库迁移 (migrations/001_init.sql)
```

### 数据库

默认 SQLite，通过 `SQL_DSN` 环境变量可切换到 PostgreSQL。使用 sqlx，自动迁移。初始 root 用户: `root / 123456`

### 关键设计

- **TgManager**: 全局客户端管理器，注入到 AppState，管理 grammers-client 实例生命周期
- **TgClientEntry**: 内存中的客户端状态（status/handle/client/login_token/password_token/session_path）
- **OptionCache**: 系统配置缓存（tg_app_id/tg_app_hash/proxy_url/图床配置/推送配置）
- **auth_guard**: 统一认证中间件，public 路由无需认证，protected 路由需 Bearer Token

## API Routes

所有 API 路径以 `/api` 为前缀：

### 公开路由（无需认证）
- `POST /api/auth/register` / `POST /api/auth/login` / `POST /api/auth/logout`
- `GET /api/status`
- `GET /api/files/download/{filename}`

### 受保护路由（需要 Bearer Token）
- `/api/clients` — 客户端 CRUD + 启停 + 认证（phone/code/password/bot_token）+ 聊天列表
- `/api/rules` — 转发规则 CRUD + 切换 + 消息列表
- `/api/collectors` — 采集器 CRUD + 切换 + 全量采集 + 历史记录
- `/api/push` — 推送触发/统计/历史/重试/调度配置
- `/api/users` — 用户管理
- `/api/files` — 文件管理
- `/api/options` — 系统配置

用户角色层级：CommonUser(1) < Admin(10) < Root(100)

## Testing

```bash
cargo test                          # 全部测试（76 单元 + 40 集成）
cargo test --test api_integration   # 仅集成测试
cargo test -- --nocapture           # 显示 println! 输出
```

测试使用 SQLite 内存数据库，集成测试通过 `tower::ServiceExt` 模拟 HTTP 请求。

## Rust 重写状态

已完成核心功能集成（specs/002-telegram-core-integration），包括：
- ✅ 客户端连接认证（手机号/验证码/两步密码/Bot Token）
- ✅ 实时消息监听与分发
- ✅ Chat + Webhook 转发
- ✅ 频道消息全量采集 + 图片上传图床
- ✅ 定时推送调度 + 消息分析管线
- ✅ API 路由保护（auth 中间件）

<!-- SPECKIT START -->
- [Telegram 核心功能集成](specs/002-telegram-core-integration/plan.md) — 客户端连接、认证、消息收发、转发、采集、推送
<!-- SPECKIT END -->
