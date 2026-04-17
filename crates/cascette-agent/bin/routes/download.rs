//! GET/POST /download -- Download speed configuration.
//!
//! GET returns current global download state.
//! POST configures per-product download parameters.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    download::{self, DownloadConfigRequest},
};

/// GET /download -- current download state.
pub async fn get_download(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(download::download(&state).await)
}

/// POST /download -- configure per-product download parameters.
pub async fn post_download(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DownloadConfigRequest>,
) -> impl IntoResponse {
    match download::set_download(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
