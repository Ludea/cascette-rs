//! GET/POST /gamesession, /gamesession/{product} -- Game session tracking.
//!
//! Tracks running game processes. The agent validates the caller's OS PID
//! against the `pid` field in the JSON body by checking the supplied PID
//! against live game process PIDs via process_detection — rejecting PIDs that
//! are not known game processes for the given product.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::handlers::AppState;
use crate::process_detection;

/// Session start request.
#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    /// Product unique identifier.
    pub uid: Option<String>,
    /// Process ID of the game.
    pub pid: Option<u32>,
}

pub async fn sessions(state: &Arc<AppState>) -> Value {
    let sessions = state.session_tracker.list().await;
    let session_data: Vec<Value> = sessions
        .iter()
        .map(|s| {
            return json!({
                "uid": s.product_code,
                "active": true,
                "pid": s.pid,
                "started_at": s.started_at.to_rfc3339(),
            });
        })
        .collect();

    json!({
        "sessions": session_data,
    })
}

pub async fn session(state: &Arc<AppState>, product: String) -> Value {
    // Check tracker first, fall back to process detection
    if let Some(session) = state.session_tracker.get(&product).await {
        return json!({
            "uid": product,
            "active": true,
            "pid": session.pid,
            "started_at": session.started_at.to_rfc3339(),
        });
    }

    // Fall back to process detection
    let active = process_detection::is_game_running(&product);
    let pids = if active {
        process_detection::game_pids(&product)
    } else {
        vec![]
    };

    json!({
        "uid": product,
        "active": active,
        "pid": pids.first().copied(),
    })
}

pub async fn set_session(state: &Arc<AppState>, body: SessionRequest, product: String) -> Value {
    let uid = body.uid.as_deref().unwrap_or(&product);

    // Validate pid against live game processes if supplied.
    if let Some(pid) = body.pid {
        let live_pids = process_detection::game_pids(uid);
        // If the product has known executables but the PID is not among them,
        // reject the request. If the product has no known executables (empty list),
        // allow through — unrecognised products should not block session tracking.
        let names = process_detection::executable_names(uid);
        if !names.is_empty() && !live_pids.contains(&pid) {
            return json!({
                "error": 2312,
                "message": format!("Unable to validate connecting process ({pid})"),
            });
        }
    }

    state.session_tracker.start_session(uid, body.pid).await;

    // Update metrics
    let count = state.session_tracker.count().await;
    #[allow(clippy::cast_possible_wrap)]
    state.metrics.active_game_sessions.set(count as i64);

    json!({
        "uid": uid,
        "active": true,
        "pid": body.pid,
    })
}
