//! Operation cancellation endpoint for agent service
//!
//! Implements T072:
//! - POST /`operations/{operation_id}/cancel` - Cancel an operation
//!
//! Cancellation is graceful: operations in progress will stop at the next
//! safe checkpoint and clean up resources.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{AgentError, OperationError};
use crate::models::OperationState;
use crate::state::AppState;

/// POST /operations/{operation_id}/cancel
///
/// Cancels an operation gracefully. The operation will stop at the next
/// safe checkpoint and transition to Cancelled state.
///
/// Only operations in active states (Queued, Initializing, Downloading, Verifying)
/// can be cancelled. Terminal states (Complete, Failed, Cancelled) cannot be cancelled.
///
/// Responses:
/// - 204 No Content: Operation cancelled successfully
/// - 404 Not Found: Operation does not exist
/// - 409 Conflict: Operation is already in a terminal state
#[tracing::instrument(skip(state))]
pub async fn cancel_operation(
    State(state): State<Arc<AppState>>,
    Path(operation_id): Path<Uuid>,
) -> Result<impl IntoResponse, AgentError> {
    // Get the operation
    let mut operation = state.queue.get(operation_id)?;

    // Check if operation can be cancelled
    if operation.is_terminal() {
        return Err(AgentError::Operation(OperationError::AlreadyCompleted(
            operation_id.to_string(),
        )));
    }

    // Set cancellation flag by transitioning to Cancelled state
    operation.set_state(OperationState::Cancelled);

    // Update operation in queue
    state.queue.update(&operation)?;

    // Log cancellation
    tracing::info!(
        operation_id = %operation_id,
        product_code = %operation.product_code,
        "Operation cancelled by user request"
    );

    // Return 204 No Content
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Operation, OperationType, Priority};
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
    async fn test_cancel_queued_operation() {
        let state = create_test_state();
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        let result = cancel_operation(State(state.clone()), Path(operation.operation_id)).await;

        assert!(result.is_ok());

        // Verify operation is cancelled
        let updated = state
            .queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert_eq!(updated.state, OperationState::Cancelled);
        assert!(updated.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_cancel_in_progress_operation() {
        let state = create_test_state();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        // Move to Initializing state
        operation.set_state(OperationState::Initializing);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");

        let result = cancel_operation(State(state.clone()), Path(operation.operation_id)).await;

        assert!(result.is_ok());

        // Verify operation is cancelled
        let updated = state
            .queue
            .get(operation.operation_id)
            .expect("Failed to get operation");
        assert_eq!(updated.state, OperationState::Cancelled);
    }

    #[tokio::test]
    async fn test_cancel_completed_operation() {
        let state = create_test_state();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        // Move through states to Complete
        operation.set_state(OperationState::Initializing);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Downloading);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Verifying);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");
        operation.set_state(OperationState::Complete);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");

        let result = cancel_operation(State(state.clone()), Path(operation.operation_id)).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(
                e,
                AgentError::Operation(OperationError::AlreadyCompleted(_))
            ));
        }
    }

    #[tokio::test]
    async fn test_cancel_failed_operation() {
        let state = create_test_state();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        // Transition to Initializing first (valid from Queued)
        operation.set_state(OperationState::Initializing);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");

        // Now set to failed state (valid from Initializing)
        operation.set_error("TEST_ERROR".to_string(), "Test error".to_string(), None);
        state
            .queue
            .update(&operation)
            .expect("Failed to update operation");

        let result = cancel_operation(State(state.clone()), Path(operation.operation_id)).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(
                e,
                AgentError::Operation(OperationError::AlreadyCompleted(_))
            ));
        }
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_operation() {
        let state = create_test_state();

        let result = cancel_operation(State(state), Path(Uuid::new_v4())).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(
                e,
                AgentError::Operation(OperationError::NotFound(_))
            ));
        }
    }

    #[tokio::test]
    async fn test_cancel_already_cancelled_operation() {
        let state = create_test_state();
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        state
            .queue
            .create(&operation)
            .expect("Failed to create operation");

        // Cancel first time
        cancel_operation(State(state.clone()), Path(operation.operation_id))
            .await
            .expect("Operation should succeed");

        // Try to cancel again
        let result = cancel_operation(State(state.clone()), Path(operation.operation_id)).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(
                e,
                AgentError::Operation(OperationError::AlreadyCompleted(_))
            ));
        }
    }
}
