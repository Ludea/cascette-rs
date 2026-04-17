//! GET /agent/download -- Per-product download state.
//!
//! Returns download configuration scoped to a product. The real agent tracks
//! per-product download priority and background download status. This endpoint
//! is separate from POST /download which sets global speed limits.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};

use cascette_agent::handlers::{
    AppState,
    agent_download::{self, AgentDownloadQuery},
};

/// GET /agent/download -- read download state for a product.
///
/// Returns per-product download configuration from the in-memory cache.
/// Falls back to defaults if no configuration has been set for the product.
pub async fn get_agent_download(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AgentDownloadQuery>,
) -> Json<serde_json::Value> {
    Json(agent_download::agent_download(&state, query).await)
}
