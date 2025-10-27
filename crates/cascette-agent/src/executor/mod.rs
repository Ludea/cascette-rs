//! Operation executor infrastructure
//!
//! Provides traits and utilities for executing operations (install, update, repair, etc.)
//! with support for cancellation, progress tracking, and state management.

pub mod context;
pub mod factory;
pub mod install;
pub mod progress_adapter;
pub mod repair;
pub mod runner;
pub mod uninstall;
pub mod update;
pub mod verify;

use crate::error::Result;
use crate::models::{Operation, OperationState, Progress};
use async_trait::async_trait;

#[cfg(test)]
use tokio_util::sync::CancellationToken;

/// Result type for operation execution
///
/// Contains the final operation state after execution completes.
pub type ExecutionResult = Result<OperationState>;

/// Trait for executing operations
///
/// Implementations handle the actual work of installing, updating, repairing,
/// verifying, or uninstalling products. The trait provides async methods with
/// cancellation support and progress callbacks.
///
/// # Cancellation
///
/// Operations must respect the cancellation token and return promptly when
/// cancelled. Partial work should be cleaned up or left in a resumable state.
///
/// # Progress Tracking
///
/// Operations should call the progress callback regularly to update the
/// operation state and provide progress metrics to users.
///
/// # State Management
///
/// Operations are responsible for transitioning through valid states:
/// - Queued → Initializing → Downloading → Verifying → Complete
/// - Any state → Failed (on error)
/// - Active states → Cancelled (on cancellation)
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::executor::{OperationExecutor, ExecutionContext};
/// use cascette_agent::models::{Operation, OperationType, Priority};
/// use tokio_util::sync::CancellationToken;
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// struct InstallExecutor;
///
/// #[async_trait::async_trait]
/// impl OperationExecutor for InstallExecutor {
///     async fn execute(
///         &self,
///         operation: &mut Operation,
///         context: &ExecutionContext,
///     ) -> cascette_agent::error::Result<cascette_agent::models::OperationState> {
///         // Implementation details...
///         Ok(cascette_agent::models::OperationState::Complete)
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait OperationExecutor: Send + Sync {
    /// Execute an operation
    ///
    /// Performs the work defined by the operation type and returns the final
    /// state. The operation is mutated in-place to track progress and state
    /// changes.
    ///
    /// # Arguments
    ///
    /// - `operation`: Mutable reference to the operation being executed
    /// - `context`: Execution context providing shared resources
    ///
    /// # Returns
    ///
    /// - `Ok(OperationState)`: Final state (Complete, Failed, or Cancelled)
    /// - `Err(AgentError)`: Fatal error preventing execution
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Operation is in invalid state for execution
    /// - Required resources are unavailable
    /// - Fatal system error occurs
    ///
    /// Recoverable errors should transition operation to Failed state and
    /// return Ok(OperationState::Failed).
    // Future use: Operation runner (currently only used in tests)
    #[allow(dead_code)]
    async fn execute(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult;

    // Future use: Operation runner cancellation
    #[allow(dead_code)]
    /// Cancel an in-progress operation
    ///
    /// Signals cancellation and waits for the operation to clean up. The
    /// operation should transition to Cancelled state and return promptly.
    ///
    /// # Arguments
    ///
    /// - `operation`: Operation to cancel
    /// - `context`: Execution context
    ///
    /// # Default Implementation
    ///
    /// Sets the cancellation token and waits for execute() to return.
    async fn cancel(&self, _operation: &Operation, context: &ExecutionContext) -> Result<()> {
        context.cancellation_token.cancel();
        Ok(())
    }

    // Future use: T074-T077 (resume support)
    #[allow(dead_code)]
    /// Resume a previously interrupted operation
    ///
    /// Attempts to continue execution from the last checkpoint. Operations
    /// should load resume state and skip already-completed work.
    ///
    /// # Arguments
    ///
    /// - `operation`: Operation to resume
    /// - `context`: Execution context
    ///
    /// # Returns
    ///
    /// - `Ok(OperationState)`: Final state after resume
    /// - `Err(AgentError)`: Cannot resume operation
    ///
    /// # Default Implementation
    ///
    /// Delegates to execute() which should handle resume internally.
    async fn resume(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult {
        self.execute(operation, context).await
    }
}

/// Progress callback for operation execution
///
/// Enables executors to report progress updates during long-running operations.
/// Implementations should update the operation state and persist changes.
pub trait ProgressReporter: Send + Sync {
    /// Report progress update
    ///
    /// Called periodically during operation execution to update progress
    /// metrics. Should persist changes for resume support.
    ///
    /// # Arguments
    ///
    /// - `operation_id`: UUID of the operation
    /// - `progress`: Current progress metrics
    fn report_progress(&self, operation_id: uuid::Uuid, progress: Progress);

    /// Report state transition
    ///
    /// Called when operation transitions to a new state. Should validate
    /// transition and persist changes.
    ///
    /// # Arguments
    ///
    /// - `operation_id`: UUID of the operation
    /// - `new_state`: Target state
    fn report_state_change(&self, operation_id: uuid::Uuid, new_state: OperationState);

    /// Report operation error
    ///
    /// Called when operation encounters an error. Should transition to Failed
    /// state and persist error details.
    ///
    /// # Arguments
    ///
    /// - `operation_id`: UUID of the operation
    /// - `error_code`: Machine-readable error code
    /// - `error_message`: Human-readable error message
    /// - `details`: Optional structured error details
    fn report_error(
        &self,
        operation_id: uuid::Uuid,
        error_code: String,
        error_message: String,
        details: Option<serde_json::Value>,
    );
}

/// Re-export context type for convenience
pub use context::ExecutionContext;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Operation, OperationType, Priority};
    use crate::observability::metrics::Metrics;
    use crate::state::{Database, ProductRegistry};
    use std::sync::{Arc, Mutex};

    /// Mock executor for testing
    struct MockExecutor {
        final_state: OperationState,
    }

    #[async_trait]
    impl OperationExecutor for MockExecutor {
        async fn execute(
            &self,
            operation: &mut Operation,
            _context: &ExecutionContext,
        ) -> ExecutionResult {
            operation.set_state(self.final_state);
            Ok(self.final_state)
        }
    }

    /// Mock progress reporter for testing
    struct MockProgressReporter;

    impl ProgressReporter for MockProgressReporter {
        fn report_progress(&self, _operation_id: uuid::Uuid, _progress: Progress) {
            // No-op for testing
        }

        fn report_state_change(&self, _operation_id: uuid::Uuid, _new_state: OperationState) {
            // No-op for testing
        }

        fn report_error(
            &self,
            _operation_id: uuid::Uuid,
            _error_code: String,
            _error_message: String,
            _details: Option<serde_json::Value>,
        ) {
            // No-op for testing
        }
    }

    #[tokio::test]
    async fn test_executor_complete() {
        let executor = MockExecutor {
            final_state: OperationState::Complete,
        };

        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let config = crate::config::AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            std::sync::Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        let result = executor.execute(&mut operation, &context).await;

        assert!(result.is_ok());
        assert_eq!(
            result.expect("Operation should succeed"),
            OperationState::Complete
        );
        assert_eq!(operation.state, OperationState::Complete);
    }

    #[tokio::test]
    async fn test_executor_failed() {
        let executor = MockExecutor {
            final_state: OperationState::Failed,
        };

        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let config = crate::config::AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            std::sync::Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        let result = executor.execute(&mut operation, &context).await;

        assert!(result.is_ok());
        assert_eq!(
            result.expect("Operation should succeed"),
            OperationState::Failed
        );
        assert_eq!(operation.state, OperationState::Failed);
    }

    #[tokio::test]
    async fn test_executor_cancel() {
        let executor = MockExecutor {
            final_state: OperationState::Complete,
        };

        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let config = crate::config::AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            std::sync::Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        assert!(!context.is_cancelled());

        let result = executor.cancel(&operation, &context).await;
        assert!(result.is_ok());
        assert!(context.is_cancelled());
    }

    #[tokio::test]
    async fn test_executor_resume_delegates_to_execute() {
        let executor = MockExecutor {
            final_state: OperationState::Complete,
        };

        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let config = crate::config::AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            std::sync::Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        // Default resume implementation should call execute
        let result = executor.resume(&mut operation, &context).await;

        assert!(result.is_ok());
        assert_eq!(
            result.expect("Operation should succeed"),
            OperationState::Complete
        );
    }
}
