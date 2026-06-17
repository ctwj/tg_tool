# Linux 服务器部署指南

> 本文档基于 GitHub Release 产物部署，服务器无需安装 Rust / Node.js 等编译工具。

## 1. 下载 Release 产物

从 GitHub Releases 页面下载对应平台的二进制文件：

```bash
# 创建部署目录
sudo mkdir -p /opt/tg_tool && cd /opt/tg_tool

# 下载最新 release（替换为实际版本号和仓库地址）
wget https://github.com/<owner>/<repo>/releases/download/v0.1.0/tgTool-x86_64-unknown-linux-musl -O tgTool
chmod +x tgTool
```

最终目录结构：

```
/opt/tg_tool/
├── tgTool              # 二进制文件
└── .env                # 环境配置（见下方）
```

## 2. PostgreSQL 数据库

### 2.1 安装 PostgreSQL

```bash
sudo apt update
sudo apt install -y postgresql postgresql-contrib
sudo systemctl enable --now postgresql
```

### 2.2 创建数据库和用户

```bash
sudo -u postgres psql
```

```sql
CREATE USER tgtool WITH PASSWORD 'your_strong_password';
CREATE DATABASE tgtool OWNER tgtool;
GRANT ALL PRIVILEGES ON DATABASE tgtool TO tgtool;
\q
```

> 程序启动时会自动执行数据库迁移，无需手动建表。

## 3. 环境配置

```bash
vim /opt/tg_tool/.env
```

**`.env` 完整配置：**

```env
# 日志级别：trace, debug, info, warn, error
RUST_LOG=info

# 服务端口
PORT=3000

# 日志目录（可选，不设则输出到 stdout/journald）
# LOG_DIR=./logs

# Telegram 客户端 session 存储路径
TG_STORE=./tg_store

# Telegram API 凭证（从 https://my.telegram.org 获取）
TG_APP_ID=你的APP_ID
TG_APP_HASH=你的APP_HASH

# PostgreSQL 数据库连接串（必填）
SQL_DSN=postgres://tgtool:your_strong_password@127.0.0.1:5432/tgtool

# Session 密钥（务必修改为随机字符串，用于 cookie 加密）
SESSION_SECRET=替换为一个随机字符串

# Telegram 代理（可选，格式：socks5://IP:端口）
# PROXY_URL=socks5://127.0.0.1:1080

# HTTP API 代理（可选，用于 AI 提取等 HTTP 请求）
# HTTP_PROXY_URL=http://127.0.0.1:7890

# 频率限制
RATE_LIMIT_MAX=100
RATE_LIMIT_WINDOW=60
```

## 4. systemd 服务

```bash
sudo vim /etc/systemd/system/tgtool.service
```

**`/etc/systemd/system/tgtool.service`：**

```ini
[Unit]
Description=Telegram Forwarding Tool
After=network.target postgresql.service
Requires=postgresql.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/tg_tool
ExecStart=/opt/tg_tool/tgTool
Restart=on-failure
RestartSec=5

# 日志输出到 journald
StandardOutput=journal
StandardError=journal
SyslogIdentifier=tgtool

# 安全限制
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

### 启动服务

```bash
sudo systemctl daemon-reload
sudo systemctl enable tgtool
sudo systemctl start tgtool

# 查看状态
sudo systemctl status tgtool

# 查看日志
sudo journalctl -u tgtool -f
```

## 5. Nginx 反向代理

```bash
sudo vim /etc/nginx/sites-available/tgtool
```

**完整 Nginx 配置（主站 + 图床子域名）：**

图床使用独立子域名（如 `img.example.com`），Nginx 将子域名根路径直接代理到后端 `/api/images/`，从而隐藏路径前缀。

图片访问效果：`https://img.example.com/file/{file_id}` → 后端 `/api/images/file/{file_id}`（推荐，按 Bot file_id 直接下载）

```nginx
# 主站
server {
    listen 80;
    server_name app.example.com;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

# 图床子域名 — 将 /file/{file_id} 代理到后端 /api/images/file/{file_id}
server {
    listen 80;
    server_name img.example.com;

    location / {
        proxy_pass http://127.0.0.1:3000/api/images/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

启用并生效：

```bash
sudo ln -s /etc/nginx/sites-available/tgtool /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

### HTTPS（推荐）

```bash
sudo apt install -y certbot python3-certbot-nginx
sudo certbot --nginx -d app.example.com -d img.example.com
```

> 如果使用 Cloudflare CDN，可在 Cloudflare 面板配置 SSL，无需 certbot。

### 图床域名系统设置

Nginx 配置完成后，登录管理后台进行图床配置：

1. 访问 `https://app.example.com`，使用 root 账号登录
2. 进入 **系统设置 → 图床配置**
3. **图床域名** 填写图床子域名（不带尾部斜杠）：`https://img.example.com`
4. 设置 **图片缓存过期天数**（默认 7 天）
5. 点击 **保存图床配置**

配置完成后，图片 URL 拼接规则为 `${图床域名}/file/${file_id}`，即 `https://img.example.com/file/{file_id}`。
其中 `file_id` 为图片两阶段转存后由 Bot 获取（未开启转存或转存未完成的资源无 file_id，其图片 URL 为空）。
可在页面下方的图片预览区域输入 file_id 测试是否生效。

## 6. 验证

```bash
curl http://127.0.0.1:3000/api/status
```

浏览器访问 `http://your-domain.com`，默认账号：

- 用户名：`root`
- 密码：`123456`

**首次登录后请立即修改默认密码。**

## 7. 更新部署

```bash
cd /opt/tg_tool

# 下载新版本
wget https://github.com/<owner>/<repo>/releases/download/v0.2.0/tgTool-x86_64-unknown-linux-musl -O tgTool
chmod +x tgTool

# 重启服务
sudo systemctl restart tgtool
```

## 8. 备份

### 数据库

```bash
sudo -u postgres pg_dump tgtool > /backup/tgtool_$(date +%Y%m%d).sql

# 恢复
# sudo -u postgres psql tgtool < /backup/tgtool_20260101.sql
```

### Session 文件

Telegram session 存储在 `tg_store/` 目录，建议定期备份：

```bash
tar czf /backup/tg_store_$(date +%Y%m%d).tar.gz /opt/tg_tool/tg_store/
```

## 9. 常见问题

### 端口被占用

修改 `.env` 中的 `PORT` 后重启。

### 日志排查

```bash
sudo journalctl -u tgtool -f          # 实时日志
sudo journalctl -u tgtool -n 200      # 最近 200 行
```

调整日志级别：在 `.env` 中设置 `RUST_LOG=debug` 后重启。

### 数据库连接失败

1. 确认 PostgreSQL 正在运行：`sudo systemctl status postgresql`
2. 确认 `pg_hba.conf` 允许本地连接（默认已允许）
3. 确认 `.env` 中 `SQL_DSN` 的用户名、密码、数据库名正确

### 目录权限

确保运行用户对以下目录有读写权限：
- `tg_store/`（程序自动创建）
- `image_cache/`（程序自动创建）
