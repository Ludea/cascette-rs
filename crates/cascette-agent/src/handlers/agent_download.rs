use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::handlers::{AppState, ProductDownloadConfig};

/// Query parameters for GET /agent/download.
#[derive(Debug, Deserialize)]
pub struct AgentDownloadQuery {
    /// Product identifier for per-product download state.
    pub uid: Option<String>,
}

pub async fn agent_download(state: &Arc<AppState>, query: AgentDownloadQuery) -> Value {
    let uid = query.uid.as_deref().unwrap_or("");

    let configs = state.product_download_config.read().await;
    let default_config = ProductDownloadConfig::default();
    let config = configs.get(uid).unwrap_or(&default_config).clone();
    drop(configs);

    json!({
        "uid": uid,
        "background_download": config.background_download,
        "priority": config.priority,
        "download_limit": config.download_limit,
        "paused": config.paused,
    })
}
