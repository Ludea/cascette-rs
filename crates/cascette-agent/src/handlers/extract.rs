//! POST /extract/{product} -- Extract CASC content to a directory.
//!
//! This is a cascette extension; it is not present in Blizzard Agent.exe.
//!
//! The product must be installed before extraction. The executor reads
//! stored build/cdn config hashes from the product registry to fetch the
//! install manifest from CDN without a Ribbit query.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::models::operation::{Operation, OperationType, Priority};
use crate::models::product::ProductStatus;

/// Extract request body.
#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    /// Product unique identifier (overrides path parameter when set).
    pub uid: Option<String>,
    /// Target directory for extracted files.
    pub output_path: String,
    /// Optional glob-style file filter (e.g. "Interface/*").
    pub pattern: Option<String>,
    /// Priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    700
}

/// Shared extract handler.
pub async fn extract(
    state: &Arc<AppState>,
    product_code: &str,
    body: &ExtractRequest,
) -> Result<Value, u32> {
    // Product must be installed
    match state.registry.get(product_code).await {
        Ok(p) if p.status != ProductStatus::Installed => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
        Err(_) => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
        Ok(_) => {
            let empty = json!({});
            Ok::<Value, u32>(empty)
        }
    }?;

    // Retrieve stored install_path from registry
    let install_path = match state.registry.get(product_code).await {
        Ok(p) => match p.install_path {
            Some(path) => path,
            None => {
                return Err(AGENT_ERROR_INVALID_REQUEST);
            }
        },
        Err(_) => {
            return Err(AGENT_ERROR_INVALID_REQUEST);
        }
    };

    let mut params = json!({
        "install_path": install_path,
        "output_path": body.output_path,
    });

    if let Some(ref pat) = body.pattern {
        params["pattern"] = serde_json::Value::String(pat.clone());
    }

    let priority = Priority::from_agent_priority(body.priority);
    let operation = Operation::new(
        product_code.to_string(),
        OperationType::Extract,
        priority,
        Some(params),
    );

    match state.queue.insert(&operation).await {
        Ok(()) => {
            state.queue_notify.notify_one();
            let response_uri = format!("/extract/{product_code}");
            let result_uri = format!("/extract/{product_code}");
            Ok(json!({
                "response_uri": response_uri,
                "result_uri": result_uri,
                "uid": product_code,
                "priority": body.priority,
            }))
        }
        Err(_) => return Err(AGENT_ERROR_INVALID_REQUEST),
    }
}
