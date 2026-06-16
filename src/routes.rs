use crate::handlers;
use crate::models::user::User;
use crate::state::AppState;
use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};

/// 内联 auth 中间件 — 检查请求是否携带有效认证信息
async fn auth_guard(req: Request, next: Next) -> Response {
    let state = match req.extensions().get::<AppState>().cloned() {
        Some(s) => s,
        None => {
            return crate::errors::AppError::Internal("服务器配置错误：AppState 缺失".into())
                .into_response();
        }
    };

    // 在 await 之前提取所有需要的 headers 数据，避免跨 await 持有 &Request
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let cookie_header = req
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let auth_result = crate::middleware::auth::extract_current_user_from_parts(
        &state,
        auth_header.as_deref(),
        cookie_header.as_deref(),
    )
    .await;

    let mut req = req;
    match auth_result {
        Ok(user) => {
            // feature 027 SEC-002：must_change 用户仅放行改密相关端点（/auth/me），其余受保护端点拒绝
            if user.must_change_password {
                let path = req.uri().path();
                let allow_change_password = path.ends_with("/auth/me");
                if !allow_change_password {
                    return crate::errors::AppError::Forbidden(
                        "账号需先修改默认密码后才能进行此操作".into(),
                    )
                    .into_response();
                }
            }
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

/// Admin 权限中间件 — 要求 role >= 10 (Admin)
/// 必须在 auth_guard 之后使用，因为依赖 Extension<User>
async fn admin_guard(req: Request, next: Next) -> Response {
    let user = req.extensions().get::<User>();
    match user {
        Some(u) if u.role >= 10 => next.run(req).await,
        _ => crate::errors::AppError::Forbidden("需要管理员权限".into()).into_response(),
    }
}

/// Root 权限中间件 — 要求 role >= 100 (Root)
async fn root_guard(req: Request, next: Next) -> Response {
    let user = req.extensions().get::<User>();
    match user {
        Some(u) if u.role >= 100 => next.run(req).await,
        _ => crate::errors::AppError::Forbidden("需要 Root 权限".into()).into_response(),
    }
}

pub fn build_router(state: AppState) -> Router {
    // Public routes — no auth required
    let public_routes = Router::new()
        .route("/auth/register", post(handlers::auth::register))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/logout", post(handlers::auth::logout))
        .route(
            "/auth/register-status",
            get(handlers::auth::register_status),
        )
        .route("/auth/captcha-status", get(handlers::auth::captcha_status))
        .route("/auth/captcha-image", get(handlers::auth::captcha_image))
        .route("/status", get(handlers::misc::system_status))
        .route(
            "/files/download/{filename}",
            get(handlers::file::download_file),
        )
        .route("/images/{photo_id}", get(handlers::image::get_image))
        .route(
            "/images/file/{file_id}",
            get(handlers::image::get_image_by_file_id),
        )
        .route(
            "/images/{client_id}/{photo_id}",
            get(handlers::image::get_image_with_client),
        );

    // User-level routes — require login (role >= 1)
    let user_routes = Router::new()
        // Auth (protected)
        .route(
            "/auth/me",
            get(handlers::auth::get_me).put(handlers::auth::update_me),
        )
        .route("/auth/token", post(handlers::auth::generate_api_token))
        .layer(middleware::from_fn(auth_guard))
        .layer(axum::Extension(state.clone()));

    // Admin-level routes — require role >= 10
    let admin_routes = Router::new()
        // Clients
        .route(
            "/clients",
            get(handlers::client::list_clients).post(handlers::client::add_client),
        )
        .route(
            "/clients/{id}",
            delete(handlers::client::remove_client).get(handlers::client::get_client_status),
        )
        .route("/clients/{id}/start", post(handlers::client::start_client))
        .route("/clients/{id}/stop", post(handlers::client::stop_client))
        .route("/clients/{id}/auth", post(handlers::client::auth_client))
        .route("/clients/{id}/chats", get(handlers::client::get_chats))
        .route(
            "/clients/{id}/bot-chats",
            get(handlers::client::get_bot_chats),
        )
        .route(
            "/clients/{id}/validate-chat",
            post(handlers::client::validate_bot_chat),
        )
        // Rules
        .route(
            "/rules",
            get(handlers::rule::list_rules).post(handlers::rule::create_rule),
        )
        .route(
            "/rules/{id}",
            get(handlers::rule::get_rule)
                .put(handlers::rule::update_rule)
                .delete(handlers::rule::delete_rule),
        )
        .route("/rules/{id}/toggle", put(handlers::rule::toggle_rule))
        .route(
            "/rules/{id}/messages",
            get(handlers::rule::get_rule_messages),
        )
        // Collectors
        .route(
            "/collectors",
            get(handlers::collector::list_collectors).post(handlers::collector::create_collector),
        )
        .route(
            "/collectors/histories",
            get(handlers::collector::list_histories),
        )
        .route(
            "/collectors/{id}",
            get(handlers::collector::get_collector)
                .put(handlers::collector::update_collector)
                .delete(handlers::collector::delete_collector),
        )
        .route(
            "/collectors/{id}/toggle",
            put(handlers::collector::toggle_collector),
        )
        .route(
            "/collectors/{id}/fetch",
            post(handlers::collector::fetch_history),
        )
        // Push
        .route("/push/trigger", post(handlers::push::trigger_push))
        .route("/push/stats", get(handlers::push::get_stats))
        .route("/push/histories", get(handlers::push::list_histories))
        .route(
            "/push/histories/{id}",
            get(handlers::push::get_push_history_detail),
        )
        .route("/push/retry", post(handlers::push::retry_push))
        .route("/push/scheduler", put(handlers::push::update_scheduler))
        .route("/push/config-check", get(handlers::push::config_check))
        // Push Configs (多推送配置管理)
        .route(
            "/push/configs",
            get(handlers::push::list_push_configs).post(handlers::push::create_push_config),
        )
        .route(
            "/push/configs/{id}",
            get(handlers::push::get_push_config)
                .put(handlers::push::update_push_config)
                .delete(handlers::push::delete_push_config),
        )
        .route(
            "/push/configs/{id}/toggle",
            put(handlers::push::toggle_push_config),
        )
        .route(
            "/push/configs/{id}/duplicate",
            post(handlers::push::duplicate_push_config),
        )
        .route(
            "/push/configs/{id}/trigger",
            post(handlers::push::trigger_push_for_config),
        )
        .route(
            "/push/configs/{id}/check-links",
            post(handlers::push::check_links_for_config),
        )
        .route(
            "/push/extract-config",
            put(handlers::resource::update_extract_config),
        )
        // Extract histories (scheduler dashboard)
        .route(
            "/extract-histories",
            get(handlers::scheduler::list_extract_histories),
        )
        .route(
            "/extract-histories/stats",
            get(handlers::scheduler::get_extract_histories_stats),
        )
        // Resources
        .route("/resources", get(handlers::resource::list_resources))
        .route(
            "/resources/extract",
            post(handlers::resource::extract_resources),
        )
        .route(
            "/resources/extract/{history_id}",
            post(handlers::resource::extract_single),
        )
        .route(
            "/resources/stats",
            get(handlers::resource::get_resource_stats),
        )
        .route(
            "/resources/{id}/detail",
            get(handlers::resource::get_resource_detail),
        )
        .route(
            "/resources/{id}/push",
            post(handlers::resource::push_resource),
        )
        .route(
            "/resources/{id}/check-link",
            post(handlers::resource::check_link),
        )
        .route(
            "/resources/{id}",
            get(handlers::resource::get_resource)
                .put(handlers::resource::update_resource)
                .delete(handlers::resource::delete_resource),
        )
        // Users (admin can list/create/manage users)
        .route(
            "/users",
            get(handlers::user::list_users).post(handlers::user::create_user),
        )
        .route(
            "/users/{id}",
            get(handlers::user::get_user)
                .put(handlers::user::update_user)
                .delete(handlers::user::delete_user),
        )
        // Files
        .route(
            "/files",
            get(handlers::file::list_files).post(handlers::file::upload_file),
        )
        .route("/files/{id}", delete(handlers::file::delete_file))
        // Image forward queue
        .route(
            "/image-forward/queue",
            get(handlers::image_forward::queue_status),
        )
        .route(
            "/image-forward/retry/{id}",
            post(handlers::image_forward::retry_task),
        )
        .route(
            "/image-forward/retry-all",
            post(handlers::image_forward::retry_all),
        )
        .layer(middleware::from_fn(admin_guard))
        .layer(middleware::from_fn(auth_guard))
        .layer(axum::Extension(state.clone()));

    // Root-level routes — require role >= 100
    let root_routes = Router::new()
        // Options (system configuration)
        .route(
            "/options",
            get(handlers::option::get_options).put(handlers::option::update_options),
        )
        .route("/options/test-proxy", post(handlers::option::test_proxy))
        .route(
            "/options/test-http-proxy",
            post(handlers::option::test_http_proxy),
        )
        .route("/options/ai-test", post(handlers::option::test_ai_endpoint))
        .layer(middleware::from_fn(root_guard))
        .layer(middleware::from_fn(auth_guard))
        .layer(axum::Extension(state.clone()));

    Router::new()
        .nest(
            "/api",
            public_routes
                .merge(user_routes)
                .merge(admin_routes)
                .merge(root_routes),
        )
        .fallback(crate::embed::static_handler)
        .with_state(state)
        .layer(middleware::from_fn(
            crate::middleware::rate_limit::rate_limit_middleware,
        ))
}
