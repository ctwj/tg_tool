# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram 消息转发工具 — Rust 全栈实现。后端 axum 0.8 + grammers-client 0.7 + sqlx 0.8 + tokio 1 + reqwest 0.12；前端 React 18 + TypeScript + Vite + Ant Design（`web/`），构建产物经 rust-embed 嵌入二进制（`embed-frontend` feature，Docker 部署用）。

核心功能域：
- **Telegram 核心**：账号管理（Client/Bot 认证）、消息采集（Collector）、转发规则（Chat/Webhook）、图片转存图床、定时推送调度
- **资源提取管线**：规则引擎 + AI 大模型并发提取 + 9 平台网盘链接识别 + 推送前链接有效性检测
- **爬虫采集子系统**：配置驱动多站点 + 可视化字段配置器 + 7 种提取模式（含 JS 脚本沙箱）
- **网盘账号管理与链接转存**（feature 047）：夸克账号凭据加密管理 + 分享链接转存/直链上传 + 开放转存 API

`demo/` 与 `telegram-forwarding/` 为原 Go 源码，仅供参考，不是活跃代码。

## Build & Run Commands

```bash
cargo build                                # 编译
cargo run                                  # 运行（默认端口 3000，SQLite ./data.db）
cargo test                                 # 全部测试（单元 + 集成）
cargo test --lib                           # 仅单元测试
cargo test --test api_integration          # 单个集成测试文件
cargo test pan                             # 按名称过滤测试
cargo clippy --all-targets -- -D warnings  # 零警告（CI 必过，.github/workflows/ci.yml）
```

前端（`web/`）：

```bash
cd web
npm install
npm start                          # 开发服务器，代理到 localhost:3000
npm run build                      # 产出 web/dist/
cargo build --features embed-frontend  # 构建嵌入前端的二进制（Dockerfile 即此方式）
```

### 环境变量（.env，模板见 .env.example）

| 变量 | 说明 |
|------|------|
| `SQL_DSN` | 留空默认 SQLite `./data.db`；设 `postgres://...` 切换 PostgreSQL |
| `TG_APP_ID` / `TG_APP_HASH` | Telegram API 凭证（也可在系统配置页设置，走 OptionCache） |
| `SESSION_SECRET` | 启动期强制校验：非空/非默认值/≥32 字符，否则 panic（feature 027） |
| `PAN_CRED_KEY` | 网盘凭据加密主密钥（base64 32 字节，AES-256-GCM）；缺失时启动自动生成并写回 .env 持久化 |
| `PAN_STAGING_DIR` | 直链上传本地中转目录（默认 `./.tmp/pan-staging`） |
| `TG_STORE` | Telegram session 存储路径（默认 `./tg_store`） |
| `REDIS_CONN_STRING` | 留空使用内存 session 存储 |
| `PROXY_URL` | 可选代理（Telegram 与 HTTP API 代理在系统配置中分离） |
| `PORT` / `RUST_LOG` / `LOG_DIR` / `RATE_LIMIT_MAX` / `RATE_LIMIT_WINDOW` | 常规（限流默认 100 次/60s） |

## Architecture

```
src/
├── main.rs              # 服务启动：DB 初始化 + run_migrations、TgManager 注入、客户端恢复、后台任务 spawn
├── config.rs            # clap 配置（env/命令行参数）；PAN_CRED_KEY 自动生成逻辑在此
├── state.rs             # AppState、DbPool（enum Sqlite/Postgres）、TgClientEntry、CaptchaStore、PeerCache
├── routes.rs            # public/user/admin/root 四层路由 merge + auth_guard/admin_guard/root_guard
├── embed.rs             # rust-embed 前端嵌入 + static_handler fallback
├── errors.rs            # 统一错误类型 AppError
├── models/              # 数据模型（sqlx FromRow）
├── handlers/            # HTTP handler 层（CRUD + 业务调用）
│   ├── client.rs        # 客户端管理：添加/删除/启停/认证/聊天列表/bot 校验
│   ├── auth.rs          # 注册/登录/登出/验证码/me/API Token（含注册开关状态）
│   ├── rule.rs          # 转发规则 CRUD
│   ├── collector.rs     # 采集器 CRUD + 全量采集触发
│   ├── push.rs          # 推送触发/统计/历史/重试/调度配置 + 多推送配置 CRUD/复制/触发/链接检测
│   ├── resource.rs      # 资源列表/批量提取/单条提取/详情对比/推送测试/链接检测/提取配置
│   ├── scheduler.rs     # 提取历史（调度可视化面板）
│   ├── image.rs         # 图片代理：GET /api/images/{photo_id}、/api/images/file/{file_id}
│   ├── image_forward.rs # 图片转发队列监控：queue_status/retry/retry_all
│   ├── crawler.rs       # 爬虫：任务/文章/历史 CRUD + test_run + 字段树 + 探针 + 字段库 + 模板 + 脚本沙盒
│   ├── pan_account.rs   # [047] 网盘账号 CRUD + check 校验 + diagnose 综合诊断
│   ├── pan_transfer.rs  # [047] 转存/上传任务 CRUD + retry + cleanup
│   ├── pan_config.rs    # [047] 网盘功能配置在线调整
│   ├── api_keys.rs      # [047] 开放 API 凭据：list/create/revoke/rotate
│   ├── api_transfer.rs  # [047] 开放转存 API /api/v1/*（X-API-Key 鉴权）
│   ├── user.rs / file.rs / option.rs / misc.rs / response.rs
├── services/            # 业务逻辑层
│   ├── tg_manager.rs    # 客户端生命周期（connect/disconnect/update loop）
│   ├── tg_auth.rs       # 认证状态机（phone→code→password→active, bot_sign_in）
│   ├── tg_api.rs        # Telegram API 高层封装（聊天列表、发消息）
│   ├── message_handler.rs # 消息分发（匹配规则+采集器）
│   ├── forwarder.rs     # Chat + Webhook 转发
│   ├── forward_queue.rs # 图片转发队列调度（两阶段转存状态机 + 崩溃恢复）
│   ├── collector.rs     # 全量采集 + 图片上传图床
│   ├── image_proxy.rs   # 图片代理缓存（下载、缓存、TTL、并发控制）
│   ├── push.rs / push_config.rs  # 批量推送 + 消息分析管线 / 多推送配置
│   ├── extractor.rs     # 规则引擎资源提取
│   ├── ai_extractor.rs  # AI 大模型提取（多 API 轮询、可配并发）
│   ├── resource.rs / extract_history.rs  # 资源管理 / 提取历史
│   ├── link_parser.rs / link_check.rs / link_checker.rs  # 链接解析 / 推送前检测（PanCheck 双通道）
│   ├── transfer.rs / share.rs / staging.rs  # [047] 转存任务编排状态机 / 分享生成 / 直链中转落盘
│   ├── pan_account.rs   # [047] 账号凭据加解密/健康检查/容量
│   ├── pan/             # [047] 网盘驱动（credential.rs 凭据加密、quark.rs 夸克驱动；多驱动时抽 PanDriver trait）
│   ├── api_key.rs       # [047] API Key 签发/校验/配额
│   ├── bot_api.rs       # Bot API 调用（forwardMessage 取 file_id）
│   ├── crawler/         # 爬虫子系统（见下）
│   └── crypto.rs        # JWT/密码哈希/SESSION_SECRET 校验
├── middleware/
│   ├── auth.rs          # Bearer Token / Session Cookie 认证
│   ├── api_key.rs       # [047] X-API-Key 鉴权（/api/v1/*）
│   ├── session.rs       # tower-sessions 管理
│   ├── cors.rs / rate_limit.rs
└── （迁移在根目录 migrations/，不在 src/db/）
```

### 数据库与迁移（重要流程）

- 默认 SQLite（`./data.db`），`SQL_DSN` 切 PostgreSQL；sqlx 手写 runner（**不是** `sqlx::migrate!`）：`main.rs::run_migrations` 逐个 `include_str!` 执行根目录 `migrations/*.sql`，靠幂等检查跳过已应用项
- **新增 migration**：在根目录 `migrations/` 加 `NNN_desc_sqlite.sql` + `NNN_desc_postgres.sql` 两套，并**手动在 `run_migrations` 注册**（漏注册不会执行）；双库禁止专有语法
- 现有 001–041；026-039 为 crawler 字段树/字段库/文章扩展值/分页深度/refresh_on_read 等，040-041 为 pan 账号表与容量列
- 初始 root 用户：`root / 123456`（feature 027 后改为随机强口令 + must_change_password）

### 后台任务（main.rs 启动清单）

- 推送/提取调度 `services/scheduler.rs`（`start_scheduler` + `start_extract_scheduler`）
- 图片转发队列 `services/forward_queue.rs::start_forward_scheduler`（两阶段转存：客户端 copy_media 到群组 A → Bot forwardMessage 到群组 B 取 file_id；stage1/stage2_running 中间态 + 崩溃恢复扫描）
- 爬虫调度 `services/crawler/scheduler.rs`（30s tick + Semaphore 全局并发，默认 3，可配 `crawler_global_concurrency`）
- 爬虫图片上传 `services/crawler/image_uploader.rs`（下载 → 落盘 `image_cache_dir/crawler/` → 上传图床群组）
- Telegram 客户端恢复 + update loop `services/tg_manager.rs`

### 爬虫子系统（services/crawler/）

- `engine.rs` — 字段树驱动两阶段 list/detail 抓取；[045] URL 模板分页（`{page}` 占位符，模板优先独占）+ 跨 seed 全局去重；[044] `force_full_collect=false` 时连续空页早停
- `field_schema.rs` — 字段树 schema（list/detail 双作用域、7 种模式、父子嵌套、校验）
- `extractor.rs` — 7 模式字段提取（css/regex/prefix_suffix/json_path/meta_attr/header_field/script）+ 后处理链；单字段失败不中断
- `source_layer.rs` — 4 tab 源码素材抓取（header/html/script/meta）
- `probe.rs` — 字段验证探针（结构化 ProbeError）
- `script_engine.rs` / `script_runner.rs` / `script_fetch.rs` — [046] rquickjs 沙箱（单字段 100ms 墙钟超时可配；ctx 注入 value/fields/url/fetch；SSRF 防护 + 1MB 上限；6 模式未匹配仍跑脚本，失败 hard fail 不覆盖旧值）。**rquickjs 不启用 `bindgen` feature**（避免 libclang 依赖，走预生成绑定）
- `refresh.rs` — [046] lazy refresh on read：字段级 `refresh_on_read` + 调用方 `force_refresh` 双层控制；管理性读取不触发，消费性读取按需重跑
- `preset_library.rs` / `templates.rs` — ≥20 类预置字段库 + 内置字段树模板（Discuz/WordPress/通用）
- `pan_detector.rs` — 9 平台网盘识别（quark/uc/baidu/tianyi/123pan/115/aliyun/xunlei/mobile），与 `link_checker.rs` 的 PanCheck 对齐
- `block_detector.rs` — 反爬拦截感知（403/429/503 + 登录墙/验证码/Cloudflare）；连续失败自动 `auto_blocked`

## API Routes

所有 API 以 `/api` 为前缀，`routes.rs` 按 **public → user(role≥1) → admin(role≥10) → root(role≥100)** 四层 Router merge 装配，中间件顺序 `X_guard ← auth_guard ← Extension(state)`。

### 公开路由
- `POST /api/auth/register|login|logout`；`GET /api/auth/register-status|captcha-status|captcha-image`（注册开关 + 登录验证码）
- `GET /api/status` — 系统状态（含 `schedulers.push_configs[]` 各配置独立调度视图 + `crawler` 调度状态）
- `GET /api/files/download/{filename}`；`GET /api/images/{photo_id}`、`/api/images/file/{file_id}`

### user 级（登录即可）
- `GET/PUT /api/auth/me`；`POST /api/auth/token`

### admin 级
- `/api/clients` — CRUD + 启停 + 认证（phone/code/password/bot_token）+ chats + bot-chats + validate-chat
- `/api/rules`、`/api/collectors` — CRUD + toggle + 触发/历史
- `/api/push` — trigger/stats/histories/retry/scheduler/config-check + **configs** 多推送配置（CRUD/toggle/duplicate/trigger/check-links）+ extract-config
- `/api/extract-histories`（+ `/stats`）— 提取历史（调度面板）
- `/api/resources` — 列表/批量提取/单条提取 `{history_id}`/stats/`{id}/detail`（提取对比）/`{id}/push`（推送测试）/`{id}/check-link`/CRUD
- `/api/image-forward` — queue / retry/{id} / retry-all
- `/api/crawler` — tasks CRUD + import/export/from-template + run/test + fetch-source/fetch-detail-sample/field-probe + templates + **field-tree/field-nodes（含 reorder）/field-stats** + field-library + articles（CRUD/batch-delete/links check/images retry/fields refresh/script-sandbox）+ histories（+stats）
- `/api/pan` — **accounts**（CRUD + check + diagnose）、**transfers**（CRUD + retry + cleanup）、**api-keys**（list/create/revoke/rotate）、**config**（get/put）

### root 级
- `/api/options` — 系统配置 + test-proxy / test-http-proxy / ai-test

### 开放转存 API（独立鉴权，非 session）
- `POST /api/v1/transfer/tasks`、`GET /api/v1/transfer/tasks/{id}`、`GET /api/v1/accounts` — `X-API-Key` header 鉴权（`middleware/api_key.rs`）

用户角色层级：Guest(0) < CommonUser(1) < Admin(10) < Root(100)

## Testing

```bash
cargo test                          # 全部测试
cargo test --test api_integration   # 集成测试（tests/ 下 3 个文件：api_integration / pan_account_tests / push_candidate_tests）
cargo test -- --nocapture           # 显示 println! 输出
```

测试使用 SQLite 内存数据库，集成测试通过 `tower::ServiceExt` / `axum-test` / `wiremock` 模拟 HTTP 请求。

### 关键测试锚点（改动对应模块时先看这些）
- `services::crawler::engine::tests::upsert_article_idempotent_on_repeated_calls` — 同 URL 重复抓取幂等（UNIQUE 索引）
- `services::crawler::extractor::tests` / `script_engine::tests` / `script_runner::tests` / `script_fetch::tests` — 7 模式提取、沙箱逃逸向量、SSRF 全分支
- `services::crawler::refresh::tests::t_should_refresh_truth_table` — [046] 刷新决策矩阵
- `handlers::crawler::field_stats_tests` — 命中率三态边界（healthy ≥0.80 / degraded / stale_warning <0.10）
- `tests/push_candidate_tests.rs` — [041] 推送候选集（多配置 is_pushed 语义）
- `tests/pan_account_tests.rs` — [047] 网盘账号凭据加密/校验

## Specs（specs/ 目录，历史设计文档）

存在的 spec：001-rust-rewrite、002-telegram-core-integration、003-completeness-audit、004-production-improvements、005-push-ai-extraction、006-refine-extraction-push、007-image-proxy-cache、008-push-universal-config、009-push-config-preview、016-resource-extraction-view、018-scheduler-dashboard、019-forward-queue-monitor、020-forward-rule-enhance、021-multi-push-config、022-resource-link-check、025-system-audit、026-fix-stage-atomicity、027-harden-default-secrets、028-fix-data-leak-pathtraversal、029-fix-error-swallow-retry、030-graceful-shutdown-bind、044-crawler-force-full-collect-toggle、045-crawler-pagination-enhance、**047-pan-account-management**（网盘账号管理与链接转存，含 contracts/transfer-api.md 与 admin-api.md）

注：早期引用的 010-013、015、017、023、039-043、046 等编号 spec 目录已删除；对应功能（注册开关、登录验证码、HTTP 代理分离、并发 AI 提取、两阶段转存、死信清理、爬虫 042/043/046）已合入代码，描述以本文件为准。
