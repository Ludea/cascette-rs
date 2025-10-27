//! Progress monitoring endpoints for agent service
//!
//! Implements T069-T071:
//! - GET /`operations/{operation_id}/progress` - Get operation progress
//! - GET /`products/{product_code}/progress` - Get aggregated product progress
//!
//! Supports long-polling with ?wait=true&timeout=30 query parameters for
//! efficient real-time updates.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

use crate::error::{AgentError, OperationError};
use crate::state::AppState;

/// Query parameters for progress endpoints
#[derive(Debug, Deserialize)]
pub struct ProgressQuery {
    /// Enable long-polling (wait for state change)
    #[serde(default)]
    wait: bool,

    /// Long-poll timeout in seconds (max 30)
    #[serde(default = "default_timeout")]
    timeout: u32,
}

fn default_timeout() -> u32 {
    30
}

/// Progress response for a single operation
#[derive(Debug, Serialize)]
pub struct OperationProgressResponse {
    /// Operation identifier
    pub operation_id: String,

    /// Current operation state
    pub state: String,

    /// Progress details (null if operation not started)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressDetails>,

    /// When progress was last updated
    pub updated_at: String,
}

impl IntoResponse for OperationProgressResponse {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// Detailed progress metrics
#[derive(Debug, Serialize)]
pub struct ProgressDetails {
    /// Current phase (downloading, verifying, etc.)
    pub phase: String,

    /// Completion percentage (0.0-100.0)
    pub percentage: f64,

    /// Bytes downloaded
    pub bytes_downloaded: u64,

    /// Total bytes
    pub bytes_total: u64,

    /// Files completed
    pub files_completed: usize,

    /// Total files
    pub files_total: usize,

    /// Current file being processed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,

    /// Download speed in bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_speed_bps: Option<u64>,

    /// Estimated time to completion in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
}

/// Aggregated progress response for all operations on a product
#[derive(Debug, Serialize)]
pub struct ProductProgressResponse {
    /// Product code
    pub product_code: String,

    /// Active operations on this product
    pub active_operations: Vec<OperationProgressResponse>,

    /// Aggregated progress (if any active operations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregated: Option<AggregatedProgress>,
}

/// Aggregated progress metrics across multiple operations
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedProgress {
    /// Overall percentage (weighted average)
    pub percentage: f64,

    /// Total bytes downloaded across all operations
    pub bytes_downloaded: u64,

    /// Total bytes across all operations
    pub bytes_total: u64,

    /// Total files completed across all operations
    pub files_completed: usize,

    /// Total files across all operations
    pub files_total: usize,

    /// Combined download speed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_speed_bps: Option<u64>,

    /// Estimated time to completion (slowest operation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
}

/// GET /operations/{operation_id}/progress
///
/// Returns current progress for an operation. Supports long-polling to wait
/// for state changes.
///
/// Query parameters:
/// - wait: Enable long-polling (default: false)
/// - timeout: Long-poll timeout in seconds (default: 30, max: 30)
///
/// Responses:
/// - 200 OK: Progress returned
/// - 304 Not Modified: No change during long-poll timeout
/// - 404 Not Found: Operation does not exist
#[tracing::instrument(skip(state))]
pub async fn get_operation_progress(
    State(state): State<Arc<AppState>>,
    Path(operation_id): Path<Uuid>,
    Query(params): Query<ProgressQuery>,
) -> Result<Response, AgentError> {
    // Get initial operation state
    let operation = state.queue.get(operation_id)?;
    let initial_updated_at = operation.updated_at;

    // If long-polling is enabled, wait for state change
    if params.wait {
        let timeout = params.timeout.min(30);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(timeout));
        let poll_interval = Duration::from_millis(500);

        loop {
            // Check if we've exceeded timeout
            if tokio::time::Instant::now() >= deadline {
                // Return 304 Not Modified
                return Ok(StatusCode::NOT_MODIFIED.into_response());
            }

            // Wait before checking again
            sleep(poll_interval).await;

            // Get current operation state
            match state.queue.get(operation_id) {
                Ok(current) => {
                    // If state or progress changed, return immediately
                    if current.updated_at > initial_updated_at {
                        return Ok(build_progress_response(&current).into_response());
                    }
                }
                Err(AgentError::Operation(OperationError::NotFound(_))) => {
                    // Operation was deleted during polling
                    return Err(AgentError::Operation(OperationError::NotFound(
                        operation_id.to_string(),
                    )));
                }
                Err(e) => return Err(e),
            }
        }
    } else {
        // Return immediately without long-polling
        Ok(build_progress_response(&operation).into_response())
    }
}

/// GET /products/{product_code}/progress
///
/// Returns aggregated progress for all active operations on a product.
///
/// Responses:
/// - 200 OK: Progress returned (may have no active operations)
/// - 400 Bad Request: Invalid product code
#[tracing::instrument(skip(state))]
pub async fn get_product_progress(
    State(state): State<Arc<AppState>>,
    Path(product_code): Path<String>,
) -> Result<Json<ProductProgressResponse>, AgentError> {
    // Get all operations for the product
    let operations = state.queue.list_by_product(&product_code)?;

    // Filter to only active (non-terminal) operations
    let active_operations: Vec<_> = operations
        .into_iter()
        .filter(crate::models::operation::Operation::is_active)
        .collect();

    // Build individual progress responses
    let progress_responses: Vec<_> = active_operations
        .iter()
        .map(build_progress_response)
        .collect();

    // Calculate aggregated progress
    let aggregated = if active_operations.is_empty() {
        None
    } else {
        Some(calculate_aggregated_progress(&active_operations))
    };

    Ok(Json(ProductProgressResponse {
        product_code,
        active_operations: progress_responses,
        aggregated,
    }))
}

/// Build a progress response from an operation
fn build_progress_response(operation: &crate::models::Operation) -> OperationProgressResponse {
    OperationProgressResponse {
        operation_id: operation.operation_id.to_string(),
        state: format!("{:?}", operation.state).to_lowercase(),
        progress: operation.progress.as_ref().map(|p| ProgressDetails {
            phase: p.phase.clone(),
            percentage: p.percentage,
            bytes_downloaded: p.bytes_downloaded,
            bytes_total: p.bytes_total,
            files_completed: p.files_completed,
            files_total: p.files_total,
            current_file: p.current_file.clone(),
            download_speed_bps: p.download_speed_bps,
            eta_seconds: p.eta_seconds,
        }),
        updated_at: operation.updated_at.to_rfc3339(),
    }
}

/// Calculate aggregated progress across multiple operations
fn calculate_aggregated_progress(operations: &[crate::models::Operation]) -> AggregatedProgress {
    let mut total_bytes_downloaded = 0u64;
    let mut total_bytes_total = 0u64;
    let mut total_files_completed = 0usize;
    let mut total_files_total = 0usize;
    let mut total_speed = 0u64;
    let mut max_eta: Option<u64> = None;

    for operation in operations {
        if let Some(progress) = &operation.progress {
            total_bytes_downloaded += progress.bytes_downloaded;
            total_bytes_total += progress.bytes_total;
            total_files_completed += progress.files_completed;
            total_files_total += progress.files_total;

            if let Some(speed) = progress.download_speed_bps {
                total_speed += speed;
            }

            if let Some(eta) = progress.eta_seconds {
                max_eta = Some(max_eta.map_or(eta, |current_max: u64| current_max.max(eta)));
            }
        }
    }

    let percentage = if total_bytes_total > 0 {
        (total_bytes_downloaded as f64 / total_bytes_total as f64) * 100.0
    } else {
        0.0
    };

    AggregatedProgress {
        percentage,
        bytes_downloaded: total_bytes_downloaded,
        bytes_total: total_bytes_total,
        files_completed: total_files_completed,
        files_total: total_files_total,
        download_speed_bps: if total_speed > 0 {
            Some(total_speed)
        } else {
            None
        },
        eta_seconds: max_eta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Operation, OperationType, Priority, Progress};
    use crate::state::{Database, OperationQueue, ProductRegistry};
    use std::sync::Mutex;

    fn create_test_state() -> Arc<AppState> {
        let db = Database::in_memory().expect("Failed to create test database");

        // Insert a test product
        db.connection()
            .execute(
                "INSERT INTO products (product_code, name, status, created_at, updated_at)
                 VALUES ('wow', 'World of Warcraft', 'Available', datetime('now'), datetime('now'))",
                [],
            )
            .expect("Operation should succeed");

        let db_arc = Arc::new(Mutex::new(db));
        let queue = Arc::new(OperationQueue::new(db_arc.clone()));
        let registry = Arc::new(ProductRegistry::new(db_arc));
        let metrics = Arc::new(crate::observability::Metrics::new());

        Arc::new(AppState::new(queue, registry, metrics))
    }

    #[tokio::test]
    async fn test_get_operation_progress() {
        let state = create_test_state();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        // Add progress
        let mut progress = Progress::new("downloading".to_string(), 10000, 100);
        progress.update_bytes(5000);
        progress.set_download_speed(1000);
        operation.set_progress(progress);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        // Get progress without long-polling
        let result = get_operation_progress(
            State(state.clone()),
            Path(operation.operation_id),
            Query(ProgressQuery {
                wait: false,
                timeout: 30,
            }),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_operation_progress_not_found() {
        let state = create_test_state();

        let result = get_operation_progress(
            State(state),
            Path(Uuid::new_v4()),
            Query(ProgressQuery {
                wait: false,
                timeout: 30,
            }),
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_product_progress_empty() {
        let state = create_test_state();

        let result = get_product_progress(State(state), Path("wow".to_string())).await;

        assert!(result.is_ok());
        let response = result.expect("Operation should succeed");
        assert_eq!(response.product_code, "wow");
        assert!(response.active_operations.is_empty());
        assert!(response.aggregated.is_none());
    }

    #[tokio::test]
    async fn test_get_product_progress_with_operations() {
        let state = create_test_state();

        // Create operation with progress
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let mut progress = Progress::new("downloading".to_string(), 10000, 100);
        progress.update_bytes(5000);
        operation.set_progress(progress);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        let result = get_product_progress(State(state), Path("wow".to_string())).await;

        assert!(result.is_ok());
        let response = result.expect("Operation should succeed");
        assert_eq!(response.active_operations.len(), 1);
        assert!(response.aggregated.is_some());

        let agg = response
            .aggregated
            .clone()
            .expect("Aggregated progress should exist");
        assert_eq!(agg.bytes_downloaded, 5000);
        assert_eq!(agg.bytes_total, 10000);
        assert_eq!(agg.percentage, 50.0);
    }

    #[tokio::test]
    async fn test_aggregated_progress_calculation() {
        let mut op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let mut progress1 = Progress::new("downloading".to_string(), 10000, 100);
        progress1.update_bytes(5000);
        progress1.set_download_speed(1000);
        op1.set_progress(progress1);

        let mut op2 = Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);
        let mut progress2 = Progress::new("downloading".to_string(), 20000, 200);
        progress2.update_bytes(10000);
        progress2.set_download_speed(2000);
        op2.set_progress(progress2);

        let operations = vec![op1, op2];
        let agg = calculate_aggregated_progress(&operations);

        assert_eq!(agg.bytes_downloaded, 15000);
        assert_eq!(agg.bytes_total, 30000);
        assert_eq!(agg.percentage, 50.0);
        assert_eq!(agg.download_speed_bps, Some(3000));
    }

    #[tokio::test]
    async fn test_build_progress_response() {
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let mut progress = Progress::new("downloading".to_string(), 10000, 100);
        progress.update_bytes(5000);
        progress.set_current_file("data.001".to_string());
        operation.set_progress(progress);

        let response = build_progress_response(&operation);

        assert_eq!(response.operation_id, operation.operation_id.to_string());
        assert_eq!(response.state, "queued");
        assert!(response.progress.is_some());

        let prog = response.progress.expect("Progress should exist");
        assert_eq!(prog.phase, "downloading");
        assert_eq!(prog.percentage, 50.0);
        assert_eq!(prog.current_file, Some("data.001".to_string()));
    }
}
