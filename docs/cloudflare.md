# Cloudflare CDN 缓存配置 — 图片代理

本文档指导如何配置 Cloudflare CDN 缓存规则，使图片在首次访问后被 CDN 边缘节点缓存，减少回源请求。

## 架构

```
浏览器 → Cloudflare CDN（缓存命中 → 直接返回）
              ↓ 缓存未命中
         Nginx → API /api/images/{photo_id}
              ↓ 本地缓存未命中
         Telegram 客户端下载 → 缓存到本地 → 返回图片
```

## 前提条件

- 图片域名（如 `img.example.com`）已配置 Nginx 反向代理（参考 [Nginx 配置文档](./nginx.md)）
- API 响应头已包含 `Cache-Control: public, max-age=2592000` 和 `ETag`
- Cloudflare 账号（免费版即可）

## 步骤 1：DNS 配置

1. 登录 Cloudflare Dashboard
2. 选择你的域名（如 `example.com`）
3. 进入 **DNS** → **Records**
4. 添加 A 记录：
   - Type: `A`
   - Name: `img`（即 `img.example.com`）
   - IPv4 address: 你的服务器 IP
   - Proxy status: **Proxied**（橙色云朵 ☁️）— 必须开启代理才能使用 CDN 缓存

## 步骤 2：SSL/TLS 配置

1. 进入 **SSL/TLS** → **Overview**
2. 加密模式选择 **Full (strict)**
   - 这要求源站（Nginx）配置了有效的 SSL 证书（如 Let's Encrypt）

## 步骤 3：配置 Cache Rule

1. 进入 **Caching** → **Configuration** → **Cache Rules**
2. 点击 **Create rule**
3. 配置规则：

   **规则名称**：`Image Cache`

   **匹配条件**：
   - Field: `Hostname`
   - Operator: `equals`
   - Value: `img.example.com`

   **缓存设置**：
   - Cache eligibility: **Eligible for cache**
   - Edge TTL: **7 days**
   - Browser TTL: **30 days**

4. 点击 **Deploy**

### 替代方案：使用 Page Rule（旧版）

如果没有 Cache Rule 功能，可以使用 Page Rule：

1. 进入 **Rules** → **Page Rules**
2. 创建规则：
   - URL: `img.example.com/*`
   - Setting: **Cache Level** = Cache Everything
   - Setting: **Edge Cache TTL** = 7 days
   - Setting: **Browser Cache TTL** = 30 days

## Cache-Control 与 Cloudflare 的交互

API 返回的 `Cache-Control` 头决定了 Cloudflare 的缓存行为：

| 响应头 | 效果 |
|--------|------|
| `Cache-Control: public, max-age=2592000` | Cloudflare 缓存 30 天（除非 Cache Rule 覆盖） |
| `Cache-Control: private` 或 `no-store` | Cloudflare 不缓存 |
| 无 `Cache-Control` 头 | Cloudflare 使用默认缓存规则（2 小时） |

配置了 Cache Rule 后，Edge TTL 以 Cache Rule 中配置的为准，不受 `max-age` 限制。

## 验证

配置完成后，使用 `curl` 验证缓存行为：

```bash
# 首次访问 — 缓存未命中
curl -I https://img.example.com/5123456789012345678
# 响应头应包含：
# CF-Cache-Status: MISS

# 第二次访问 — 缓存命中
curl -I https://img.example.com/5123456789012345678
# 响应头应包含：
# CF-Cache-Status: HIT
```

### CF-Cache-Status 含义

| 状态 | 说明 |
|------|------|
| `MISS` | 未命中缓存，回源获取 |
| `HIT` | 命中缓存，直接返回 |
| `EXPIRED` | 缓存已过期，回源刷新 |
| `BYPASS` | 缓存被跳过 |
| `STALE` | 返回过期缓存的同时后台刷新 |
| `REVALIDATED` | 通过 ETag 条件请求验证缓存仍有效 |

## 缓存失效处理

如果需要强制刷新某张图片的 CDN 缓存：

### 方法 1：Cloudflare Dashboard 手动清除

1. 进入 **Caching** → **Configuration**
2. 点击 **Purge Everything**（清除所有缓存）或 **Custom Purge**
3. Custom Purge 输入完整 URL：`https://img.example.com/{photo_id}`

### 方法 2：API 清除

```bash
# 清除指定 URL 的缓存
curl -X POST "https://api.cloudflare.com/client/v4/zones/{zone_id}/purge_cache" \
  -H "Authorization: Bearer {api_token}" \
  -H "Content-Type: application/json" \
  --data '{"files":["https://img.example.com/5123456789012345678"]}'
```

## 注意事项

- **免费版限制**：Cache Rule 和 Page Rule 数量有限，请合理规划规则
- **图片更新**：同一 `photo_id` 对应的图片内容通常不会变化，适合长缓存
- **源站 TTL**：API 端本地缓存 TTL 默认 7 天，过期后会重新从 Telegram 下载。即使 CDN 缓存过期，API 仍可从本地缓存快速响应
- **ETag 支持**：API 返回 `ETag` 头，Cloudflare 会使用条件请求验证缓存有效性
