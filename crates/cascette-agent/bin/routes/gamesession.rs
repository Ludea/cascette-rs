//! GET/POST /gamesession, /gamesession/{product} -- Game session tracking.
//!
//! Tracks running game processes. The agent validates the caller's OS PID
//! against the `pid` field in the JSON body by checking the supplied PID
//! against live game process PIDs via process_detection — rejecting PIDs that
//! are not known game processes for the given product.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    gamesession::{self, SessionRequest},
};

/// GET /gamesession -- list all active sessions.
pub async fn get_sessions(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(gamesession::sessions(&state).await)
}

/// GET /gamesession/{product} -- get session for specific product.
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
) -> impl IntoResponse {
    Json(gamesession::session(&state, product).await)
}

/// POST /gamesession/{product} -- start or update a game session.
///
/// If a `pid` is supplied in the request body, it is validated against live
/// game process PIDs for the product (via `process_detection::game_pids`).
/// A PID that does not belong to a known game process for this product is
/// rejected with HTTP 400.
pub async fn post_session(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<SessionRequest>,
) -> impl IntoResponse {
    Json(gamesession::set_session(&state, body, product).await)
}
