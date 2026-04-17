//! GET /health -- Health check endpoint (cascette extension).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use cascette_agent::handlers::{AppState, health};

/// GET /health
pub async fn get_health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(health::health(&state).await)
}
