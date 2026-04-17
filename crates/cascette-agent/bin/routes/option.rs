//! GET/POST /option -- Product user options (language, region preferences).
//!
//! Options are stored in-memory per-product. The agent reads the default
//! locale from its configuration.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    option::{self, OptionQuery, OptionRequest},
};

/// GET /option -- get current options.
///
/// When `uid` is provided as a query parameter, returns the product-specific
/// locale and region. Otherwise returns the global default locale.
pub async fn get_option(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OptionQuery>,
) -> Json<serde_json::Value> {
    Json(option::option(&state, query).await)
}

/// POST /option -- update options for a product.
///
/// When a product uid is provided, updates that product's locale/region
/// in the registry. Returns error 2312 if uid is given but product is not found
/// (matches Agent.exe behavior for unknown UIDs).
pub async fn post_option(
    State(state): State<Arc<AppState>>,
    Json(body): Json<OptionRequest>,
) -> impl IntoResponse {
    match option::set_option(&state, body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
