use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::models::operation::{Operation, OperationType, Priority};
use crate::models::product::ProductStatus;

/// Repair request body.
#[derive(Debug, Deserialize)]
pub struct RepairRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    700
}

/// Shared repair logic used by both `/repair/{product}` and bare `/repair`.
async fn repair(
    state: &Arc<AppState>,
    product_code: &str,
    body: &RepairRequest,
) -> Result<Value, u32> {
    // Agent.exe rejects "unsupported" products — those not in the registry or
    // not in an installed state. We mirror this by allowing repair from any
    // installed state (Installed, Corrupted, Updating, Verifying) and
    // rejecting products that are busy with an incompatible operation
    // (Installing, Uninstalling, Repairing) or not yet installed (Available).
    match state.registry.get(product_code).await {
        Ok(p) if !p.status.is_installed() || p.status == ProductStatus::Repairing => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
        Err(_) => Err(AGENT_ERROR_INVALID_REQUEST),
        Ok(_) => Ok({}),
    }?;

    let priority = Priority::from_agent_priority(body.priority);
    let operation = Operation::new(
        product_code.to_string(),
        OperationType::Repair,
        priority,
        None,
    );

    match state.queue.insert(&operation).await {
        Ok(()) => {
            state.queue_notify.notify_one();
            let response_uri = format!("/repair/{product_code}");
            let result_uri = format!("/repair/{product_code}");
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

pub async fn set_repair(
    state: &Arc<AppState>,
    body: &RepairRequest,
    product: String,
) -> Result<Value, u32> {
    let product_code = body.uid.as_deref().unwrap_or(&product);
    repair(state, product_code, &body).await
}

pub async fn set_repair_bare(state: &Arc<AppState>, body: RepairRequest) -> Result<Value, u32> {
    let product_code = match body.uid.as_deref() {
        Some(uid) if !uid.is_empty() => uid.to_string(),
        _ => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
    };
    repair(state, &product_code, &body).await
}
