//! POST /backfill/{product} and POST /backfill -- Start background content fill.
//!
//! Registers both `/backfill/{product}` and bare `/backfill/`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    backfill::{self, BackfillRequest},
};

/// POST /backfill/{product}
pub async fn post_backfill(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<BackfillRequest>,
) -> impl IntoResponse {
    match backfill::set_backfill(&state, &body, product).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /backfill (bare endpoint, product resolved from body `uid`).
pub async fn post_backfill_bare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BackfillRequest>,
) -> impl IntoResponse {
    match backfill::set_backfill_bare(&state, &body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
