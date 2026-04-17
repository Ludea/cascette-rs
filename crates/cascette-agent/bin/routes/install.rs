//! POST /install/{product} and POST /install -- Start product installation.
//!
//! The original Agent.exe requires the product to be registered via `/register`
//! first. It does not auto-create products. The bare `/install` endpoint
//! dispatches via `uid` in the JSON body.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    install::{self, InstallRequest},
};

/// POST /install/{product}
pub async fn post_install(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<InstallRequest>,
) -> impl IntoResponse {
    match install::set_install(&state, body, product).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /install (bare endpoint, product resolved from body `uid`).
pub async fn post_install_bare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InstallRequest>,
) -> impl IntoResponse {
    match install::set_install_bare(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
