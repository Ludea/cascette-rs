//! POST /extract/{product} -- Extract CASC content to a directory.
//!
//! This is a cascette extension; it is not present in Blizzard Agent.exe.
//!
//! The product must be installed before extraction. The executor reads
//! stored build/cdn config hashes from the product registry to fetch the
//! install manifest from CDN without a Ribbit query.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use cascette_agent::handlers::{AppState, ExtractRequest, extract};

/// POST /extract/{product}
pub async fn post_extract(
    State(state): State<Arc<AppState>>,
    Path(product): Path<String>,
    Json(body): Json<ExtractRequest>,
) -> impl IntoResponse {
    let product_code = body.uid.as_deref().unwrap_or(&product);
    match extract::extract(&state, product_code, &body).await {
        Ok(json) => (StatusCode::OK, Json(json)),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": err})),
        ),
    }
}
