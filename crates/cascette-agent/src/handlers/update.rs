//! POST /update/{product} and POST /update -- Start product update.
//!
//! Registers both `/update/{product}` and bare `/update/`.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::models::operation::{Operation, OperationType, Priority};
use crate::models::product::ProductStatus;

/// Update request body.
#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Custom target build config hash for updating to a specific version.
    pub build_config: Option<String>,
    /// Custom target CDN config hash for updating to a specific version.
    pub cdn_config: Option<String>,
}

fn default_priority() -> u32 {
    700
}

/// Shared update logic used by both `/update/{product}` and bare `/update`.
async fn update(
    state: &Arc<AppState>,
    product_code: &str,
    body: &UpdateRequest,
) -> Result<Value, u32> {
    match state.registry.get(product_code).await {
        Ok(p) if p.status != ProductStatus::Installed => Err(AGENT_ERROR_INVALID_REQUEST),
        Err(_) => Err(AGENT_ERROR_INVALID_REQUEST),
        Ok(_) => Ok(json!({})),
    };

    let priority = Priority::from_agent_priority(body.priority);
    let params = if body.build_config.is_some() || body.cdn_config.is_some() {
        Some(json!({
            "build_config": body.build_config,
            "cdn_config": body.cdn_config,
        }))
    } else {
        None
    };
    let operation = Operation::new(
        product_code.to_string(),
        OperationType::Update,
        priority,
        params,
    );

    match state.queue.insert(&operation).await {
        Ok(()) => {
            state.queue_notify.notify_one();
            let response_uri = format!("/update/{product_code}");
            let result_uri = format!("/update/{product_code}");
            Ok(json!({
                "response_uri": response_uri,
                "result_uri": result_uri,
                "uid": product_code,
                "priority": body.priority,
            }))
        }
        Err(_) => Err(AGENT_ERROR_INVALID_REQUEST),
    }
}

/// POST /update/{product}
pub async fn set_update(
    state: &Arc<AppState>,
    product: String,
    body: UpdateRequest,
) -> Result<Value, u32> {
    let product_code = body.uid.as_deref().unwrap_or(&product);
    update(state, product_code, &body).await
}

/// POST /update (bare endpoint, product resolved from body `uid`).
pub async fn set_update_bare(state: &Arc<AppState>, body: UpdateRequest) -> Result<Value, u32> {
    let product_code = match body.uid.as_deref() {
        Some(uid) if !uid.is_empty() => uid.to_string(),
        _ => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
    };
    update(state, &product_code, &body).await
}
