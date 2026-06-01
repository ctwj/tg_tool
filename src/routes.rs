use crate::handlers;
use crate::state::AppState;
use axum::{
    Router,
    routing::{delete, get, post, put},
};

pub fn build_router(state: AppState) -> Router {
    // All API routes — auth middleware will be added in Phase 3
    let api_routes = Router::new()
        // Auth (public)
        .route("/auth/register", post(handlers::auth::register))
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/logout", post(handlers::auth::logout))
        .route(
            "/auth/me",
            get(handlers::auth::get_me).put(handlers::auth::update_me),
        )
        .route("/auth/token", post(handlers::auth::generate_api_token))
        // Status (public)
        .route("/status", get(handlers::misc::system_status))
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
        .route("/push/retry", post(handlers::push::retry_push))
        .route("/push/scheduler", put(handlers::push::update_scheduler))
        // Users
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
        .route(
            "/files/download/{filename}",
            get(handlers::file::download_file),
        )
        // Options
        .route(
            "/options",
            get(handlers::option::get_options).put(handlers::option::update_options),
        );

    Router::new()
        .nest("/api", api_routes)
        .fallback(crate::embed::static_handler)
        .with_state(state)
}
