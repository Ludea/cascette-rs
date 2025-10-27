//! Uninstall operation executor
//!
//! Handles removal of installed products to free disk space.
//! Supports configuration preservation for reinstallation.

use crate::{
    error::{AgentError, Result},
    executor::{ExecutionContext, ExecutionResult, OperationExecutor},
    models::{Operation, OperationState, OperationType, Progress},
};
use async_trait::async_trait;

/// Executor for uninstall operations
///
/// Removes product installation files and frees disk space.
/// Optionally preserves configuration files for future reinstallation.
///
/// # Features
///
/// - Complete product removal
/// - Optional configuration preservation
/// - Disk space tracking
/// - Graceful cancellation
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::executor::{uninstall::UninstallExecutor, OperationExecutor, ExecutionContext};
/// use cascette_agent::models::{Operation, OperationType, Priority};
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// let executor = UninstallExecutor::new();
/// let mut operation = Operation::new("wow".to_string(), OperationType::Uninstall, Priority::Normal);
///
/// operation.parameters = Some(serde_json::json!({
///     "install_path": "/games/wow",
///     "keep_config": false
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
pub struct UninstallExecutor;

impl UninstallExecutor {
    /// Create a new uninstall executor
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

    /// Check if configuration should be preserved
    fn should_keep_config(&self, operation: &Operation) -> bool {
        operation
            .parameters
            .as_ref()
            .and_then(|p| p.get("keep_config"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }

    /// Validate that product is installed
    async fn validate_installation_exists(&self, operation: &Operation) -> Result<()> {
        let install_path = self.get_install_path(operation)?;

        if !install_path.exists() {
            return Err(AgentError::ProductNotInstalled(
                operation.product_code.clone(),
            ));
        }

        Ok(())
    }

    /// Calculate directory size recursively
    fn calculate_directory_size<'a>(
        &'a self,
        path: &'a std::path::Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + 'a>> {
        Box::pin(async move {
            let mut total_size = 0u64;

            let mut read_dir = tokio::fs::read_dir(path).await.map_err(|e| {
                AgentError::IoError(format!(
                    "Failed to read directory {}: {}",
                    path.display(),
                    e
                ))
            })?;

            while let Some(entry) = read_dir
                .next_entry()
                .await
                .map_err(|e| AgentError::IoError(format!("Failed to read directory entry: {e}")))?
            {
                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|e| AgentError::IoError(format!("Failed to get metadata: {e}")))?;

                if metadata.is_file() {
                    total_size += metadata.len();
                } else if metadata.is_dir() {
                    total_size += self.calculate_directory_size(&entry.path()).await?;
                }
            }

            Ok(total_size)
        })
    }

    /// Remove installation directory
    async fn remove_installation(
        &self,
        operation: &Operation,
        context: &ExecutionContext,
    ) -> Result<UninstallResult> {
        let install_path = self.get_install_path(operation)?;
        let keep_config = self.should_keep_config(operation);

        tracing::info!(
            operation_id = %operation.operation_id,
            path = %install_path.display(),
            keep_config = keep_config,
            "Removing installation"
        );

        // Calculate size before removal
        let freed_bytes = self
            .calculate_directory_size(&install_path)
            .await
            .unwrap_or(0);

        tracing::debug!(
            operation_id = %operation.operation_id,
            size_mb = freed_bytes / (1024 * 1024),
            "Calculated installation size"
        );

        if keep_config {
            // Preserve configuration by moving it temporarily
            let config_backup = install_path
                .parent()
                .ok_or_else(|| AgentError::InvalidOperation("Invalid installation path".into()))?
                .join(format!(".{}_config_backup", operation.product_code));

            tracing::debug!(
                operation_id = %operation.operation_id,
                backup_path = %config_backup.display(),
                "Preserving configuration"
            );

            // Identify configuration directories
            let config_dirs = vec![
                install_path.join("WTF"),       // WoW config/addons
                install_path.join("Interface"), // WoW interface customizations
            ];

            // Backup configuration if it exists
            for config_dir in &config_dirs {
                if config_dir.exists() {
                    let dest = config_backup
                        .join(config_dir.file_name().expect("Path should have filename"));
                    if let Err(e) = tokio::fs::rename(config_dir, &dest).await {
                        tracing::warn!(
                            operation_id = %operation.operation_id,
                            error = %e,
                            "Failed to backup configuration directory"
                        );
                    }
                }
            }

            // Remove main installation
            if let Err(e) = tokio::fs::remove_dir_all(&install_path).await {
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Failed to remove installation directory"
                );
                return Err(AgentError::IoError(format!(
                    "Failed to remove installation: {e}"
                )));
            }

            // Restore configuration to parent directory
            if config_backup.exists() {
                tokio::fs::create_dir_all(&install_path)
                    .await
                    .map_err(|e| {
                        AgentError::IoError(format!("Failed to recreate config directory: {e}"))
                    })?;

                let mut read_dir = tokio::fs::read_dir(&config_backup).await.map_err(|e| {
                    AgentError::IoError(format!("Failed to read config backup: {e}"))
                })?;

                while let Some(entry) = read_dir
                    .next_entry()
                    .await
                    .map_err(|e| AgentError::IoError(format!("Failed to read backup entry: {e}")))?
                {
                    let dest = install_path.join(entry.file_name());
                    if let Err(e) = tokio::fs::rename(entry.path(), &dest).await {
                        tracing::warn!(
                            operation_id = %operation.operation_id,
                            error = %e,
                            "Failed to restore configuration"
                        );
                    }
                }

                // Remove backup directory
                let _ = tokio::fs::remove_dir_all(&config_backup).await;
            }
        } else {
            // Remove entire installation directory
            if let Err(e) = tokio::fs::remove_dir_all(&install_path).await {
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Failed to remove installation directory"
                );
                return Err(AgentError::IoError(format!(
                    "Failed to remove installation: {e}"
                )));
            }
        }

        // Check for cancellation
        if context.is_cancelled() {
            return Err(AgentError::OperationCancelled);
        }

        Ok(UninstallResult {
            freed_bytes,
            config_preserved: keep_config,
        })
    }
}

impl Default for UninstallExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of uninstall operation
#[derive(Debug)]
struct UninstallResult {
    freed_bytes: u64,
    config_preserved: bool,
}

impl UninstallResult {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "freed_bytes": self.freed_bytes,
            "freed_mb": self.freed_bytes / (1024 * 1024),
            "config_preserved": self.config_preserved,
        })
    }
}

#[async_trait]
impl OperationExecutor for UninstallExecutor {
    async fn execute(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult {
        // Validate operation type
        if operation.operation_type != OperationType::Uninstall {
            return Err(AgentError::InvalidOperation(format!(
                "Expected Uninstall operation, got {:?}",
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
            "Starting uninstall"
        );

        // Validate installation exists
        if let Err(e) = self.validate_installation_exists(operation).await {
            let error_msg = format!("Product not installed: {e}");
            tracing::error!(
                operation_id = %operation.operation_id,
                error = %e,
                "Uninstall validation failed"
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

        // Transition to processing (uninstall doesn't have downloading/verifying phases)
        operation.set_state(OperationState::Downloading); // Reusing state for "processing"
        context
            .progress_reporter
            .report_state_change(operation.operation_id, OperationState::Downloading);

        // Report initial progress
        let mut progress = Progress::default();
        progress.phase = "uninstalling".to_string();
        progress.bytes_total = 1;
        progress.bytes_downloaded = 0;
        progress.percentage = 0.0;
        context
            .progress_reporter
            .report_progress(operation.operation_id, progress.clone());

        // Perform uninstall
        let uninstall_result = match self.remove_installation(operation, context).await {
            Ok(result) => result,
            Err(e) if matches!(e, AgentError::OperationCancelled) => {
                tracing::info!(
                    operation_id = %operation.operation_id,
                    "Uninstall cancelled"
                );
                operation.set_state(OperationState::Cancelled);
                return Ok(OperationState::Cancelled);
            }
            Err(e) => {
                let error_msg = format!("Uninstall failed: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Uninstall execution failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "UNINSTALL_FAILED".to_string(),
                    error_msg,
                    None,
                );
                operation.set_state(OperationState::Failed);
                return Ok(OperationState::Failed);
            }
        };

        // Report completion progress
        progress.bytes_downloaded = 1;
        progress.percentage = 100.0;
        context
            .progress_reporter
            .report_progress(operation.operation_id, progress);

        // Store uninstall results
        operation.metadata = Some(uninstall_result.to_json());

        tracing::info!(
            operation_id = %operation.operation_id,
            product_code = %operation.product_code,
            freed_mb = uninstall_result.freed_bytes / (1024 * 1024),
            "Uninstall completed successfully"
        );

        operation.set_state(OperationState::Complete);
        operation.completed_at = Some(chrono::Utc::now());
        context
            .progress_reporter
            .report_state_change(operation.operation_id, OperationState::Complete);

        Ok(OperationState::Complete)
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
    use std::sync::Arc;
    use std::sync::Mutex;
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
    fn test_uninstall_executor_new() {
        let executor = UninstallExecutor::new();
        assert!(
            executor
                .get_install_path(&Operation::new(
                    "wow".to_string(),
                    OperationType::Uninstall,
                    Priority::Normal
                ))
                .is_err()
        );
    }

    #[test]
    fn test_get_install_path() {
        let executor = UninstallExecutor::new();
        let mut operation = Operation::new(
            "wow".to_string(),
            OperationType::Uninstall,
            Priority::Normal,
        );

        operation.parameters = Some(serde_json::json!({
            "install_path": "/test/path"
        }));

        let path = executor
            .get_install_path(&operation)
            .expect("Failed to get install path");
        assert_eq!(path, std::path::PathBuf::from("/test/path"));
    }

    #[test]
    fn test_should_keep_config() {
        let executor = UninstallExecutor::new();
        let mut operation = Operation::new(
            "wow".to_string(),
            OperationType::Uninstall,
            Priority::Normal,
        );

        operation.parameters = Some(serde_json::json!({
            "install_path": "/test/path",
            "keep_config": true
        }));

        assert!(executor.should_keep_config(&operation));

        operation.parameters = Some(serde_json::json!({
            "install_path": "/test/path",
            "keep_config": false
        }));

        assert!(!executor.should_keep_config(&operation));
    }

    #[tokio::test]
    async fn test_execute_invalid_operation_type() {
        let executor = UninstallExecutor::new();
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
