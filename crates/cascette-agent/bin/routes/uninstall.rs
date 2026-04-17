//! POST /uninstall/{product} and POST /uninstall -- Start product uninstallation.
//!
//! Registers both `/uninstall/{product}` and bare `/uninstall/`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    uninstall::{self, UninstallRequest},
};

/// POST /uninstall/{product}
pub async fn post_uninstall(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<UninstallRequest>,
) -> impl IntoResponse {
    match uninstall::set_uninstall(&state, product, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /uninstall (bare endpoint, product resolved from body `uid`).
pub async fn post_uninstall_bare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UninstallRequest>,
) -> impl IntoResponse {
    match uninstall::set_uninstall_bare(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
