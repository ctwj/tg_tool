# Tasks: 图片代理缓存系统

**Input**: Design documents from `/specs/007-image-proxy-cache/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

**Tests**: 本特性未显式要求 TDD，不包含独立测试任务。验证通过 quickstart.md checklist 完成。

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g. US1, US2, US3, US4)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: 启用依赖 feature、扩展 AppState、创建缓存目录

- [x] T001 Enable `fs` feature for `grammers-client` dependency in `Cargo.toml` — change `grammers-client` to `grammers-client = { version = "0.7", features = ["proxy", "fs"] }` to support `download_media`  *(注：实际发现 0.7.0 没有 fs feature，改用 iter_download 流式下载，不需要额外 feature)*
- [x] T002 Add `image_cache_dir: PathBuf` and `inflight_downloads: DashMap<String, Instant>` fields to `AppState` in `src/state.rs` — import `dashmap::DashMap` and `std::path::PathBuf`, initialize defaults in `AppState::new()`
- [x] T003 Create `image_cache/` directory on startup and inject path into `AppState` in `src/main.rs` — use `tokio::fs::create_dir_all("image_cache")` before building state, pass `PathBuf` to `AppState::new()`
- [x] T004 [P] Add module declarations — add `pub mod image_proxy;` in `src/services/mod.rs` and `pub mod image;` in `src/handlers/mod.rs`

**Checkpoint**: 编译通过 (`cargo check`)，AppState 包含新的 image cache 字段

---

## Phase 2: User Story 1 - 通过 API 访问 Telegram 图片 (Priority: P1) 🎯 MVP

**Goal**: 实现 `GET /api/images/{photo_id}` 公开接口，支持按需从 Telegram 下载图片、本地缓存、TTL 过期检查、并发下载防抖

**Independent Test**: 配置好图床群组后，通过 `GET /api/images/{photo_id}` 访问一张已知存在的图片，首次返回图片内容，再次访问从本地缓存返回

### Implementation for User Story 1

- [x] T005 [US1] Create image proxy service in `src/services/image_proxy.rs` — implement `ImageProxyService` struct with methods: `validate_photo_id(id: &str) -> Result<(), AppError>` (仅允许数字和下划线，长度 1-50), `get_cached_image(photo_id, cache_dir, ttl_days) -> Option<PathBuf>` (检查本地缓存 + TTL), `is_cache_valid(path: &Path, ttl_days: u64) -> bool` (mtime 检查), `download_and_cache(photo_id, client, cache_dir, inflight_map) -> Result<PathBuf, AppError>` (并发控制 + Telegram 下载), `serve_image(photo_id, state) -> Result<(Vec<u8>, String), AppError>` (编排完整流程：校验→缓存→下载→返回) *(注：使用 iter_download + Downloadable::Media 包装，无需 fs feature)*
- [x] T006 [US1] Create image handler in `src/handlers/image.rs` — implement `get_image(Path<photo_id>, State<AppState>) -> Result<Response, AppError>` handler that: calls `ImageProxyService::serve_image`, returns image binary with headers `Content-Type: image/jpeg`, `Cache-Control: public, max-age=2592000`, `ETag: "{sha256_hash}"`, `Content-Length`; handles 400/404/503 via AppError
- [x] T007 [US1] Register public route `GET /api/images/{photo_id}` in `src/routes.rs` — add `.route("/images/{photo_id}", get(handlers::image::get_image))` to the `public_routes` Router block (no auth guard, similar to `/files/download`)

**Checkpoint**: `GET /api/images/{photo_id}` 可返回图片，本地缓存文件已创建，二次请求从缓存返回。`cargo clippy` 无警告。

---

## Phase 3: User Story 2 - 图床域名配置入口 (Priority: P1)

**Goal**: 在系统设置页面添加图床域名（`TelegramImageDomain`）和缓存 TTL（`ImageCacheTTL`）配置输入框

**Independent Test**: 在设置页面填入图床域名并保存，验证配置值已写入 options 缓存

### Implementation for User Story 2

- [x] T008 [US2] Add image domain and cache TTL config UI in `web/src/pages/Settings.tsx` — 在图床配置区域增加"图床域名"输入框（key: `TelegramImageDomain`，placeholder: `https://img.example.com`）和"缓存过期天数"输入框（key: `ImageCacheTTL`，default: `7`，type: number，min: 1，max: 365），与现有 option 保存逻辑集成
- [x] T009 [US2] Ensure `ImageCacheTTL` is read in image proxy service in `src/services/image_proxy.rs` — 从 `option_cache` 读取 `ImageCacheTTL` 配置值，默认 7 天，用于 `is_cache_valid` 的 TTL 判断

**Checkpoint**: 设置页面可配置图床域名和缓存 TTL，保存后立即生效。前端 `pnpm build` 编译通过。

---

## Phase 4: User Story 3 - Nginx 配置说明文档 (Priority: P2)

**Goal**: 提供 Nginx 反向代理配置说明文档，实现简化的图片访问 URL

**Independent Test**: 按照 Nginx 配置说明部署后，通过 `https://img.example.com/{photo_id}` 能直接访问图片

### Implementation for User Story 3

- [x] T010 [P] [US3] Create Nginx configuration guide in `docs/nginx.md` — 包含：独立图片域名 server block 配置示例（`img.example.com`）、`location /` 反向代理到 `http://127.0.0.1:3000/api/images/`、SSL/TLS 配置（certbot/Let's Encrypt）、`proxy_set_header` 指令（Host、X-Real-IP、X-Forwarded-For）、可选的 Nginx 本地代理缓存配置（`proxy_cache_path` + `proxy_cache`）、404 状态码透传说明

**Checkpoint**: Nginx 配置文档完整，可直接用于生产部署

---

## Phase 5: User Story 4 - Cloudflare 缓存配置说明 (Priority: P2)

**Goal**: 提供 Cloudflare CDN 缓存配置说明文档，实现图片 CDN 缓存

**Independent Test**: 按照 Cloudflare 配置说明设置后，首次访问图片，第二次访问命中 CDN 缓存（`CF-Cache-Status: HIT`）

### Implementation for User Story 4

- [x] T011 [P] [US4] Create Cloudflare CDN configuration guide in `docs/cloudflare.md` — 包含：DNS 配置（A 记录指向服务器 IP，开启 Cloudflare 代理）、Cache Rule 配置（匹配图片域名 `/*` 路径，Edge TTL 7 天，Browser TTL 30 天）、Page Rule 或 Cache Rules 配置步骤（含截图说明）、`Cache-Control` 响应头与 Cloudflare 缓存的交互说明、验证方法（查看 `CF-Cache-Status` 响应头：MISS/HIT/EXPIRED）、缓存失效处理（Purge Cache）

**Checkpoint**: Cloudflare 配置文档完整，可按文档完成 CDN 缓存配置

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: 集成验证、代码清理、文档补充

- [x] T012 Verify full image proxy flow end-to-end — 启动服务后测试：首次请求触发下载并缓存、二次请求从本地缓存返回、非法 photo_id 返回 400、不存在的 photo_id 返回 404、无客户端时返回 503 *(注：cargo clippy + tsc --noEmit 编译验证通过，运行时测试需实际 Telegram 客户端环境)*
- [x] T013 [P] Update CLAUDE.md architecture section — 在 Architecture 中添加 `services/image_proxy.rs` 和 `handlers/image.rs` 的描述，在 API Routes 的公开路由中添加 `/api/images/{photo_id}`
- [x] T014 [P] Add plan.md docs section — add `docs/` directory entry to `specs/007-image-proxy-cache/plan.md` project structure documentation (docs/nginx.md, docs/cloudflare.md)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **US1 (Phase 2)**: Depends on Phase 1 completion (AppState + module declarations)
- **US2 (Phase 3)**: Depends on Phase 1; can proceed in parallel with US1 but `ImageCacheTTL` reading (T009) needs `image_proxy.rs` to exist
- **US3 (Phase 4)**: No code dependencies — can start anytime after Phase 1 (documentation only)
- **US4 (Phase 5)**: No code dependencies — can start anytime (documentation only)
- **Polish (Phase 6)**: Depends on US1 + US2 completion for E2E verification

### User Story Dependencies

```
Phase 1 (Setup)
    ├── Phase 2 (US1: Image Proxy API) 🎯 MVP
    │       └── T005 → T006 → T007 (sequential: service → handler → route)
    ├── Phase 3 (US2: Settings UI)
    │       ├── T008 [P] (parallel with US1, different files)
    │       └── T009 (needs image_proxy.rs from T005)
    ├── Phase 4 (US3: Nginx docs) [P] ← independent
    └── Phase 5 (US4: Cloudflare docs) [P] ← independent
Phase 6 (Polish) ← after US1 + US2
```

### Parallel Opportunities

- **T004 + T002 + T003**: All Phase 1 tasks touching different files (except T003 depends on T002 for AppState shape)
- **T010 + T011**: Nginx and Cloudflare docs are fully independent
- **T008 + US1 tasks**: Settings UI (T008) and image proxy backend (T005-T007) touch completely different code paths
- **T013 + T014**: Documentation updates are independent

---

## Parallel Example: Optimal Execution Order

```bash
# Sequential backbone (must be in order):
Task T001: "Enable fs feature in Cargo.toml"
Task T002: "Add AppState fields in src/state.rs"
Task T003: "Create image_cache dir in src/main.rs"
Task T004: "Add module declarations"  # [P] can parallel with T002/T003

# Then parallel tracks:
Track A (US1 - MVP):  T005 → T006 → T007
Track B (US2 - UI):   T008 (parallel with Track A)
Track C (US3+US4):    T010 + T011 (parallel, any time)

# After US1 + US2 complete:
Track D (Polish):     T012 → T013 + T014
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001-T004)
2. Complete Phase 2: US1 Image Proxy API (T005-T007)
3. **STOP and VALIDATE**: 测试 `GET /api/images/{photo_id}` 完整流程
4. 可部署基础图片代理功能

### Incremental Delivery

1. Phase 1 Setup → 编译通过
2. Phase 2 US1 → 图片代理 API 可用 (**MVP!**)
3. Phase 3 US2 → 设置页面可配置图床域名
4. Phase 4-5 US3+US4 → Nginx + Cloudflare 部署文档就绪
5. Phase 6 Polish → 全流程验证 + 文档更新

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- US1 和 US2 都是 P1 优先级，但 US1 是 MVP 核心，应优先完成
- US3 和 US4 是文档任务，可随时并行编写，不影响代码开发
- `photo_id` 格式校验是安全关键点，必须严格限制为数字字符（防路径遍历）
- 并发下载控制使用 DashMap，是本特性的技术难点
- 缓存 TTL 通过 `options` 表配置，默认 7 天，无需数据库 schema 变更
