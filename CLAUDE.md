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
RATE_LIMIT_MAX=100                 # 单 IP 请求限制次数（默认 100）
RATE_LIMIT_WINDOW=60               # 速率限制窗口秒数（默认 60）
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
│   ├── image.rs         # 图片代理：GET /api/images/{photo_id}
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
│   ├── image_proxy.rs   # 图片代理缓存（下载、缓存、TTL、并发控制）
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
- `GET /api/images/{photo_id}` — 图片代理缓存（按需下载 Telegram 图片并缓存）

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
- [生产就绪度改进](specs/004-production-improvements/plan.md) — 客户端信息完善、优雅关闭、健康检查、转发缓存、采集分页、速率限制
- [推送管理增强 — 资源提取独立化 + AI 分析](specs/005-push-ai-extraction/plan.md) — 资源提取解耦、规则引擎移植、AI 大模型增强、独立资源管理页面、多 API 轮询
- [提取推送优化](specs/006-refine-extraction-push/plan.md) — 推送配置清理、提取模式对比说明、AI 提示词优化、测试覆盖补全
- [图片代理缓存系统](specs/007-image-proxy-cache/plan.md) — Telegram 图片按需下载代理、本地缓存 + TTL、Nginx/Cloudflare CDN 缓存配置、图床域名配置入口
- [推送管理通用化 + 实时请求预览](specs/009-push-config-preview/plan.md) — 通用 HTTP 推送适配器、自定义认证/方法/模板/请求头、配置实时预览
For additional context about technologies to be used, project structure,
shell commands, and other important information, read the current plan:
- [注册开关配置 Plan](specs/010-registration-toggle/plan.md) — allow_register 系统配置，控制登录页注册 Tab 显示与 API 注册权限
- [登录验证码 Plan](specs/011-login-captcha/plan.md) — 登录接口图形验证码保护，连续失败 3 次后强制验证码
- [采集记录资源提取弹窗 Plan](specs/012-collector-resource-extraction/plan.md) — 采集记录单条资源提取、测试/正式模式弹窗、提取方法选择
- [分离 Telegram 代理与 HTTP API 代理](specs/013-http-proxy-split/plan.md) — 新增 http_proxy_url 配置，AI 提取等 HTTP 请求独立代理，移除 socks5→http 协议替换 hack
- [并发 AI 提取加速](specs/015-parallel-ai-extraction/plan.md) — AI 资源提取并发化，可配置并发数（默认 5），20 条记录提取从 3 分钟缩短到 40 秒
- [资源管理查看提取对比](specs/016-resource-extraction-view/plan.md) — 资源列表查看按钮、左右分栏对比弹窗（原始消息 vs 提取结果）、只读验证
- [资源推送测试 + hover 修复](specs/017-resource-push-test/plan.md) — 单条资源推送测试按钮（不改 is_pushed）、修复固定列 hover 背景透明问题
- [调度可视化面板](specs/018-scheduler-dashboard/plan.md) — 调度状态卡片（运行/间隔/下次执行倒计时）、推送/提取历史记录、提取历史持久化、修正重启后 next_run 计算
- [转发队列监控与代码卫生](specs/019-forward-queue-monitor/plan.md) — 图片转发队列监控页面、queue_status 补 failed 列表、清理死代码 WebSocket、修复 2 处中间件 panic
- [转发规则配置增强](specs/020-forward-rule-enhance/plan.md) — 客户端→频道级联选择、关键词+媒体类型过滤、转发客户端绑定
- [推送管理多 API 配置](specs/021-multi-push-config/plan.md) — 多推送配置(CRUD+复制)、数据源采集器选择、独立推送状态追踪、多配置串行调度
- [推送资源链接有效性检测](specs/022-resource-link-check/plan.md) — 推送前图片转存+网盘链接双维过滤、可插拔 LinkChecker(PanCheck)、URL 键缓存(TTL 24h)、双通道检测(内联兜底+按配置批量)、跳过统计与明细
<!-- SPECKIT END -->
