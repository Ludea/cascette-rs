//! POST /repair/{product} and POST /repair -- Start product repair.
//!
//! Registers both `/repair/{product}` and bare `/repair/`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    repair::{self, RepairRequest},
};

/// POST /repair/{product}
pub async fn post_repair(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<RepairRequest>,
) -> impl IntoResponse {
    match repair::set_repair(&state, &body, product).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /repair (bare endpoint, product resolved from body `uid`).
pub async fn post_repair_bare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RepairRequest>,
) -> impl IntoResponse {
    match repair::set_repair_bare(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
