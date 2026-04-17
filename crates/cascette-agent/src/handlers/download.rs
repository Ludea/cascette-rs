//! GET/POST /download -- Download speed configuration.
//!
//! GET returns current global download state.
//! POST configures per-product download parameters.

use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::handlers::error_codes::AGENT_ERROR_INVALID_REQUEST;

use crate::handlers::{AppState, ProductDownloadConfig};

/// Global download speed/pause state shared across handlers and executors.
#[derive(Default)]
pub struct DownloadState {
    /// Maximum download speed in bytes per second (0 = unlimited).
    pub max_speed_bps: u64,
    /// Whether downloads are paused.
    pub paused: bool,
    /// Current aggregate download speed in bytes per second.
    pub current_speed_bps: u64,
}

/// Download configuration request (POST /download).
#[derive(Debug, Deserialize)]
pub struct DownloadConfigRequest {
    /// Product identifier. Required.
    pub uid: String,
    /// Speed limit in bytes per second (0 = unlimited).
    #[serde(default)]
    pub download_limit: u64,
    /// Armadillo DRM decryption buffer.
    #[serde(default)]
    pub decryption_buffer: String,
    /// Download priority (default 700).
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    700
}

/// GET /download -- current download state.
pub async fn download(state: &Arc<AppState>) -> Value {
    let ds = state.download_state.read().await;
    json!({
        "max_speed_bps": ds.max_speed_bps,
        "paused": ds.paused,
        "current_speed_bps": ds.current_speed_bps,
    })
}

/// POST /download -- configure per-product download parameters.
pub async fn set_download(
    state: &Arc<AppState>,
    body: DownloadConfigRequest,
) -> Result<Value, u32> {
    // Validate UID.
    if body.uid.is_empty() {
        return Err(AGENT_ERROR_INVALID_REQUEST);
    }

    // Product must exist in registry.
    if state.registry.get(&body.uid).await.is_err() {
        return Err(AGENT_ERROR_INVALID_REQUEST);
    }

    // Store per-product download config.
    {
        let config_snapshot = {
            let mut configs = state.product_download_config.write().await;
            let config = configs
                .entry(body.uid.clone())
                .or_insert_with(ProductDownloadConfig::default);
            config.download_limit = body.download_limit;
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
            tracing::warn!(uid = %body.uid, error = %e, "failed to persist download config");
        }
    }

    // Build response.
    let response_uri = format!("/download/{}", body.uid);
    let result_uri = format!("/download/{}", body.uid);

    Ok(json!({
        "response_uri": response_uri,
        "result_uri": result_uri,
    }))
}
