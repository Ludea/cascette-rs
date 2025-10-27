//! Update operation executor
//!
//! Handles updating existing products to newer versions with delta downloads.
//! Validates version progression and rejects downgrades per FR-032.

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

/// Executor for update operations
///
/// Updates an existing product installation to a newer version. Performs delta
/// downloads where possible to minimize bandwidth usage.
///
/// # Features
///
/// - Version validation (prevents downgrades per FR-032)
/// - Delta download optimization
/// - Progress tracking
/// - Graceful cancellation
/// - Rollback safety (keeps original on failure)
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::executor::{update::UpdateExecutor, OperationExecutor, ExecutionContext};
/// use cascette_agent::models::{Operation, OperationType, Priority};
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// let executor = UpdateExecutor::new();
/// let mut operation = Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);
///
/// operation.parameters = Some(serde_json::json!({
///     "install_path": "/games/wow",
///     "current_version": "1.14.2.42597",
///     "target_version": "1.15.7.63696"
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
pub struct UpdateExecutor;

impl UpdateExecutor {
    /// Create a new update executor
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

    /// Extract target build ID from operation parameters
    fn get_target_build_id(&self, operation: &Operation) -> Result<u32> {
        let params = operation
            .parameters
            .as_ref()
            .ok_or_else(|| AgentError::InvalidOperation("Missing operation parameters".into()))?;

        params
            .get("target_build_id")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32)
            .ok_or_else(|| AgentError::InvalidOperation("Missing target_build_id parameter".into()))
    }

    /// Extract current build ID from operation parameters
    fn get_current_build_id(&self, operation: &Operation) -> Result<u32> {
        let params = operation
            .parameters
            .as_ref()
            .ok_or_else(|| AgentError::InvalidOperation("Missing operation parameters".into()))?;

        params
            .get("current_build_id")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32)
            .ok_or_else(|| {
                AgentError::InvalidOperation("Missing current_build_id parameter".into())
            })
    }

    /// Validate that target version is newer than current (FR-032)
    fn validate_version_progression(&self, operation: &Operation) -> Result<()> {
        let current_build = self.get_current_build_id(operation)?;
        let target_build = self.get_target_build_id(operation)?;

        if target_build <= current_build {
            return Err(AgentError::DowngradeRejected {
                current_version: current_build.to_string(),
                requested_version: target_build.to_string(),
            });
        }

        Ok(())
    }

    /// Validate that product is already installed
    async fn validate_installation_exists(&self, operation: &Operation) -> Result<()> {
        let install_path = self.get_install_path(operation)?;

        if !install_path.exists() {
            return Err(AgentError::ProductNotInstalled(
                operation.product_code.clone(),
            ));
        }

        // Check for CASC structure markers
        let data_dir = install_path.join("Data");
        if !data_dir.exists() {
            return Err(AgentError::InvalidOperation(
                "Installation directory exists but is not a valid CASC installation".into(),
            ));
        }

        Ok(())
    }

    /// Update product version in registry on successful update (T099)
    ///
    /// Updates the Product.version field with the target build ID from operation parameters.
    /// Logs error if update fails but doesn't fail the operation since the actual update succeeded.
    async fn update_product_version(
        &self,
        operation: &Operation,
        context: &ExecutionContext,
    ) -> Result<()> {
        let target_build_id = self.get_target_build_id(operation)?;
        let target_version = target_build_id.to_string();

        tracing::debug!(
            operation_id = %operation.operation_id,
            product_code = %operation.product_code,
            target_version = %target_version,
            "Updating product version in registry"
        );

        // Get current product from registry
        let mut product = context.registry.get(&operation.product_code)?;

        // Update version field
        product.version = Some(target_version.clone());
        product.updated_at = chrono::Utc::now();

        // Clear update available flags since we just updated
        product.is_update_available = Some(false);
        product.available_version = None;

        // Save updated product
        match context.registry.update(&product) {
            Ok(()) => {
                tracing::info!(
                    operation_id = %operation.operation_id,
                    product_code = %operation.product_code,
                    new_version = %target_version,
                    "Successfully updated product version in registry"
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    operation_id = %operation.operation_id,
                    product_code = %operation.product_code,
                    error = %e,
                    "Failed to update product version in registry"
                );
                Err(e)
            }
        }
    }

    /// Create installation request for update
    fn create_request(&self, operation: &Operation) -> Result<InstallationRequest> {
        let install_path = self.get_install_path(operation)?;
        let target_build_id = self.get_target_build_id(operation)?;

        Ok(InstallationRequest {
            product_code: operation.product_code.clone(),
            build_id: Some(target_build_id),
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
}

impl Default for UpdateExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OperationExecutor for UpdateExecutor {
    async fn execute(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult {
        // Validate operation type
        if operation.operation_type != OperationType::Update {
            return Err(AgentError::InvalidOperation(format!(
                "Expected Update operation, got {:?}",
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
            "Starting update"
        );

        // Validate installation exists
        if let Err(e) = self.validate_installation_exists(operation).await {
            let error_msg = format!("Product not installed: {e}");
            tracing::error!(
                operation_id = %operation.operation_id,
                error = %e,
                "Update validation failed"
            );
            context.progress_reporter.report_error(
                operation.operation_id,
                "NOT_INSTALLED".to_string(),
                error_msg,
                None,
            );
            operation.set_state(OperationState::Failed);
            return Ok(OperationState::Failed);
        }

        // Validate version progression (FR-032)
        if let Err(e) = self.validate_version_progression(operation) {
            let error_msg = format!("Version downgrade rejected: {e}");
            tracing::warn!(
                operation_id = %operation.operation_id,
                error = %e,
                "Version downgrade rejected"
            );
            context.progress_reporter.report_error(
                operation.operation_id,
                "DOWNGRADE_REJECTED".to_string(),
                error_msg,
                Some(serde_json::json!({
                    "current_build": self.get_current_build_id(operation).ok(),
                    "target_build": self.get_target_build_id(operation).ok()
                })),
            );
            operation.set_state(OperationState::Failed);
            return Ok(OperationState::Failed);
        }

        // Create installation request
        let request = match self.create_request(operation) {
            Ok(req) => req,
            Err(e) => {
                let error_msg = format!("Failed to create update request: {e}");
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
            "Building update plan"
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
                let error_msg = format!("Failed to build update plan: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Update plan creation failed"
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

        // Execute update
        tracing::info!(
            operation_id = %operation.operation_id,
            "Executing update plan"
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
                    "Update cancelled"
                );
                // Cancel progress reporter
                progress_reporter_task.abort();
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
                    "Verifying update"
                );

                // T099: Update product version in registry after successful update
                if let Err(e) = self.update_product_version(operation, context).await {
                    tracing::error!(
                        operation_id = %operation.operation_id,
                        error = %e,
                        "Failed to update product version in registry (non-fatal)"
                    );
                    // Do not fail the operation since the actual update succeeded
                    // The registry update is a bookkeeping operation
                }

                // Transition to Complete state
                operation.set_state(OperationState::Complete);
                operation.completed_at = Some(chrono::Utc::now());
                context
                    .progress_reporter
                    .report_state_change(operation.operation_id, OperationState::Complete);

                tracing::info!(
                    operation_id = %operation.operation_id,
                    product_code = %operation.product_code,
                    "Update completed successfully"
                );

                Ok(OperationState::Complete)
            }
            Err(e) => {
                let error_msg = format!("Update failed: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Update execution failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "UPDATE_FAILED".to_string(),
                    error_msg,
                    None,
                );
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
    fn test_validate_version_progression_valid() {
        let executor = UpdateExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "current_build_id": 42597,
            "target_build_id": 63696
        }));

        assert!(executor.validate_version_progression(&operation).is_ok());
    }

    #[test]
    fn test_validate_version_progression_downgrade() {
        let executor = UpdateExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "current_build_id": 63696,
            "target_build_id": 42597
        }));

        let result = executor.validate_version_progression(&operation);
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("Should be error"),
            AgentError::DowngradeRejected { .. }
        ));
    }

    #[test]
    fn test_validate_version_progression_same_version() {
        let executor = UpdateExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Update, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "current_build_id": 63696,
            "target_build_id": 63696
        }));

        let result = executor.validate_version_progression(&operation);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_invalid_operation_type() {
        let executor = UpdateExecutor::new();
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
        assert!(result.is_err());
    }
}
