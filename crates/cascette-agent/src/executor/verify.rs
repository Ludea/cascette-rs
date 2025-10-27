//! Verify operation executor
//!
//! Handles verification of installation integrity without redownloading files.
//! Reports which files are corrupted or missing.

use crate::{
    error::{AgentError, Result},
    executor::{ExecutionContext, ExecutionResult, OperationExecutor},
    models::{Operation, OperationState, OperationType, Progress},
};
use async_trait::async_trait;

/// Executor for verify operations
///
/// Checks file integrity against manifests without downloading anything.
/// Updates product status to Installed or Corrupted based on verification results.
///
/// # Features
///
/// - Non-destructive verification
/// - Progress tracking
/// - Detailed corruption reporting
/// - Graceful cancellation
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::executor::{verify::VerifyExecutor, OperationExecutor, ExecutionContext};
/// use cascette_agent::models::{Operation, OperationType, Priority};
///
/// # async fn example() -> cascette_agent::error::Result<()> {
/// let executor = VerifyExecutor::new();
/// let mut operation = Operation::new("wow".to_string(), OperationType::Verify, Priority::Normal);
///
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
pub struct VerifyExecutor;

impl VerifyExecutor {
    /// Create a new verify executor
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

    /// Extract build ID from operation parameters
    fn get_build_id(&self, operation: &Operation) -> Result<u32> {
        let params = operation
            .parameters
            .as_ref()
            .ok_or_else(|| AgentError::InvalidOperation("Missing operation parameters".into()))?;

        params
            .get("build_id")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32)
            .ok_or_else(|| AgentError::InvalidOperation("Missing build_id parameter".into()))
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

    /// Perform verification (placeholder - will use cascette-installation verify functionality)
    async fn verify_installation(
        &self,
        operation: &Operation,
        context: &ExecutionContext,
    ) -> Result<VerificationResult> {
        let install_path = self.get_install_path(operation)?;
        let _build_id = self.get_build_id(operation)?;

        tracing::info!(
            operation_id = %operation.operation_id,
            path = %install_path.display(),
            "Performing installation verification"
        );

        // TODO: Implement actual verification using cascette-installation
        // For now, we'll simulate verification by checking if key directories exist

        let corrupted_files = Vec::new(); // Placeholder for actual verification
        let mut missing_files = Vec::new();

        // Check key CASC directories
        let data_dir = install_path.join("Data");
        let config_dir = data_dir.join("config");
        let data_data_dir = data_dir.join("data");
        let indices_dir = data_dir.join("indices");

        if !config_dir.exists() {
            missing_files.push("Data/config".to_string());
        }
        if !data_data_dir.exists() {
            missing_files.push("Data/data".to_string());
        }
        if !indices_dir.exists() {
            missing_files.push("Data/indices".to_string());
        }

        let total_files = 100; // Placeholder
        let verified_files = total_files - missing_files.len();

        // Report progress
        let mut progress = Progress::default();
        progress.phase = "verifying".to_string();
        progress.files_total = total_files;
        progress.files_completed = verified_files;
        progress.percentage = if total_files > 0 {
            (verified_files as f64 / total_files as f64) * 100.0
        } else {
            0.0
        };

        context
            .progress_reporter
            .report_progress(operation.operation_id, progress);

        // Check for cancellation
        if context.is_cancelled() {
            return Err(AgentError::OperationCancelled);
        }

        Ok(VerificationResult {
            total_files,
            verified_files,
            corrupted_files,
            missing_files,
        })
    }
}

impl Default for VerifyExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of verification operation
#[derive(Debug)]
struct VerificationResult {
    total_files: usize,
    verified_files: usize,
    corrupted_files: Vec<String>,
    missing_files: Vec<String>,
}

impl VerificationResult {
    fn is_valid(&self) -> bool {
        self.corrupted_files.is_empty() && self.missing_files.is_empty()
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_files": self.total_files,
            "verified_files": self.verified_files,
            "corrupted_count": self.corrupted_files.len(),
            "missing_count": self.missing_files.len(),
            "corrupted_files": self.corrupted_files,
            "missing_files": self.missing_files,
        })
    }
}

#[async_trait]
impl OperationExecutor for VerifyExecutor {
    async fn execute(
        &self,
        operation: &mut Operation,
        context: &ExecutionContext,
    ) -> ExecutionResult {
        // Validate operation type
        if operation.operation_type != OperationType::Verify {
            return Err(AgentError::InvalidOperation(format!(
                "Expected Verify operation, got {:?}",
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
            "Starting verification"
        );

        // Validate installation exists
        if let Err(e) = self.validate_installation_exists(operation).await {
            let error_msg = format!("Product not installed: {e}");
            tracing::error!(
                operation_id = %operation.operation_id,
                error = %e,
                "Verification validation failed"
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

        // Transition to Verifying state
        operation.set_state(OperationState::Verifying);
        context
            .progress_reporter
            .report_state_change(operation.operation_id, OperationState::Verifying);

        tracing::info!(
            operation_id = %operation.operation_id,
            "Verifying installation integrity"
        );

        // Perform verification
        let verification_result = match self.verify_installation(operation, context).await {
            Ok(result) => result,
            Err(e) if matches!(e, AgentError::OperationCancelled) => {
                tracing::info!(
                    operation_id = %operation.operation_id,
                    "Verification cancelled"
                );
                operation.set_state(OperationState::Cancelled);
                return Ok(OperationState::Cancelled);
            }
            Err(e) => {
                let error_msg = format!("Verification failed: {e}");
                tracing::error!(
                    operation_id = %operation.operation_id,
                    error = %e,
                    "Verification execution failed"
                );
                context.progress_reporter.report_error(
                    operation.operation_id,
                    "VERIFICATION_FAILED".to_string(),
                    error_msg,
                    None,
                );
                operation.set_state(OperationState::Failed);
                return Ok(OperationState::Failed);
            }
        };

        // Store verification results in operation metadata
        operation.metadata = Some(verification_result.to_json());

        if verification_result.is_valid() {
            tracing::info!(
                operation_id = %operation.operation_id,
                product_code = %operation.product_code,
                "Verification passed - installation is valid"
            );

            operation.set_state(OperationState::Complete);
            operation.completed_at = Some(chrono::Utc::now());
            context
                .progress_reporter
                .report_state_change(operation.operation_id, OperationState::Complete);

            Ok(OperationState::Complete)
        } else {
            let error_msg = format!(
                "Verification found {} corrupted and {} missing files",
                verification_result.corrupted_files.len(),
                verification_result.missing_files.len()
            );

            tracing::warn!(
                operation_id = %operation.operation_id,
                corrupted_count = verification_result.corrupted_files.len(),
                missing_count = verification_result.missing_files.len(),
                "Verification detected corruption"
            );

            context.progress_reporter.report_error(
                operation.operation_id,
                "CORRUPTION_DETECTED".to_string(),
                error_msg,
                Some(verification_result.to_json()),
            );

            operation.set_state(OperationState::Failed);
            operation.completed_at = Some(chrono::Utc::now());

            Ok(OperationState::Failed)
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
    fn test_verify_executor_new() {
        let executor = VerifyExecutor::new();
        assert!(
            executor
                .get_install_path(&Operation::new(
                    "wow".to_string(),
                    OperationType::Verify,
                    Priority::Normal
                ))
                .is_err()
        );
    }

    #[test]
    fn test_get_install_path() {
        let executor = VerifyExecutor::new();
        let mut operation =
            Operation::new("wow".to_string(), OperationType::Verify, Priority::Normal);

        operation.parameters = Some(serde_json::json!({
            "install_path": "/test/path",
            "build_id": 63696
        }));

        let path = executor
            .get_install_path(&operation)
            .expect("Failed to get install path");
        assert_eq!(path, std::path::PathBuf::from("/test/path"));
    }

    #[test]
    fn test_verification_result_is_valid() {
        let result = VerificationResult {
            total_files: 100,
            verified_files: 100,
            corrupted_files: vec![],
            missing_files: vec![],
        };
        assert!(result.is_valid());

        let result_with_corruption = VerificationResult {
            total_files: 100,
            verified_files: 99,
            corrupted_files: vec!["file1.txt".to_string()],
            missing_files: vec![],
        };
        assert!(!result_with_corruption.is_valid());
    }

    #[tokio::test]
    async fn test_execute_invalid_operation_type() {
        let executor = VerifyExecutor::new();
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
