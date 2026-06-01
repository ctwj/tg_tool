# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram 消息转发工具，包含两个子项目：
- **根目录**: Rust 项目 (`src/main.rs`)，目前为初始脚手架（仅 Hello World），`Cargo.toml` 使用 edition 2024
- **telegram-forwarding/**: 核心应用，基于 Go (Gin) + React 的 Telegram 消息转发系统，使用 tdlib 与 Telegram 交互

核心功能：Telegram 账号管理、消息采集（Collector）、转发规则（Rule）、网盘链接检测与转存（Quark）、消息推送调度

## Build & Run Commands

### Go 后端 (telegram-forwarding/)

```bash
# 进入后端目录
cd telegram-forwarding

# 下载依赖
go mod download

# 编译（需要 tdlib 库）
CGO_CFLAGS="-I/usr/local/include" \
CGO_LDFLAGS="-L/usr/local/lib -Wl,-rpath,'$ORIGIN/lib' -ltdjson" \
go build -ldflags="-X common.Version=v1.2.3" -o telegramForwarding main.go

# 运行
./telegramForwarding --port 3000 --log-dir ./logs

# 热重载开发（使用 Air）
air -c .air.tomal
```

**tdlib 编译依赖**：需要 `libtdjson.so` 在 `/usr/local/lib/`，Ubuntu 需安装 `libc++-dev`

### React 前端 (telegram-forwarding/web/)

```bash
cd telegram-forwarding/web
npm install
npm start      # 开发服务器，代理到 localhost:3000
npm run build  # 生产构建，输出到 build/
```

### Rust 项目（根目录）

```bash
cargo build
cargo run
```

## Architecture

### Go 后端分层 (telegram-forwarding/)

```
main.go → router/ → controller/ → pkg/service/ → db/repo/
                        ↓                ↓
                   pkg/tglib/     pkg/interf/ (接口定义)
                        ↓
                   db/entity/ (GORM 模型)
```

- **router/**: 路由注册，`SetApiRouter()` 负责所有 API 路由，`setWebRouter()` 处理前端静态资源
- **controller/**: HTTP 处理层，通过构造函数注入依赖（repo/service）
- **pkg/service/**: 业务逻辑层（`ClientService`, `MessageService`），面向接口编程
- **pkg/interf/**: 核心接口定义（`IClientService`, `IMessageService`, `ITgApi`, `ITgClient`, 各 Repository 接口）
- **pkg/tglib/**: tdlib 封装，Telegram 客户端生命周期管理、消息监听、认证流程（`authorizer.go`）
- **db/repo/**: 数据访问层，每个实体对应一个 Repo 结构体
- **db/entity/**: GORM 实体定义
- **middleware/**: 认证（`UserAuth`, `AdminAuth`, `RootAuth`）、CORS、频率限制、Cloudflare Turnstile 校验
- **common/**: 全局配置、日志、工具函数、Redis/JWT/加密等

### 数据库

默认 SQLite，通过 `SQL_DSN` 环境变量可切换到 PostgreSQL。使用 GORM ORM，自动迁移。初始 root 用户: `root / 123456`

### 前端

React 18 + Semantic UI React，构建产物通过 Go 的 `embed.FS` 嵌入到二进制文件中

### 关键配置

环境变量见 `.env.example`，重要的有：
- `SQL_DSN` — PostgreSQL 连接串
- `REDIS_CONN_STRING` — Redis（用于频率限制和 Session 存储）
- `TG_STORE` / `TG_APP_ID` / `TG_APP_HASH` — tdlib 配置
- `GIN_MODE` — `debug` 或 `release`

## API Routes

所有 API 路径以 `/api` 为前缀：
- `/api/tg/*` — Telegram 客户端管理（需要 Admin 权限）
- `/api/rule/*` — 转发规则 CRUD（需要 Admin 权限）
- `/api/collector/*` — 采集器 CRUD + 消息拉取（需要 Admin 权限）
- `/api/tool/*` — 工具接口（网盘检测、Quark 转存、消息提取、推送调度）
- `/api/user/*` — 用户管理（注册/登录/Token 认证）
- `/api/file/*` — 文件管理（需要 Admin 权限）
- `/api/option/*` — 系统配置（需要 Root 权限）

用户角色层级：Guest(0) < CommonUser(1) < Admin(10) < Root(100)

## Go Module 注意事项

Go module 名为 `gin-template`（源自上游模板），所有 import 路径使用 `gin-template/...`

## Rust 重写计划

当前正在进行 Go → Rust 全栈重写（后端 + 前端），详细实现计划见 `specs/001-rust-rewrite/plan.md`。

技术栈：
- 后端: axum + grammers-client + sqlx + tokio
- 前端: React 18 + TypeScript + Vite + Ant Design（全新开发）
- 已移除: 网盘检测/转存模块（Quark）
- 全新 API 设计，无数据迁移约束

原 Go 源码参考在 `demo/` 和 `telegram-forwarding/` 目录中。
