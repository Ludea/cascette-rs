//! GET /install/{product} (polling) -- Operation progress.
//!
//! When polled via GET, the install endpoint returns the current progress
//! of any active operation on the product. The response includes `response_uri`
//! and `result_uri` fields matching Agent.exe polling behavior.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{AppState, progress};

/// GET /install/{product} -- poll for installation progress.
pub async fn get_progress(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
) -> impl IntoResponse {
    match progress::progress(&state, product).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": err.to_string()})),
        ),
    }
}
