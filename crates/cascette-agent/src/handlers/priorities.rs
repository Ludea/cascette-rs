//! GET/POST /priorities -- Product priority list and per-product priority setting.
//!
//! GET returns the priority ordering of all registered products.
//! POST sets the download priority for a specific product.

use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;
use crate::handlers::{AppState, ProductDownloadConfig};

/// Per-product priority request (POST /priorities).
#[derive(Debug, Deserialize)]
pub struct PriorityRequest {
    /// Product identifier. Required.
    pub uid: String,
    /// Download priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

/// Default download priority.
const DEFAULT_PRIORITY: u32 = 700;

fn default_priority() -> u32 {
    DEFAULT_PRIORITY
}

/// GET /priorities -- list registered products with their download priorities.
///
/// Reads stored per-product priorities from the in-memory cache,
/// falling back to the default (700) for products without explicit config.
pub async fn priorities(state: &Arc<AppState>) -> Value {
    let configs = state.product_download_config.read().await;

    let priorities = match state.registry.list().await {
        Ok(products) => products
            .iter()
            .map(|p| {
                let priority = configs
                    .get(&p.product_code)
                    .map_or(DEFAULT_PRIORITY, |c| c.priority);
                json!({
                    "uid": p.product_code,
                    "priority": priority,
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };

    json!({
        "priorities": priorities,
    })
}

/// POST /priorities -- set download priority for a product.
pub async fn set_priorities(state: &Arc<AppState>, body: PriorityRequest) -> Result<Value, u32> {
    // Validate UID.
    if body.uid.is_empty() {
        return Err(AGENT_ERROR_INVALID_REQUEST);
    }

    // Product must exist in registry.
    if state.registry.get(&body.uid).await.is_err() {
        return Err(AGENT_ERROR_INVALID_REQUEST);
    }

    // Store priority per-product.
    {
        let config_snapshot = {
            let mut configs = state.product_download_config.write().await;
            let config = configs
                .entry(body.uid.clone())
                .or_insert_with(ProductDownloadConfig::default);
            config.priority = body.priority;
            let snapshot = config.clone();
            drop(configs);
            snapshot
        };

        // Persist to database (guard already dropped above).
        if let Err(e) = state
            .registry
            .set_download_config(&body.uid, &config_snapshot)
            .await
        {
            tracing::warn!(uid = %body.uid, error = %e, "failed to persist priority");
        }
    }

    // Build response.
    let response_uri = format!("/priorities/{}", body.uid);
    let result_uri = format!("/priorities/{}", body.uid);

    Ok(json!({
        "response_uri": response_uri,
        "result_uri": result_uri,
    }))
}
