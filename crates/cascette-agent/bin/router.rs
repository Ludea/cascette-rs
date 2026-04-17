//! Axum router configuration matching real Blizzard Agent.exe endpoints.
//!
//! The real agent exposes 22 static endpoints on port 1120. Dynamic per-product
//! endpoints are registered for install, update, repair, uninstall, fill,
//! game, and gamesession operations.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::routes;
use cascette_agent::handlers::AppState;

/// Create the full Axum router with all agent endpoints.
pub fn create_router(state: Arc<AppState>) -> Router {
    let timeout = Duration::from_secs(30);

    // CORS restricted to localhost (agent is a local service)
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(|origin, _req| {
            origin
                .to_str()
                .map(|s| s.contains("localhost") || s.contains("127.0.0.1"))
                .unwrap_or(false)
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    Router::new()
        // Real agent endpoints
        .route(
            "/agent",
            axum::routing::get(routes::agent::get_agent_info)
                .post(routes::agent::post_agent_config),
        )
        .route(
            "/game",
            axum::routing::get(routes::game::list_games).post(routes::game::post_game_config),
        )
        .route(
            "/game/{product}",
            axum::routing::get(routes::game::get_game),
        )
        .route(
            "/install",
            axum::routing::post(routes::install::post_install_bare),
        )
        .route(
            "/install/{product}",
            axum::routing::post(routes::install::post_install).get(routes::progress::get_progress),
        )
        .route(
            "/update",
            axum::routing::post(routes::update::post_update_bare),
        )
        .route(
            "/update/{product}",
            axum::routing::post(routes::update::post_update).get(routes::progress::get_progress),
        )
        .route(
            "/repair",
            axum::routing::post(routes::repair::post_repair_bare),
        )
        .route(
            "/repair/{product}",
            axum::routing::post(routes::repair::post_repair).get(routes::progress::get_progress),
        )
        .route(
            "/uninstall",
            axum::routing::post(routes::uninstall::post_uninstall_bare),
        )
        .route(
            "/uninstall/{product}",
            axum::routing::post(routes::uninstall::post_uninstall)
                .get(routes::progress::get_progress),
        )
        .route(
            "/backfill",
            axum::routing::post(routes::backfill::post_backfill_bare),
        )
        .route(
            "/backfill/{product}",
            axum::routing::post(routes::backfill::post_backfill)
                .get(routes::progress::get_progress),
        )
        .route("/version", axum::routing::get(routes::version::get_version))
        .route(
            "/hardware",
            axum::routing::get(routes::hardware::get_hardware),
        )
        .route(
            "/gamesession",
            axum::routing::get(routes::gamesession::get_sessions),
        )
        .route(
            "/gamesession/{product}",
            axum::routing::get(routes::gamesession::get_session)
                .post(routes::gamesession::post_session),
        )
        .route(
            "/download",
            axum::routing::get(routes::download::get_download)
                .post(routes::download::post_download),
        )
        .route(
            "/option",
            axum::routing::get(routes::option::get_option).post(routes::option::post_option),
        )
        .route(
            "/size_estimate",
            axum::routing::post(routes::size_estimate::post_size_estimate),
        )
        .route(
            "/size_estimate/{uid}",
            axum::routing::get(routes::size_estimate::get_size_estimate_result),
        )
        .route(
            "/agent/download",
            axum::routing::get(routes::agent_download::get_agent_download),
        )
        .route(
            "/agent/override",
            axum::routing::get(routes::override_config::get_override_config)
                .post(routes::override_config::post_override_config),
        )
        .route(
            "/agent/{product}",
            axum::routing::get(routes::override_config::get_product_override_state)
                .post(routes::override_config::post_product_override_state),
        )
        .route(
            "/spawned",
            axum::routing::get(routes::spawned::get_spawned).post(routes::spawned::post_spawned),
        )
        .route(
            "/spawned/{product}",
            axum::routing::get(routes::spawned::get_spawned_product)
                .post(routes::spawned::post_spawned_product),
        )
        .route(
            "/gce_state",
            axum::routing::get(routes::admin::get_gce_state).post(routes::admin::post_gce_state),
        )
        .route(
            "/createshortcut",
            axum::routing::post(routes::admin::post_createshortcut),
        )
        .route(
            "/admin_command",
            axum::routing::post(routes::admin::post_admin_command),
        )
        .route("/admin", axum::routing::post(routes::admin::post_admin))
        .route(
            "/register",
            axum::routing::post(routes::register::post_register),
        )
        .route(
            "/priorities",
            axum::routing::get(routes::priorities::get_priorities)
                .post(routes::priorities::post_priorities),
        )
        .route(
            "/content/{hash}",
            axum::routing::get(routes::content::get_content),
        )
        // Cascette extensions
        .route("/health", axum::routing::get(routes::health::get_health))
        .route("/metrics", axum::routing::get(routes::metrics::get_metrics))
        .route(
            "/extract/{product}",
            axum::routing::post(routes::extract::post_extract).get(routes::progress::get_progress),
        )
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            timeout,
        ))
        .with_state(state)
}
