//! Execution runner for background operation orchestration
//!
//! Polls the operation queue, spawns executors, manages concurrency limits,
//! and handles cancellation and shutdown.

use crate::{
    config::AgentConfig,
    error::Result,
    executor::{ExecutionContext, ProgressReporter, factory::ExecutorFactory},
    models::{Operation, OperationState},
};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{Duration, interval};
use tokio_util::sync::CancellationToken;

/// Background execution runner
///
/// Orchestrates operation execution by polling the operation queue and
/// spawning executors with concurrency control.
///
/// # Features
///
/// - Concurrent operation execution with configurable limits
/// - Graceful shutdown on cancellation
/// - Automatic retry for transient errors
/// - Progress tracking and state management
///
/// # Examples
///
/// ```ignore
/// use cascette_agent::executor::runner::ExecutionRunner;
/// use cascette_agent::config::AgentConfig;
/// use tokio_util::sync::CancellationToken;
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// # struct MockOperationQueue;
/// # impl MockOperationQueue {
/// #     async fn dequeue(&self) -> Option<cascette_agent::models::Operation> { None }
/// #     async fn complete(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
/// # }
/// # struct MockProgressReporter;
/// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
/// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
/// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
/// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
/// # }
/// let config = AgentConfig::default();
/// let queue = std::sync::Arc::new(MockOperationQueue);
/// let reporter = std::sync::Arc::new(MockProgressReporter);
/// let shutdown_token = CancellationToken::new();
///
/// let runner = ExecutionRunner::new(config, queue, reporter, registry.clone(), metrics.clone(), shutdown_token.clone());
///
/// // Run in background
/// let handle = tokio::spawn(async move {
///     runner.run().await
/// });
///
/// // Shutdown
/// shutdown_token.cancel();
/// handle.await??;
/// # Ok(())
/// # }
/// ```
// Future use: T078 (main.rs background operation runner)
#[allow(dead_code)]
pub struct ExecutionRunner<Q, R>
where
    Q: OperationQueue,
    R: ProgressReporter,
{
    config: AgentConfig,
    queue: Arc<Q>,
    progress_reporter: Arc<R>,
    registry: Arc<crate::state::ProductRegistry>,
    metrics: crate::observability::metrics::Metrics,
    factory: ExecutorFactory,
    shutdown_token: CancellationToken,
    semaphore: Arc<Semaphore>,
}

// Future use: T078 (main.rs background operation runner)
#[allow(dead_code)]
impl<Q, R> ExecutionRunner<Q, R>
where
    Q: OperationQueue + 'static,
    R: ProgressReporter + 'static,
{
    /// Create a new execution runner
    ///
    /// # Arguments
    ///
    /// - `config`: Agent configuration
    /// - `queue`: Operation queue implementation
    /// - `progress_reporter`: Progress reporting implementation
    /// - `registry`: Product registry for state updates
    /// - `metrics`: Metrics collection for observability
    /// - `shutdown_token`: Token for graceful shutdown
    pub fn new(
        config: AgentConfig,
        queue: Arc<Q>,
        progress_reporter: Arc<R>,
        registry: Arc<crate::state::ProductRegistry>,
        metrics: crate::observability::metrics::Metrics,
        shutdown_token: CancellationToken,
    ) -> Self {
        let max_concurrent = config.operations.max_concurrent;
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        Self {
            config,
            queue,
            progress_reporter,
            registry,
            metrics: metrics.clone(),
            factory: ExecutorFactory::new(),
            shutdown_token,
            semaphore,
        }
    }

    /// Run the execution loop
    ///
    /// Continuously polls the operation queue and executes operations until
    /// shutdown is requested.
    ///
    /// # Returns
    ///
    /// - `Ok(())`: Shutdown completed gracefully
    /// - `Err(AgentError)`: Fatal error occurred
    pub async fn run(self) -> Result<()> {
        tracing::info!("Execution runner started");

        let mut poll_interval = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    // Poll for operations
                    if let Err(e) = self.poll_and_execute().await {
                        tracing::error!(error = %e, "Error polling operations");
                    }
                }
                () = self.shutdown_token.cancelled() => {
                    tracing::info!("Shutdown requested, stopping execution runner");
                    break;
                }
            }
        }

        // Wait for all in-progress operations to complete
        tracing::info!("Waiting for in-progress operations to complete");
        let max_concurrent = self.config.operations.max_concurrent;
        for _ in 0..max_concurrent {
            let _ = self.semaphore.acquire().await;
        }

        tracing::info!("Execution runner stopped");
        Ok(())
    }

    /// Poll queue and execute available operations
    async fn poll_and_execute(&self) -> Result<()> {
        // Try to acquire semaphore permit (non-blocking)
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            // Check for queued operations
            if let Some(operation) = self.queue.dequeue().await {
                tracing::debug!(
                    operation_id = %operation.operation_id,
                    operation_type = ?operation.operation_type,
                    "Dequeued operation for execution"
                );

                // Spawn execution task
                let queue = self.queue.clone();
                let progress_reporter = self.progress_reporter.clone();
                let registry = self.registry.clone();
                let metrics = self.metrics.clone();
                let factory = self.factory.clone();
                let config = self.config.clone();
                let shutdown_token = self.shutdown_token.clone();

                tokio::spawn(async move {
                    let result = Self::execute_operation(
                        operation,
                        config,
                        queue.clone(),
                        progress_reporter,
                        registry,
                        metrics,
                        factory,
                        shutdown_token,
                    )
                    .await;

                    // Release permit when done
                    drop(permit);

                    if let Err(e) = result {
                        tracing::error!(error = %e, "Operation execution failed");
                    }
                });
            }
        }

        Ok(())
    }

    /// Execute a single operation
    async fn execute_operation(
        mut operation: Operation,
        config: AgentConfig,
        queue: Arc<Q>,
        progress_reporter: Arc<R>,
        registry: Arc<crate::state::ProductRegistry>,
        metrics: crate::observability::metrics::Metrics,
        factory: ExecutorFactory,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        tracing::info!(
            operation_id = %operation.operation_id,
            operation_type = ?operation.operation_type,
            product_code = %operation.product_code,
            "Starting operation execution"
        );

        // Create executor for operation type
        let executor = factory.create(operation.operation_type)?;

        // Create execution context with operation-specific cancellation token
        let operation_token = shutdown_token.child_token();
        let context = ExecutionContext::new(
            config,
            progress_reporter,
            registry,
            metrics,
            operation_token,
        );

        // Execute operation
        let final_state = executor.execute(&mut operation, &context).await?;

        tracing::info!(
            operation_id = %operation.operation_id,
            final_state = ?final_state,
            "Operation execution completed"
        );

        // Update queue with final state
        queue.complete(operation.operation_id, final_state).await;

        Ok(())
    }
}

/// Trait for operation queue implementations
///
/// Provides interface for dequeuing operations and updating their state.
#[async_trait::async_trait]
pub trait OperationQueue: Send + Sync + 'static {
    // Future use: T078 (main.rs operation runner)
    #[allow(dead_code)]
    /// Dequeue the next operation ready for execution
    ///
    /// # Returns
    ///
    /// - `Some(Operation)`: Next operation to execute
    /// - `None`: No operations ready
    async fn dequeue(&self) -> Option<Operation>;

    // Future use: T078 (main.rs operation runner)
    #[allow(dead_code)]
    /// Mark operation as complete with final state
    ///
    /// # Arguments
    ///
    /// - `operation_id`: UUID of completed operation
    /// - `final_state`: Final operation state
    async fn complete(&self, operation_id: uuid::Uuid, final_state: OperationState);

    // Future use: T074-T077 (resume support)
    #[allow(dead_code)]
    /// Resume interrupted operations on startup
    ///
    /// # Returns
    ///
    /// List of operations that were in progress when service stopped
    async fn resume_interrupted(&self) -> Vec<Operation>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{OperationType, Priority, Progress};
    use crate::observability::metrics::Metrics;
    use crate::state::{Database, ProductRegistry};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockQueue {
        operations: Arc<Mutex<Vec<Operation>>>,
        completed: Arc<Mutex<Vec<(uuid::Uuid, OperationState)>>>,
    }

    impl MockQueue {
        fn new() -> Self {
            Self {
                operations: Arc::new(Mutex::new(Vec::new())),
                completed: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn enqueue(&self, operation: Operation) {
            self.operations
                .lock()
                .expect("Failed to acquire lock")
                .push(operation);
        }
    }

    #[async_trait::async_trait]
    impl OperationQueue for MockQueue {
        async fn dequeue(&self) -> Option<Operation> {
            self.operations
                .lock()
                .expect("Failed to acquire lock")
                .pop()
        }

        async fn complete(&self, operation_id: uuid::Uuid, final_state: OperationState) {
            self.completed
                .lock()
                .expect("Operation should succeed")
                .push((operation_id, final_state));
        }

        async fn resume_interrupted(&self) -> Vec<Operation> {
            Vec::new()
        }
    }

    struct MockProgressReporter;

    impl ProgressReporter for MockProgressReporter {
        fn report_progress(&self, _operation_id: uuid::Uuid, _progress: Progress) {}

        fn report_state_change(&self, _operation_id: uuid::Uuid, _new_state: OperationState) {}

        fn report_error(
            &self,
            _operation_id: uuid::Uuid,
            _error_code: String,
            _error_message: String,
            _details: Option<serde_json::Value>,
        ) {
        }
    }

    #[test]
    fn test_runner_new() {
        let config = AgentConfig::default();
        let queue = Arc::new(MockQueue::new());
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let reporter = Arc::new(MockProgressReporter);
        let shutdown_token = CancellationToken::new();

        let _runner = ExecutionRunner::new(
            config,
            queue,
            reporter,
            registry.clone(),
            metrics.clone(),
            shutdown_token,
        );
    }

    #[tokio::test]
    async fn test_runner_shutdown() {
        let config = AgentConfig::default();
        let queue = Arc::new(MockQueue::new());
        let reporter = Arc::new(MockProgressReporter);
        let shutdown_token = CancellationToken::new();

        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let runner = ExecutionRunner::new(
            config,
            queue,
            reporter,
            registry.clone(),
            metrics.clone(),
            shutdown_token.clone(),
        );

        let handle = tokio::spawn(async move { runner.run().await });

        // Immediately shutdown
        shutdown_token.cancel();

        let result = handle.await.expect("Task should complete");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_poll_and_execute() {
        let config = AgentConfig::default();
        let queue = Arc::new(MockQueue::new());
        let reporter = Arc::new(MockProgressReporter);
        let shutdown_token = CancellationToken::new();

        // Initialize database, registry, and metrics
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();

        // Enqueue an operation (will fail due to missing parameters, but tests the flow)
        let operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
        queue.enqueue(operation);

        let runner = ExecutionRunner::new(
            config,
            queue.clone(),
            reporter,
            registry,
            metrics,
            shutdown_token.clone(),
        );

        // Poll once
        let result = runner.poll_and_execute().await;
        assert!(result.is_ok());

        // Give spawned task time to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check that operation was completed (in Failed state due to missing params)
        let completed = queue.completed.lock().expect("Failed to acquire lock");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].1, OperationState::Failed);
    }
}
