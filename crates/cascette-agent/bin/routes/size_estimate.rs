//! POST /size_estimate + GET /size_estimate/{uid} -- async size estimation.
//!
//! Matches the Agent.exe `HandleSizeEstimate` wire format: POST starts a
//! background estimation task, GET polls for the result.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    size_estimate::{self, SizeEstimateRequest},
};

/// POST /size_estimate -- start a background size estimation.
pub async fn post_size_estimate(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SizeEstimateRequest>,
) -> impl IntoResponse {
    match size_estimate::set_size_estimate(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err })),
        ),
    }
}

/// GET /size_estimate/{uid} -- poll for estimation result.
pub async fn get_size_estimate_result(
    State(state): State<Arc<AppState>>,
    Path(uid): Path<String>,
) -> impl IntoResponse {
    Json(size_estimate::size_estimate_result(&state, uid).await)
}
