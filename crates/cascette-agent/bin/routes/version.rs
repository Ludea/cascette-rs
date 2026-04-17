//! GET /version -- Version information.
//!
//! Agent.exe accepts an optional `uid` query parameter and returns a single
//! `version` string: `"TACT x.y.z (CASC a.b.c)"`. The uid is ignored; the
//! response is the same regardless of its value.
//!
//! cascette-agent returns the wire-compatible `version` field plus additional
//! informational fields (`agent_version`, `product_version`, `build_date`).
//! These extensions are not present in Agent.exe but do not break callers that
//! only read `version`.

use axum::Json;

use cascette_agent::handlers::version;

/// GET /version
pub async fn get_version() -> Json<serde_json::Value> {
    Json(version::version().await)
}
