//! GET/POST /spawned -- child process spawn tracking.
//! GET/POST /spawned/{product} -- per-product spawn status.
//!
//! Registers `/spawned/{product}` as a dynamic sub-route. Allocates a
//! spawner context, associates the request with a running child process, and
//! returns a `response_uri` pointing to the per-product spawn sub-endpoint.
//!
//! In cascette-agent, game launch lifecycle is tracked via `POST /gamesession`.
//! These endpoints exist for wire compatibility and return valid responses
//! without side effects.

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::spawned::{self, SpawnedRequest};

/// GET /spawned -- global spawn status list.
pub async fn get_spawned() -> impl IntoResponse {
    (StatusCode::OK, Json(spawned::spawned().await))
}

/// GET /spawned/{product} -- per-product spawn status.
pub async fn get_spawned_product(Path(product): Path<String>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(spawned::spawned_product(product).await),
    )
}

/// POST /spawned -- record a global spawn event.
///
/// Returns `response_uri`.
pub async fn post_spawned(Json(body): Json<SpawnedRequest>) -> impl IntoResponse {
    (StatusCode::OK, Json(spawned::set_spawned(body).await))
}

/// POST /spawned/{product} -- record a per-product spawn event.
///
/// Returns `response_uri` for the spawned product sub-endpoint.
pub async fn post_spawned_product(Path(product): Path<String>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(spawned::set_spawned_product(product).await),
    )
}
