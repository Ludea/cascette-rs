//! Install operation executor
//!
//! Handles installation of new products using the cascette-installation library.
//! Provides progress tracking, cancellation support, and error handling.

use crate::{
    error::{AgentError, Result},
    executor::{
        ExecutionContext, ExecutionResult, OperationExecutor, progress_adapter::ProgressAdapter,
    },
    models::{Operation, OperationState, OperationType, Progress},
};
use async_trait::async_trait;
use cascette_installation::{
    executor::{InstallationMode, PlanExecutor},
    models::{CacheConfig, InstallationRequest, RetryConfig},
    plan_ngdp::NgdpPlanBuilder,
};
use std::sync::Arc;
use std::sync::Mutex;

/// Executor for install operations
///
/// Downloads and installs a new product using the cascette-installation library.
/// Creates a Battle.net-compatible CASC structure with all required metadata.
///
/// # Features
///
/// - Progress tracking with regular updates
/// - Graceful cancellation support
/// - Automatic cleanup on failure
/// - Resume capability through cascette-installation
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::executor::{install::InstallExecutor, OperationExecutor, ExecutionContext};
/// use cascette_agent::models::{Operation, OperationType, Priority};
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// let executor = InstallExecutor::new();
/// let mut operation = Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);
///
/// // Set required installation parameters
/// operation.parameters = Some(serde_json::json!({
///     "install_path": "/games/wow",
///     "build_id": 63696
/// }));
///
/// # let config = cascette_agent::config::AgentConfig::default();
/// # let context = ExecutionContext::new(
/// #     config,
/// #     std::sync::Arc::new(MockProgressReporter),
/// #     tokio_util::sync::CancellationToken::new(),
/// # );
/// # struct MockProgressReporter;
/// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
/// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
/// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
/// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
/// # }
/// let final_state = executor.execute(&mut operation, &context).await?;
/// # Ok(())
/// # }
/// ```
pub struct InstallExecutor;

impl InstallExecutor {
    /// Create a new install executor
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Extract install path from operation parameters
    fn get_install_path(&self, operation: &Operation) -> Result<std::path::PathBuf> {
        let params = operation
            .parameters
            .as_ref()
            .ok_or_else(|| AgentError::InvalidOperation("Missing operation parameters".into()))?;

        let install_path = params
            .get("install_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::InvalidOperation("Missing install_path parameter".into()))?;

        Ok(std::path::PathBuf::from(install_path))
    }

    /// Extract optional build ID from operation parameters
    fn get_build_id(&self, operation: &Operation) -> Option<u32> {
        operation
            .parameters
            .as_ref()
            .and_then(|p| p.get("build_id"))
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32)
    }

    /// Create installation request from operation
    fn create_request(&self, operation: &Operation) -> Result<InstallationRequest> {
        let install_path = self.get_install_path(operation)?;
        let build_id = self.get_build_id(operation);

        Ok(InstallationRequest {
            product_code: operation.product_code.clone(),
            build_id,
            output_dir: install_path,
            plan_only: false,
            execute_plan: None,
            retry_config: RetryConfig {
                max_attempts: 3,
                initial_delay: std::time::Duration::from_secs(1),
                max_delay: std::time::Duration::from_secs(60),
                backoff_factor: 2.0,
                jitter: true,
            },
            cache_config: CacheConfig {
                enabled: true,
                directory: std::env::var("CASCETTE_CACHE_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::cache_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("cascette")
                    }),
                max_size: 1024 * 1024 * 1024, // 1GB in bytes
                retention: std::time::Duration::from_secs(3600),
                eviction_policy: cascette_installation::models::EvictionPolicy::Lru,
            },
            max_concurrent: 4,
        })
    }

    /// Clean up partial installation on failure
    async fn cleanup_on_failure(&self, operation: &Operation) -> Result<()> {
        if let Ok(install_path) = self.get_install_path(operation) {
            // Only clean up if the directory was created by this operation
            // Check for .cascette marker to avoid deleting existing installations
            let marker_path = install_path.join(".cascette").join("in-progress");
            if marker_path.exists() {
                tracing::info!(
                    operation_id = %operation.operation_id,
                    path = %install_path.display(),
                    "Cleaning up partial installation"
                );

                // Remove the installation directory
                if let Err(e) = tokio::fs::remove_dir_all(&install_path).await {
                    tracing::warn!(
                        operation_id = %operation.operation_id,
                        path = %install_path.display(),
                        error = %e,
                        "Failed to clean up installation directory"
                    );
                }
            }
        }
        Ok(())
    }
}

impl Default for InstallExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationExecutor for InstallExecutor {
    async fn execute(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult {
        // Validate operation type
        if operation.operation_type != OperationType::Install {
            return Err(AgentError::InvalidOperation(format!(
                "Expected Install operation, got {:?}",
                operation.operation_type
            )));
        }

        // Transition to Initializing state
        operation.set_state(OperationState::Initializing);
        operation.started_at = Some(chrono::Utc::now());
        context
            .progress_reporter
            .report_state_change(operation.operation_id, OperationState::Initializing);

        tracing::info!(
            operation_id = %operation.operation_id,
            product_code = %operation.product_code,
            "Starting installation"
        );

        // Create installation request
        let request = match self.create_request(operation) {
            Ok(req) => req,
            Err(e) => {
                let error_msg = format!("Failed to create installation request: {e}");
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "INVALID_REQUEST".to_string(),
                    error_msg,
                    None,
                );
                operation.set_state(OperationState::Failed);
                return Ok(OperationState::Failed);
            }
        };

        // Create installation plan
        tracing::debug!(
            operation_id = %operation.operation_id,
            "Building installation plan"
        );

        let plan = match NgdpPlanBuilder::new(request.clone())
            .with_data_dir(
                std::env::var("CASCETTE_DATA_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::data_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("cascette")
                    }),
            )
            .build()
            .await
        {
            Ok(plan) => plan,
            Err(e) => {
                let error_msg = format!("Failed to build installation plan: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Installation plan creation failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "PLAN_FAILED".to_string(),
                    error_msg,
                    None,
                );
                operation.set_state(OperationState::Failed);
                return Ok(OperationState::Failed);
            }
        };

        // Transition to Downloading state
        operation.set_state(OperationState::Downloading);
        context
            .progress_reporter
            .report_state_change(operation.operation_id, OperationState::Downloading);

        // Create progress tracking state
        let progress_state = Arc::new(Mutex::new(Progress {
            phase: "downloading".to_string(),
            ..Default::default()
        }));
        let operation_id = operation.operation_id;
        let reporter = context.progress_reporter.clone();

        // Progress callback adapter for installation library
        let progress_callback = ProgressAdapter::new(progress_state.clone());

        // Spawn a background task to periodically report progress
        let progress_reporter_task = {
            let progress_state = progress_state.clone();
            let reporter = reporter.clone();
            let cancellation = context.cancellation_token.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let progress = progress_state.lock().expect("Mutex poisoned").clone();
                            reporter.report_progress(operation_id, progress);
                        }
                        () = cancellation.cancelled() => {
                            break;
                        }
                    }
                }
            })
        };

        // Execute installation plan
        tracing::info!(
            operation_id = %operation.operation_id,
            "Executing installation plan"
        );

        let mut executor = match PlanExecutor::new() {
            Ok(exec) => exec
                .with_installation_mode(InstallationMode::Battlenet)
                .with_progress_callback(Box::new(progress_callback)),
            Err(e) => {
                let error_msg = format!("Failed to create plan executor: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Plan executor creation failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "EXECUTOR_FAILED".to_string(),
                    error_msg,
                    None,
                );
                self.cleanup_on_failure(operation).await?;
                operation.set_state(OperationState::Failed);
                return Ok(OperationState::Failed);
            }
        };

        // Execute with cancellation support
        let execution_result = tokio::select! {
            result = executor.execute_plan(&plan) => result,
            () = context.cancellation_token.cancelled() => {
                tracing::info!(
                    operation_id = %operation.operation_id,
                    "Installation cancelled"
                );
                // Cancel progress reporter
                progress_reporter_task.abort();
                self.cleanup_on_failure(operation).await?;
                operation.set_state(OperationState::Cancelled);
                return Ok(OperationState::Cancelled);
            }
        };

        // Cancel progress reporter
        progress_reporter_task.abort();

        match execution_result {
            Ok(()) => {
                // Transition to Verifying state
                operation.set_state(OperationState::Verifying);
                context
                    .progress_reporter
                    .report_state_change(operation.operation_id, OperationState::Verifying);

                tracing::info!(
                    operation_id = %operation.operation_id,
                    "Verifying installation"
                );

                // Verification is done as part of installation
                // Transition to Complete state
                operation.set_state(OperationState::Complete);
                operation.completed_at = Some(chrono::Utc::now());
                context
                    .progress_reporter
                    .report_state_change(operation.operation_id, OperationState::Complete);

                tracing::info!(
                    operation_id = %operation.operation_id,
                    product_code = %operation.product_code,
                    "Installation completed successfully"
                );

                Ok(OperationState::Complete)
            }
            Err(e) => {
                let error_msg = format!("Installation failed: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Installation execution failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "INSTALLATION_FAILED".to_string(),
                    error_msg,
                    None,
                );
                self.cleanup_on_failure(operation).await?;
                operation.set_state(OperationState::Failed);
                Ok(OperationState::Failed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::AgentConfig,
        executor::ProgressReporter,
        models::{OperationState, Priority},
        observability::metrics::Metrics,
        state::{Database, ProductRegistry},
    };
    use tokio_util::sync::CancellationToken;

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
    fn test_install_executor_new() {
        let executor = InstallExecutor::new();
        assert!(
            executor
                .get_install_path(&Operation::new(
                    "wow".to_string(),
                    OperationType::Install,
                    Priority::Normal
                ))
                .is_err()
        );
    }

    #[test]
    fn test_get_install_path() {
        let executor = InstallExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "install_path": "/test/path"
        }));

        let path = executor
            .get_install_path(&operation)
            .expect("Failed to get install path");
        assert_eq!(path, std::path::PathBuf::from("/test/path"));
    }

    #[test]
    fn test_get_build_id() {
        let executor = InstallExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "build_id": 63696
        }));

        let build_id = executor.get_build_id(&operation);
        assert_eq!(build_id, Some(63696));
    }

    #[tokio::test]
    async fn test_execute_invalid_operation_type() {
        let executor = InstallExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);

        let config = AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        let result = executor.execute(&mut operation, &context).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_parameters() {
        let executor = InstallExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Install, Priority::Normal);

        let config = AgentConfig::default();
        let db = Database::in_memory().unwrap();
        let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
        let metrics = Metrics::new();
        let context = ExecutionContext::new(
            config,
            Arc::new(MockProgressReporter),
            registry,
            metrics,
            CancellationToken::new(),
        );

        let result = executor.execute(&mut operation, &context).await;
        assert_eq!(
            result.expect("Operation should succeed"),
            OperationState::Failed
        );
    }
}
