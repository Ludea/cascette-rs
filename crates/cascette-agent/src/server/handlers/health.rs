//! Health check endpoint handler
//!
//! Provides service health status for monitoring and load balancing.

use axum::{Json, extract::State};
use std::sync::Arc;
use std::time::Instant;

use crate::server::models::HealthResponse;
use crate::state::AppState;

/// Service start time for uptime calculation
static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Initialize start time
pub fn init_start_time() {
    START_TIME.get_or_init(Instant::now);
}

/// GET /health - Health check endpoint
///
/// Returns service health status, version, uptime, and active operations count.
/// Always returns 200 OK if the service is running.
pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let start_time = START_TIME.get().copied().unwrap_or_else(Instant::now);
    let uptime = start_time.elapsed();

    // Get active operations count
    let active_count = state.queue.active_operations_count().unwrap_or(0);

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: uptime.as_secs(),
        active_operations: active_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::Metrics;
    use crate::state::{OperationQueue, ProductRegistry, db::Database};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_health_check() {
        init_start_time();

        let db = Arc::new(Mutex::new(
            Database::in_memory().expect("Failed to create test database"),
        ));
        let state = Arc::new(AppState {
            queue: Arc::new(OperationQueue::new(db.clone())),
            registry: Arc::new(ProductRegistry::new(db)),
            metrics: Arc::new(Metrics::new()),
        });

        let response = health_check(State(state)).await;

        assert_eq!(response.0.status, "healthy");
        assert!(!response.0.version.is_empty());
        assert_eq!(response.0.active_operations, 0);
    }
}
