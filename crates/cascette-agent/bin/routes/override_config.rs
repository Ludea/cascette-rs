//! GET/POST /agent/override -- CDN, version service, and config server overrides.
//! GET/POST /agent/{product} -- per-product override state.
//!
//! The override state is stored in-memory and reset when the agent process exits.
//! Battle.net client uses these overrides to redirect CDN or version service traffic
//! for debugging or staging purposes.
//!
//! ## GET /agent/override
//!
//! Returns the current override configuration with four fields:
//! - `patch_url_override`: array of per-product CDN URL overrides
//! - `version_service_overrides`: global version service override string
//! - `version_server_url_override`: direct version server URL override
//! - `config_overrides`: object of arbitrary key/value config pairs
//!
//! ## POST /agent/override
//!
//! Applies new overrides. Accepts the same four fields plus
//! `version_server_override` (a named server override). Returns a
//! `failed_overrides` array for any entries that could not be applied.
//!
//! ## GET /agent/{product}
//!
//! Returns per-product session state combining install status, session context,
//! and any active per-product overrides. Fields:
//! - `pid`: active game session PID, or 0
//! - `user_id`, `user_name`, `session`: session context strings (empty when no
//!   session is active)
//! - `state`: integer install state (1003 = installing, 1004 = installed, 1007 = not installed)
//! - `version`: installed version string, or empty
//! - `region`: configured region, or empty
//! - `type`: product type string ("agent")
//! - `opt_in_feedback`: always true (matches Agent.exe default)
//! - `patch_url_override`: per-product CDN URL overrides (array)
//! - `version_service_overrides`: version service overrides (array)
//! - `version_server_url_override`: direct version server URL override
//!
//! ## POST /agent/{product}
//!
//! Applies per-product overrides. Accepted fields:
//! - `patch_url_override`: array of key/value CDN URL override entries
//! - `version_server_url_override`: direct version server URL
//! - `version_server_override`: named version server
//! - `account_country`, `geo_ip_country`: country overrides
//! - `region`: region redirect (triggers CDN re-routing in Agent.exe)

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{
    AppState,
    override_config::{self, SetOverrideRequest, SetProductOverrideRequest},
};

/// GET /agent/override -- return current override configuration.
///
/// Returns the four user-visible override fields. `version_server_override`
/// is not returned (internal field, not surfaced by GET).
pub async fn get_override_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(override_config::override_config(&state).await)
}

/// POST /agent/override -- apply new override configuration.
///
/// Merges the supplied fields into the current override state.
/// Returns `failed_overrides` — an empty array when all overrides are accepted.
pub async fn post_override_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetOverrideRequest>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(override_config::set_override_config(&state, body).await),
    )
}

/// GET /agent/{product} -- per-product session and override state.
///
/// Returns a snapshot of the product's install state, active game session,
/// and any per-product overrides stored in the global override config.
pub async fn get_product_override_state(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(override_config::product_override_state(&state, product).await),
    )
}

/// POST /agent/{product} -- apply per-product override state.
///
/// Merges the supplied per-product overrides into the global override config.
/// Returns HTTP 200 on success.
pub async fn post_product_override_state(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<SetProductOverrideRequest>,
) -> impl IntoResponse {
    match override_config::set_product_override_state(&state, body, product).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({}))),
        Err(_) => (StatusCode::OK, Json(serde_json::json!({}))),
    }
}
