//! GET /install/{product} (polling) -- Operation progress.
//
use std::sync::Arc;

use crate::handlers::AppState;
use serde_json::{Value, json};

pub async fn progress(state: &Arc<AppState>, product: String) -> Result<Value, String> {
    let response_uri = format!("/install/{product}");
    let result_uri = format!("/install/{product}");

    match state.queue.find_active_for_product(&product).await {
        Ok(Some(op)) => Ok(json!({
            "uid": product,
            "operation_id": op.operation_id.to_string(),
            "operation_type": op.operation_type.to_string(),
            "state": op.state.to_string(),
            "progress": op.progress,
            "error": op.error,
            "response_uri": response_uri,
            "result_uri": result_uri,
        })),
        Ok(None) => Ok(json!({
            "uid": product,
            "state": "idle",
            "response_uri": response_uri,
            "result_uri": result_uri,
        })),
        Err(e) => Err(e.to_string()),
    }
}
