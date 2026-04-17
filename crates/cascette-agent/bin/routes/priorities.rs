//! GET/POST /priorities -- Product priority list and per-product priority setting.
//!
//! GET returns the priority ordering of all registered products.
//! POST sets the download priority for a specific product.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    priorities::{self, PriorityRequest},
};

/// GET /priorities -- list registered products with their download priorities.
///
/// Reads stored per-product priorities from the in-memory cache,
/// falling back to the default (700) for products without explicit config.
pub async fn get_priorities(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(priorities::priorities(&state).await)
}

/// POST /priorities -- set download priority for a product.
pub async fn post_priorities(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PriorityRequest>,
) -> impl IntoResponse {
    match priorities::set_priorities(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
