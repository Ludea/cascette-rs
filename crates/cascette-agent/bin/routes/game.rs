//! GET /game, GET /game/{product}, POST /game -- Product listing, details, and launch config.
//!
//! GET returns product lists and details.
//! POST configures game launch parameters (binary_type, run64bit, launch_arguments).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    game::{self, GameConfigRequest, GameEntry},
};

/// GET /game -- list all products.
pub async fn list_games(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<GameEntry>>, (StatusCode, Json<serde_json::Value>)> {
    match game::list_games(&state).await {
        Ok(vec) => Ok(Json(vec)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": err})),
        )),
    }
}

pub async fn get_game(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
) -> impl IntoResponse {
    match game::game(&state, product).await {
        Ok(vec) => (StatusCode::OK, Json(vec)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}

/// POST /game -- configure game launch parameters.
///
/// Validates the UID, resolves the binary path, and writes `Launcher.db`.
/// Returns `{"response_uri": "...", "launch_path": "..."}` on success or
/// `{"error": 2312}` on failure.
pub async fn post_game_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<GameConfigRequest>,
) -> impl IntoResponse {
    match game::set_game_config(&state, body).await {
        Ok(vec) => (StatusCode::OK, Json(vec)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
