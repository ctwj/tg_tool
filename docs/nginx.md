# Nginx 反向代理配置 — 图片代理缓存

本文档指导如何配置 Nginx 反向代理，将独立图片域名映射到 API 的图片接口，实现简化的图片访问 URL。

## 架构

```
浏览器 → https://img.example.com/{photo_id}          （自动选择客户端）
        → https://img.example.com/{client_id}/{photo_id}（指定客户端）
         → Nginx (443) → http://127.0.0.1:3000/api/images/{photo_id}
         → API 服务（检查本地缓存 → Telegram 客户端下载 → 返回图片）
```

## 基础配置

```nginx
server {
    listen 443 ssl http2;
    server_name img.example.com;

    # SSL 证书（使用 certbot / Let's Encrypt 自动获取）
    ssl_certificate     /etc/letsencrypt/live/img.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/img.example.com/privkey.pem;

    # SSL 优化
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;
    ssl_prefer_server_ciphers on;

    # 图片接口反向代理
    location / {
        proxy_pass http://127.0.0.1:3000/api/images/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # 超时设置（首次下载可能较慢）
        proxy_connect_timeout 30s;
        proxy_read_timeout 30s;

        # 缓存状态码透传
        proxy_intercept_errors off;
    }
}
```

## 可选：Nginx 本地代理缓存

在 Nginx 层增加一层本地缓存，减少对 API 的请求：

```nginx
# 在 http 块中添加缓存路径定义
http {
    proxy_cache_path /var/cache/nginx/images levels=1:2 keys_zone=image_cache:10m max_size=1g inactive=30d;
}

server {
    # ... (同上)

    location / {
        proxy_pass http://127.0.0.1:3000/api/images/;

        # 启用代理缓存
        proxy_cache image_cache;
        proxy_cache_valid 200 30d;
        proxy_cache_valid 404 1m;
        proxy_cache_valid 503 1m;

        # 添加缓存命中状态头（调试用）
        add_header X-Cache-Status $upstream_cache_status;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## 获取 SSL 证书

使用 certbot 自动获取和续期 Let's Encrypt 证书：

```bash
# 安装 certbot
apt install certbot python3-certbot-nginx

# 获取证书（先确保 DNS 已指向服务器）
certbot --nginx -d img.example.com

# 自动续期（certbot 会自动添加 cron）
certbot renew --dry-run
```

## HTTP → HTTPS 重定向

```nginx
server {
    listen 80;
    server_name img.example.com;
    return 301 https://$host$request_uri;
}
```

## 404 状态码透传

默认配置下 Nginx 会透传上游的状态码（404、503 等），确保 `proxy_intercept_errors off;`（默认值）即可。API 返回的 404（图片不存在）和 503（无可用客户端）会正确传递给客户端和 CDN。

## 验证

配置完成后验证：

```bash
# 检查 Nginx 配置语法
nginx -t

# 重载配置
nginx -s reload

# 测试图片访问（替换为实际 photo_id）
curl -I https://img.example.com/5123456789012345678

# 预期响应头：
# HTTP/2 200
# content-type: image/jpeg
# cache-control: public, max-age=2592000
# etag: "abc123..."
```
