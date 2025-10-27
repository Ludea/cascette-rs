//! Metrics endpoint for Prometheus scraping
//!
//! Implements T041: Expose prometheus-client metrics in Prometheus text format
//!
//! ## Endpoint
//!
//! - `GET /metrics` - Returns Prometheus metrics in text format
//!
//! ## Metrics Exposed
//!
//! See `crate::observability::metrics` for complete metric descriptions:
//! - `operation_total` - Total operations started (by type)
//! - `operation_errors` - Total operation errors (by type and error category)
//! - `active_operations` - Currently active operations
//! - `operation_duration_seconds` - Operation completion time distribution
//! - `download_bytes_per_second` - Download throughput distribution

use axum::{extract::State, http::StatusCode, response::IntoResponse};
use std::sync::Arc;

use crate::state::AppState;

/// GET /metrics - Prometheus metrics endpoint
///
/// Returns all metrics in Prometheus text format for scraping.
///
/// # Response Format
///
/// ```text
/// # HELP operation_total Total number of operations started
/// # TYPE operation_total counter
/// operation_total{operation_type="install"} 5
/// operation_total{operation_type="update"} 2
///
/// # HELP active_operations Number of currently active operations
/// # TYPE active_operations gauge
/// active_operations 1
/// ...
/// ```
///
/// # Examples
///
/// ```bash
/// curl http://localhost:1120/metrics
/// ```
pub async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let registry = state.metrics.registry();

    let mut buffer = String::new();
    match prometheus_client::encoding::text::encode(&mut buffer, &registry) {
        Ok(()) => (
            StatusCode::OK,
            [("Content-Type", "text/plain; version=0.0.4")],
            buffer,
        ),
        Err(e) => {
            tracing::error!(error = %e, "Failed to encode metrics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("Content-Type", "text/plain; version=0.0.4")],
                format!("Error encoding metrics: {}", e),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{observability::Metrics, state::OperationQueue, state::ProductRegistry};
    use std::sync::Mutex;

    #[tokio::test]
    async fn test_metrics_endpoint() {
        // Create test state
        let db = Arc::new(Mutex::new(
            crate::state::Database::open(":memory:").expect("Failed to create test database"),
        ));
        let queue = Arc::new(OperationQueue::new(db.clone()));
        let registry = Arc::new(ProductRegistry::new(db));
        let metrics_obj = Arc::new(Metrics::new());
        let state = Arc::new(AppState::new(queue, registry, metrics_obj.clone()));

        // Record some test metrics
        metrics_obj.record_operation_start("install");
        metrics_obj.record_download_throughput(1_000_000.0);

        // Call endpoint
        let response = metrics(State(state)).await.into_response();

        assert_eq!(response.status(), StatusCode::OK);

        // Check content type
        let content_type = response
            .headers()
            .get("content-type")
            .expect("Content-Type header should be present");
        assert_eq!(content_type, "text/plain; version=0.0.4");
    }

    #[tokio::test]
    async fn test_metrics_contains_expected_metrics() {
        // Create test state
        let db = Arc::new(Mutex::new(
            crate::state::Database::open(":memory:").expect("Failed to create test database"),
        ));
        let queue = Arc::new(OperationQueue::new(db.clone()));
        let registry = Arc::new(ProductRegistry::new(db));
        let metrics_obj = Arc::new(Metrics::new());
        let state = Arc::new(AppState::new(queue, registry, metrics_obj.clone()));

        // Record metrics
        metrics_obj.record_operation_start("install");
        metrics_obj.record_operation_complete("install", 120.5);

        // Call endpoint
        let response = metrics(State(state)).await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");
        let body_text = String::from_utf8(body.to_vec()).expect("Failed to parse body as UTF-8");

        // Verify metrics are present
        assert!(body_text.contains("operation_total"));
        assert!(body_text.contains("active_operations"));
        assert!(body_text.contains("operation_duration_seconds"));
        assert!(body_text.contains("download_bytes_per_second"));
    }
}
