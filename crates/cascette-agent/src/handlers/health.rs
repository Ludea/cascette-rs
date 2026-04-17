use std::sync::Arc;

use serde_json::{Value, json};

use crate::handlers::AppState;

pub async fn health(state: &Arc<AppState>) -> Value {
    let uptime = state.started_at.elapsed().unwrap_or_default().as_secs();

    json!({
        "status": "ok",
        "version": state.agent_version,
        "uptime_seconds": uptime,
    })
}
