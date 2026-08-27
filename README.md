# TG Tool

> Telegram 消息转发工具 — 基于 Rust 全栈重写

TG Tool 是一个 Telegram 消息转发管理系统，支持多账号管理、消息采集、转发规则配置、消息推送调度等功能。提供 Web UI 管理界面，可通过 Docker 一键部署。

## 功能特性

- 🤖 **多 Telegram 客户端管理** — 支持同时管理多个 Telegram 账号
- 📨 **消息转发规则** — 基于来源频道/群组匹配，支持 Chat 转发和 Webhook 推送
- 📡 **消息采集器** — 支持全量历史采集和实时消息监听
- 🚀 **推送调度** — 定时批量推送采集内容到外部 API
- 👥 **用户权限管理** — Guest / User / Admin / Root 四级角色
- 🔐 **JWT + Session 双认证** — 支持浏览器 Session 和 API Token 访问
- 🗄️ **SQLite / PostgreSQL** — 默认 SQLite 零配置，支持切换 PostgreSQL
- 🐳 **Docker 部署** — 多阶段构建，镜像小巧
- 🌐 **嵌入式前端** — React 构建产物嵌入二进制，单文件部署

## 技术栈

### 后端

| 组件 | 技术 |
|------|------|
| Web 框架 | [axum](https://github.com/tokio-rs/axum) 0.8 |
| 异步运行时 | [tokio](https://github.com/tokio-rs/tokio) |
| 数据库 | [sqlx](https://github.com/launchbadge/sqlx) (SQLite / PostgreSQL) |
| Telegram 客户端 | [grammers-client](https://github.com/Lonami/grammers) |
| 认证 | bcrypt + JWT (jsonwebtoken) |
| HTTP 客户端 | reqwest |
| 配置管理 | clap + dotenvy |
| 日志 | tracing + tracing-subscriber |

### 前端（规划中）

React 18 + TypeScript + Vite + Ant Design

## 快速开始

### 环境要求

- Rust 1.85+ (edition 2024)
- Node.js 20+（前端构建）
- SQLite 3 或 PostgreSQL（可选）

### 本地开发

```bash
# 克隆仓库
git clone https://github.com/your-username/tgTool.git
cd tgTool

# 复制环境变量配置
cp .env.example .env
# 编辑 .env 填入 Telegram API 凭据（TG_APP_ID、TG_APP_HASH）

# 构建运行
cargo run
```

服务默认监听 `http://localhost:3000`。首次启动会自动创建管理员账号 `root`，密码为**随机生成的强口令**（安全加固后不再是 `123456`），只在首次创建用户的那次启动日志中打印一次：

```bash
# 在启动日志中查找初始口令（tracing 输出到 stderr）
cargo run 2>&1 | grep 初始随机口令
# Docker 部署
docker logs <容器名> 2>&1 | grep 初始随机口令
```

**首次登录后请立即修改密码。** 若初始口令日志已丢失，参见 [重置 root 密码](#重置-root-密码)。

### 前端开发

```bash
cd web
npm install
npm start        # 开发服务器，代理到 localhost:3000
npm run build    # 生产构建，输出到 web/dist/
```

### Docker 部署

```bash
# 构建镜像（包含前端）
docker build -t tgtool .

# 运行
docker run -d \
  -p 3000:3000 \
  -v ./data:/app/data \
  -v ./tg_store:/app/tg_store \
  -e TG_APP_ID=your_app_id \
  -e TG_APP_HASH=your_app_hash \
  -e SESSION_SECRET=your-random-secret \
  tgtool
```

### Docker Compose

```yaml
version: "3.8"
services:
  tgtool:
    build: .
    ports:
      - "3000:3000"
    volumes:
      - ./data:/app/data
      - ./tg_store:/app/tg_store
    environment:
      - TG_APP_ID=your_app_id
      - TG_APP_HASH=your_app_hash
      - SESSION_SECRET=your-random-secret
      - RUST_LOG=info
    restart: unless-stopped
```

## 重置 root 密码

初始随机口令只在首次创建 root 用户的**那一次启动**中打印（之后再启动不会重复输出）。若日志已丢失，按以下任一方式重置。

### 方式一：重新初始化（⚠️ 会清空全部数据）

适合刚部署、还没有业务数据的场景：

```bash
rm data.db    # SQLite 默认路径；Docker 部署为容器内 /app/data.db
```

重启（或重建容器）后会重新创建 root，并在启动日志中再次打印新的随机口令。

### 方式二：保留数据，直接改库重置

**本地 / 裸二进制部署：**

```bash
# 1. 生成新密码的 bcrypt hash（需 python + bcrypt：pip install bcrypt）
python -c "import bcrypt; print(bcrypt.hashpw(b'新密码', bcrypt.gensalt()).decode())"

# 2. 写入 root 账号
sqlite3 data.db "UPDATE users SET password='<第 1 步生成的 hash>', must_change_password=0 WHERE username='root';"
```

**Docker 部署**（容器内无 sqlite3，先停容器再拷出来改）：

```bash
docker stop <容器名>
docker cp <容器名>:/app/data.db ./data.db
# ...执行上面两条命令...
docker cp ./data.db <容器名>:/app/data.db
docker start <容器名>
```

> PostgreSQL 部署：用 `psql` 执行同样的 UPDATE，布尔值写 `FALSE` 而非 `0`。

## 配置项

所有配置项可通过环境变量或 `.env` 文件设置，也支持命令行参数。

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `PORT` | `3000` | 服务端口 |
| `RUST_LOG` | `info` | 日志级别 (trace/debug/info/warn/error) |
| `LOG_DIR` | — | 日志目录（可选） |
| `TG_STORE` | `./tg_store` | Telegram 客户端 session 存储路径 |
| `TG_APP_ID` | — | Telegram API App ID（[获取地址](https://my.telegram.org)） |
| `TG_APP_HASH` | — | Telegram API App Hash |
| `SQL_DSN` | 空（SQLite） | 数据库连接串，留空使用 SQLite，设为 `postgres://...` 切换 PostgreSQL |
| `REDIS_CONN_STRING` | 空（内存） | Redis 连接串，留空使用内存存储 |
| `SESSION_SECRET` | `change-me-...` | Session 密钥，**生产环境务必修改** |

## API 概览

所有 API 路径以 `/api` 为前缀。

| 路径 | 方法 | 说明 | 权限 |
|------|------|------|------|
| `/api/auth/register` | POST | 用户注册 | 公开 |
| `/api/auth/login` | POST | 用户登录 | 公开 |
| `/api/auth/logout` | POST | 退出登录 | 公开 |
| `/api/auth/me` | GET/PUT | 获取/更新当前用户信息 | 用户 |
| `/api/auth/token` | POST | 生成 API Token | 用户 |
| `/api/status` | GET | 系统状态 | 公开 |
| `/api/clients` | GET/POST | 列出/添加 Telegram 客户端 | 管理员 |
| `/api/clients/{id}` | GET/DELETE | 客户端状态/删除 | 管理员 |
| `/api/clients/{id}/start` | POST | 启动客户端 | 管理员 |
| `/api/clients/{id}/stop` | POST | 停止客户端 | 管理员 |
| `/api/clients/{id}/auth` | POST | 客户端认证 | 管理员 |
| `/api/clients/{id}/chats` | GET | 获取聊天列表 | 管理员 |
| `/api/rules` | GET/POST | 列出/创建转发规则 | 管理员 |
| `/api/rules/{id}` | GET/PUT/DELETE | 规则详情/更新/删除 | 管理员 |
| `/api/rules/{id}/toggle` | PUT | 切换规则启用状态 | 管理员 |
| `/api/collectors` | GET/POST | 列出/创建采集器 | 管理员 |
| `/api/collectors/{id}` | GET/PUT/DELETE | 采集器详情/更新/删除 | 管理员 |
| `/api/collectors/{id}/toggle` | PUT | 切换采集器状态 | 管理员 |
| `/api/collectors/{id}/fetch` | POST | 触发全量采集 | 管理员 |
| `/api/push/trigger` | POST | 触发推送 | 用户 |
| `/api/push/stats` | GET | 推送统计 | 用户 |
| `/api/push/histories` | GET | 推送历史 | 用户 |
| `/api/push/scheduler` | PUT | 更新调度配置 | 管理员 |
| `/api/users` | GET/POST | 用户列表/创建 | 管理员 |
| `/api/users/{id}` | GET/PUT/DELETE | 用户详情/更新/删除 | 管理员 |
| `/api/files` | GET/POST | 文件列表/上传 | 管理员 |
| `/api/files/{id}` | DELETE | 删除文件 | 管理员 |
| `/api/options` | GET/PUT | 系统配置 | Root |

### 用户角色

| 角色 | 等级 | 说明 |
|------|------|------|
| Guest | 0 | 访客 |
| CommonUser | 1 | 普通用户 |
| Admin | 10 | 管理员 |
| Root | 100 | 超级管理员 |

## 项目结构

```
tgTool/
├── Cargo.toml              # Rust 项目配置
├── Dockerfile              # Docker 多阶段构建
├── .env.example            # 环境变量模板
├── migrations/
│   └── 001_init.sql        # 数据库建表语句
├── src/
│   ├── main.rs             # 入口：配置加载、数据库初始化、服务启动
│   ├── lib.rs              # 库入口（模块导出）
│   ├── config.rs           # 配置管理 (clap)
│   ├── errors.rs           # 统一错误类型
│   ├── state.rs            # 应用共享状态 (AppState, DbPool, TgClientMap)
│   ├── embed.rs            # 前端静态文件嵌入 (rust-embed)
│   ├── routes.rs           # 路由注册
│   ├── models/             # 数据模型 (sqlx FromRow)
│   │   ├── user.rs         # 用户模型、登录/注册请求
│   │   ├── client.rs       # Telegram 客户端模型
│   │   ├── rule.rs         # 转发规则模型
│   │   ├── collector.rs    # 采集器模型
│   │   ├── message.rs      # 转发消息模型
│   │   ├── collector_history.rs
│   │   ├── push_history.rs
│   │   ├── file.rs
│   │   └── option.rs       # 系统配置模型
│   ├── handlers/           # HTTP 请求处理器
│   │   ├── auth.rs         # 注册/登录/登出
│   │   ├── client.rs       # 客户端管理
│   │   ├── rule.rs         # 规则 CRUD
│   │   ├── collector.rs    # 采集器 CRUD
│   │   ├── push.rs         # 推送管理
│   │   ├── user.rs         # 用户管理
│   │   ├── file.rs         # 文件管理
│   │   ├── option.rs       # 系统配置
│   │   ├── misc.rs         # 系统状态
│   │   └── response.rs     # 响应构建工具
│   ├── middleware/          # 中间件
│   │   ├── auth.rs         # JWT/Session 认证
│   │   ├── cors.rs         # CORS
│   │   ├── rate_limit.rs   # 频率限制
│   │   └── session.rs      # Session 管理
│   └── services/           # 业务逻辑
│       ├── crypto.rs       # 密码哈希、JWT、Token 生成
│       ├── tg_manager.rs   # Telegram 客户端生命周期管理
│       ├── tg_api.rs       # Telegram API 封装
│       ├── tg_auth.rs      # Telegram 认证流程
│       ├── message_handler.rs  # 消息监听与分发
│       ├── forwarder.rs    # 消息转发
│       ├── collector.rs    # 消息采集
│       ├── push.rs         # 推送服务
│       └── scheduler.rs    # 定时调度
├── web/                    # 前端项目 (React + TypeScript + Vite)
└── tests/
    └── api_integration.rs  # API 集成测试 (SQLite 内存数据库)
```

## 测试

```bash
# 运行所有测试（71 个单元测试 + 18 个集成测试）
cargo test

# 仅运行单元测试
cargo test --lib

# 仅运行集成测试
cargo test --test api_integration

# 运行并显示输出
cargo test -- --nocapture
```

## 构建 Release

```bash
# 本地构建
cargo build --release

# 产出文件位于 target/release/tgTool（或 Windows 下 tgTool.exe）
```

## License

MIT
