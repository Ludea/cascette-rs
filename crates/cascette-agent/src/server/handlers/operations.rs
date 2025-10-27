//! Operation endpoint handlers
//!
//! Provides REST API for querying and managing operations.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{AgentError, OperationError, Result};
use crate::models::OperationState;
use crate::server::models::{OperationListResponse, OperationResponse};
use crate::state::AppState;

/// Query parameters for operation list
#[derive(Debug, Deserialize)]
pub struct OperationListQuery {
    /// Filter by product code
    pub product_code: Option<String>,

    /// Filter by state
    pub state: Option<OperationState>,

    /// Pagination limit
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Pagination offset
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

/// Generate ETag for operation response
///
/// ETag is based on operation_id, state, and updated_at timestamp.
/// This ensures the ETag changes whenever progress updates occur.
fn generate_etag(operation: &crate::models::Operation) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    operation.operation_id.hash(&mut hasher);
    format!("{:?}", operation.state).hash(&mut hasher);
    operation.updated_at.timestamp_millis().hash(&mut hasher);

    format!("\"{}\"", hasher.finish())
}

/// GET /`operations/{operation_id`} - Get operation details
///
/// Returns detailed information about a specific operation including
/// current state, progress, and error details if failed.
///
/// Supports conditional requests via If-None-Match header for efficient polling.
/// If the ETag matches the current operation state, returns 304 Not Modified.
///
/// # Headers
///
/// - `If-None-Match`: Optional ETag from previous request
/// - `ETag`: Returned with response, uniquely identifies current operation state
///
/// # Returns
///
/// - 200 OK: Operation found and returned
/// - 304 Not Modified: Operation unchanged since last poll (If-None-Match matches)
/// - 404 Not Found: Operation does not exist
pub async fn get_operation(
    State(state): State<Arc<AppState>>,
    Path(operation_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response> {
    let operation = state.queue.get_operation(operation_id)?;

    // Generate ETag for current operation state
    let etag = generate_etag(&operation);

    // Check If-None-Match header for conditional request
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH) {
        if let Ok(client_etag) = if_none_match.to_str() {
            if client_etag == etag {
                // Operation hasn't changed since last poll - return 304
                return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response());
            }
        }
    }

    // Return full response with ETag
    Ok((
        StatusCode::OK,
        [(header::ETAG, etag)],
        Json(OperationResponse::from(operation)),
    )
        .into_response())
}

/// GET /operations - List operations
///
/// Returns a list of operations matching the query filters.
/// Supports filtering by product code and state, with pagination.
///
/// # Query Parameters
///
/// - `product_code`: Filter by product (optional)
/// - `state`: Filter by operation state (optional)
/// - `limit`: Max results to return (default: 50)
/// - `offset`: Pagination offset (default: 0)
///
/// # Returns
///
/// - 200 OK: Operations list returned
pub async fn list_operations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OperationListQuery>,
) -> Result<Json<OperationListResponse>> {
    let mut operations = state.queue.list_operations()?;

    // Filter by product_code if specified
    if let Some(ref product_code) = query.product_code {
        operations.retain(|op| op.product_code == *product_code);
    }

    // Filter by state if specified
    if let Some(state_filter) = query.state {
        operations.retain(|op| op.state == state_filter);
    }

    let total = operations.len();

    // Apply pagination
    let start = query.offset.min(operations.len());
    let end = (query.offset + query.limit).min(operations.len());
    let paginated: Vec<_> = operations
        .into_iter()
        .skip(start)
        .take(end - start)
        .collect();

    let responses: Vec<OperationResponse> =
        paginated.into_iter().map(OperationResponse::from).collect();

    Ok(Json(OperationListResponse {
        operations: responses,
        total,
        links: None, // TODO: Add pagination links
    }))
}

/// POST /`operations/{operation_id}/cancel` - Cancel operation
///
/// Requests cancellation of an in-progress operation.
/// Operations in terminal states (complete, failed, cancelled) cannot be cancelled.
///
/// # Returns
///
/// - 200 OK: Cancellation requested
/// - 404 Not Found: Operation does not exist
/// - 409 Conflict: Operation cannot be cancelled (already complete)
pub async fn cancel_operation(
    State(state): State<Arc<AppState>>,
    Path(operation_id): Path<Uuid>,
) -> Result<Json<OperationResponse>> {
    // Get operation
    let mut operation = state.queue.get_operation(operation_id)?;

    // Check if operation can be cancelled
    if operation.is_terminal() {
        return Err(AgentError::Operation(OperationError::AlreadyCompleted(
            operation_id.to_string(),
        )));
    }

    // Set cancelled state
    operation.set_state(OperationState::Cancelled);

    // Update in database
    state.queue.update_operation(&operation)?;

    // Emit metrics
    state
        .metrics
        .record_operation_error(&operation.operation_type.to_string(), "cancelled");

    tracing::info!(
        operation_id = %operation_id,
        product_code = %operation.product_code,
        "Operation cancelled"
    );

    Ok(Json(OperationResponse::from(operation)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Operation, OperationType, Priority};
    use crate::observability::Metrics;
    use crate::state::{OperationQueue, ProductRegistry, db::Database};
    use std::sync::{Arc, Mutex};

    async fn setup_test_state() -> Arc<AppState> {
        let db = Arc::new(Mutex::new(
            Database::in_memory().expect("Failed to create test database"),
        ));
        let state = Arc::new(AppState {
            queue: Arc::new(OperationQueue::new(db.clone())),
            registry: Arc::new(ProductRegistry::new(db)),
            metrics: Arc::new(Metrics::new()),
        });

        // Create test products to satisfy foreign key constraints
        use crate::models::Product;
        for code in ["wow", "wow_0", "wow_1", "wow_2", "d3"] {
            let product = Product::new(code.to_string(), format!("{} Product", code));
            let _ = state.registry.create(&product);
        }

        state
    }

    #[tokio::test]
    async fn test_get_operation_not_found() {
        let state = setup_test_state().await;
        let operation_id = Uuid::new_v4();
        let headers = HeaderMap::new();

        let result = get_operation(State(state), Path(operation_id), headers).await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_operation_success() {
        let state = setup_test_state().await;

        // Create an operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // Get operation
        let headers = HeaderMap::new();
        let response = get_operation(State(state), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        // Extract JSON from response
        let status = response.status();
        assert_eq!(status, StatusCode::OK);

        // Verify ETag header is present
        let response_headers = response.headers();
        assert!(response_headers.contains_key(header::ETAG));
    }

    #[tokio::test]
    async fn test_list_operations_empty() {
        let state = setup_test_state().await;
        let query = OperationListQuery {
            product_code: None,
            state: None,
            limit: 50,
            offset: 0,
        };

        let result = list_operations(State(state), Query(query))
            .await
            .expect("Task should complete");

        assert_eq!(result.0.operations.len(), 0);
        assert_eq!(result.0.total, 0);
    }

    #[tokio::test]
    async fn test_list_operations_with_data() {
        let state = setup_test_state().await;

        // Create operations
        for i in 0..3 {
            let operation = Operation::new(
                format!("wow_{}", i),
                OperationType::Install,
                Priority::Normal,
            );
            state
                .queue
                .create_operation(&operation)
                .expect("Failed to create operation");
        }

        let query = OperationListQuery {
            product_code: None,
            state: None,
            limit: 50,
            offset: 0,
        };

        let result = list_operations(State(state), Query(query))
            .await
            .expect("Task should complete");

        assert_eq!(result.0.operations.len(), 3);
        assert_eq!(result.0.total, 3);
    }

    #[tokio::test]
    async fn test_list_operations_filter_by_product() {
        let state = setup_test_state().await;

        // Create operations for different products
        let op1 = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let op2 = Operation::new("d3".to_string(), OperationType::Install, Priority::Normal);
        state
            .queue
            .create_operation(&op1)
            .expect("Failed to create operation");
        state
            .queue
            .create_operation(&op2)
            .expect("Failed to create operation");

        let query = OperationListQuery {
            product_code: Some("wow".to_string()),
            state: None,
            limit: 50,
            offset: 0,
        };

        let result = list_operations(State(state), Query(query))
            .await
            .expect("Task should complete");

        assert_eq!(result.0.operations.len(), 1);
        assert_eq!(result.0.operations[0].product_code, "wow");
    }

    #[tokio::test]
    async fn test_cancel_operation() {
        let state = setup_test_state().await;

        // Create operation
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        operation.started_at = Some(chrono::Utc::now());
        operation.set_state(OperationState::Downloading);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // Cancel operation
        let result = cancel_operation(State(state.clone()), Path(operation_id))
            .await
            .expect("Operation should succeed");

        assert_eq!(result.0.state, OperationState::Cancelled);

        // Verify in database
        let updated = state
            .queue
            .get_operation(operation_id)
            .expect("Failed to get operation");
        assert_eq!(updated.state, OperationState::Cancelled);
    }

    #[tokio::test]
    async fn test_cancel_completed_operation() {
        let state = setup_test_state().await;

        // Create completed operation
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        operation.set_state(OperationState::Complete);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // Try to cancel
        let result = cancel_operation(State(state), Path(operation_id)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::Operation(OperationError::AlreadyCompleted(_))
        ));
    }

    #[tokio::test]
    async fn test_etag_generation() {
        let state = setup_test_state().await;

        // Create an operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // First request - get ETag
        let headers = HeaderMap::new();
        let response = get_operation(State(state.clone()), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let etag = response
            .headers()
            .get(header::ETAG)
            .expect("ETag header should be present")
            .to_str()
            .expect("ETag should be valid string");

        // Verify ETag is quoted
        assert!(etag.starts_with('"'));
        assert!(etag.ends_with('"'));
    }

    #[tokio::test]
    async fn test_etag_not_modified() {
        let state = setup_test_state().await;

        // Create an operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // First request - get ETag
        let headers = HeaderMap::new();
        let response = get_operation(State(state.clone()), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        let etag = response
            .headers()
            .get(header::ETAG)
            .expect("ETag header should be present")
            .to_str()
            .expect("ETag should be valid string")
            .to_string();

        // Second request with If-None-Match
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            etag.parse().expect("Failed to parse ETag"),
        );

        let response = get_operation(State(state), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        // Should return 304 Not Modified
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        // Verify ETag header is still present
        assert!(response.headers().contains_key(header::ETAG));
    }

    #[tokio::test]
    async fn test_etag_changes_on_update() {
        let state = setup_test_state().await;

        // Create an operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // First request - get initial ETag
        let headers = HeaderMap::new();
        let response = get_operation(State(state.clone()), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        let initial_etag = response
            .headers()
            .get(header::ETAG)
            .expect("ETag header should be present")
            .to_str()
            .expect("ETag should be valid string")
            .to_string();

        // Update operation state (Queued → Cancelled is a valid transition)
        let mut operation = state
            .queue
            .get_operation(operation_id)
            .expect("Failed to get operation");
        operation.set_state(OperationState::Cancelled);
        state
            .queue
            .update_operation(&operation)
            .expect("Failed to update operation");

        // Second request - get updated ETag
        let headers = HeaderMap::new();
        let response = get_operation(State(state.clone()), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        let updated_etag = response
            .headers()
            .get(header::ETAG)
            .expect("ETag header should be present")
            .to_str()
            .expect("ETag should be valid string");

        // ETags should be different
        assert_ne!(initial_etag, updated_etag);
    }

    #[tokio::test]
    async fn test_etag_mismatch_returns_full_response() {
        let state = setup_test_state().await;

        // Create an operation
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        let operation_id = operation.operation_id;
        state
            .queue
            .create_operation(&operation)
            .expect("Failed to create operation");

        // Request with incorrect If-None-Match
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            "\"incorrect_etag\"".parse().expect("Failed to parse ETag"),
        );

        let response = get_operation(State(state), Path(operation_id), headers)
            .await
            .expect("Operation should succeed");

        // Should return 200 OK with full response
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(header::ETAG));
    }
}
