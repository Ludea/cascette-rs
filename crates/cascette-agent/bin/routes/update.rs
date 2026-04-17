//! POST /update/{product} and POST /update -- Start product update.
//!
//! Registers both `/update/{product}` and bare `/update/`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    update::{self, UpdateRequest},
};

/// POST /update/{product}
pub async fn post_update(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<UpdateRequest>,
) -> impl IntoResponse {
    match update::set_update(&state, product, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /update (bare endpoint, product resolved from body `uid`).
pub async fn post_update_bare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpdateRequest>,
) -> impl IntoResponse {
    match update::set_update_bare(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
