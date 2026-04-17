use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::models::operation::{Operation, OperationType, Priority};
use crate::models::product::ProductStatus;

/// Backfill request body.
#[derive(Debug, Deserialize)]
pub struct BackfillRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Priority (default 700, matching Agent.exe 0x2bc).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    700
}

/// Shared backfill logic used by both `/backfill/{product}` and bare `/backfill`.
async fn backfill(
    state: &Arc<AppState>,
    product_code: &str,
    body: &BackfillRequest,
) -> Result<Value, u32> {
    match state.registry.get(product_code).await {
        Ok(p) if p.status != ProductStatus::Installed => Err(AGENT_ERROR_INVALID_REQUEST),
        Err(_) => Err(AGENT_ERROR_INVALID_REQUEST),
        Ok(_) => Ok({}),
    }?;

    let priority = Priority::from_agent_priority(body.priority);
    let operation = Operation::new(
        product_code.to_string(),
        OperationType::Backfill,
        priority,
        None,
    );

    match state.queue.insert(&operation).await {
        Ok(()) => {
            state.queue_notify.notify_one();
            let response_uri = format!("/backfill/{product_code}");
            let result_uri = format!("/backfill/{product_code}");
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

pub async fn set_backfill(
    state: &Arc<AppState>,
    body: &BackfillRequest,
    product: String,
) -> Result<Value, u32> {
    let product_code = body.uid.as_deref().unwrap_or(&product);
    backfill(state, product_code, &body).await
}

pub async fn set_backfill_bare(
    state: &Arc<AppState>,
    body: &BackfillRequest,
) -> Result<Value, u32> {
    let product_code = match body.uid.as_deref() {
        Some(uid) if !uid.is_empty() => uid.to_string(),
        _ => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
    };
    backfill(state, &product_code, &body).await
}
