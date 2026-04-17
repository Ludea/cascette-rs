//! GET/POST /agent -- Agent info and per-product download configuration.
//!
//! GET returns version info and configuration state.
//! POST configures per-product download state (background_download, priority,
//! download_limit, paused).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    agent::{self, AgentConfigRequest, AgentInfoResponse},
};

/// GET /agent
pub async fn get_agent_info(State(state): State<Arc<AppState>>) -> Json<AgentInfoResponse> {
    Json(agent::agent_info(&state).await)
}

/// POST /agent -- configure per-product download state.
pub async fn post_agent_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AgentConfigRequest>,
) -> impl IntoResponse {
    match agent::set_agent_config(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
