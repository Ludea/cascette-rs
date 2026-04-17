//! GET /content/{hash} -- Content delivery by encoding key.
//!
//! Serves raw file content from a registered installation, looked up by
//! encoding key hash. The hash is a 32-character hex string representing
//! the 16-byte encoding key used internally by CASC.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use cascette_agent::handlers::{AppState, content};

pub async fn get_content(State(state): State<Arc<AppState>>, Path(hash): Path<String>) -> Response {
    match content::content(&state, hash).await {
        Ok(vec) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .header("content-length", vec.len())
            .body(Body::from(vec))
            .unwrap_or_else(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response()
            }),
        Err(err) => (StatusCode::NOT_FOUND, err).into_response(),
    }
}
