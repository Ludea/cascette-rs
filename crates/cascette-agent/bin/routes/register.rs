//! POST /register -- Product registration.
//!
//! The launcher calls this endpoint to register a product with the agent,
//! providing its install directory, patch URL, and protocol. The agent creates
//! a product entry, detects install-path conflicts with existing products,
//! and chains to the install logic.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    register::{self, RegisterRequest},
};

/// POST /register -- register a product with the agent.
pub async fn post_register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> impl IntoResponse {
    match register::register(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
