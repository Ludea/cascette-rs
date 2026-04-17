//! POST /uninstall/{product} and POST /uninstall -- Start product uninstallation.
//!
//! Registers both `/uninstall/{product}` and bare `/uninstall/`.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::models::operation::{Operation, OperationType, Priority};

/// Uninstall request body.
#[derive(Debug, Deserialize)]
pub struct UninstallRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    700
}

/// Shared uninstall logic used by both `/uninstall/{product}` and bare `/uninstall`.
async fn uninstall(
    state: &Arc<AppState>,
    product_code: &str,
    body: &UninstallRequest,
) -> Result<Value, u32> {
    match state.registry.get(product_code).await {
        Ok(p) if !p.status.is_installed() => Err(AGENT_ERROR_INVALID_REQUEST),
        Err(_) => Err(AGENT_ERROR_INVALID_REQUEST),
        Ok(_) => Ok({}),
    }?;

    let priority = Priority::from_agent_priority(body.priority);
    let operation = Operation::new(
        product_code.to_string(),
        OperationType::Uninstall,
        priority,
        None,
    );

    match state.queue.insert(&operation).await {
        Ok(()) => {
            state.queue_notify.notify_one();
            let response_uri = format!("/uninstall/{product_code}");
            let result_uri = format!("/uninstall/{product_code}");
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

/// POST /uninstall/{product}
pub async fn set_uninstall(
    state: &Arc<AppState>,
    product: String,
    body: UninstallRequest,
) -> Result<Value, u32> {
    let product_code = body.uid.as_deref().unwrap_or(&product);

    uninstall(state, product_code, &body).await
}
/// POST /uninstall (bare endpoint, product resolved from body `uid`).
pub async fn set_uninstall_bare(
    state: &Arc<AppState>,
    body: UninstallRequest,
) -> Result<Value, u32> {
    let product_code = match body.uid.as_deref() {
        Some(uid) if !uid.is_empty() => uid.to_string(),
        _ => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
    };
    uninstall(state, &product_code, &body).await
}
