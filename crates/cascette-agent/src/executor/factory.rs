//! Executor factory for creating operation executors
//!
//! Provides a factory pattern for creating the appropriate executor based on
//! operation type with dependency injection support.

use crate::{
    error::Result,
    executor::{
        OperationExecutor, install::InstallExecutor, repair::RepairExecutor,
        uninstall::UninstallExecutor, update::UpdateExecutor, verify::VerifyExecutor,
    },
    models::OperationType,
};
use std::sync::Arc;

/// Factory for creating operation executors
///
/// Creates the appropriate executor implementation based on operation type.
/// Uses Arc for shared ownership across async tasks.
///
/// # Examples
///
/// ```
/// use cascette_agent::executor::factory::ExecutorFactory;
/// use cascette_agent::models::OperationType;
///
/// let factory = ExecutorFactory::new();
/// let executor = factory.create(OperationType::Install).expect("Failed to create executor");
/// ```
#[derive(Clone, Debug)]
pub struct ExecutorFactory;

// Future use: T078 (main.rs operation runner)
#[allow(dead_code)]
impl ExecutorFactory {
    /// Create a new executor factory
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Create an executor for the given operation type
    ///
    /// # Arguments
    ///
    /// - `operation_type`: Type of operation to execute
    ///
    /// # Returns
    ///
    /// - `Ok(Arc<dyn OperationExecutor>)`: Executor for the operation type
    /// - `Err(AgentError)`: If operation type is not supported
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::factory::ExecutorFactory;
    /// use cascette_agent::models::OperationType;
    ///
    /// let factory = ExecutorFactory::new();
    ///
    /// let install_executor = factory.create(OperationType::Install).expect("Failed to create executor");
    /// let update_executor = factory.create(OperationType::Update).expect("Failed to create executor");
    /// let repair_executor = factory.create(OperationType::Repair).expect("Failed to create executor");
    /// let verify_executor = factory.create(OperationType::Verify).expect("Failed to create executor");
    /// let uninstall_executor = factory.create(OperationType::Uninstall).expect("Failed to create executor");
    /// ```
    pub fn create(&self, operation_type: OperationType) -> Result<Arc<dyn OperationExecutor>> {
        match operation_type {
            OperationType::Install => Ok(Arc::new(InstallExecutor::new())),
            OperationType::Update => Ok(Arc::new(UpdateExecutor::new())),
            OperationType::Repair => Ok(Arc::new(RepairExecutor::new())),
            OperationType::Verify => Ok(Arc::new(VerifyExecutor::new())),
            OperationType::Uninstall => Ok(Arc::new(UninstallExecutor::new())),
        }
    }

    /// Check if an operation type is supported
    ///
    /// # Arguments
    ///
    /// - `operation_type`: Type to check
    ///
    /// # Returns
    ///
    /// `true` if the operation type is supported, `false` otherwise
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::factory::ExecutorFactory;
    /// use cascette_agent::models::OperationType;
    ///
    /// let factory = ExecutorFactory::new();
    ///
    /// assert!(factory.is_supported(OperationType::Install));
    /// assert!(factory.is_supported(OperationType::Update));
    /// ```
    #[must_use]
    pub fn is_supported(&self, operation_type: OperationType) -> bool {
        matches!(
            operation_type,
            OperationType::Install
                | OperationType::Update
                | OperationType::Repair
                | OperationType::Verify
                | OperationType::Uninstall
        )
    }

    /// Get list of supported operation types
    ///
    /// # Returns
    ///
    /// Vector of all supported operation types
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::factory::ExecutorFactory;
    ///
    /// let factory = ExecutorFactory::new();
    /// let supported = factory.supported_types();
    ///
    /// assert_eq!(supported.len(), 5);
    /// ```
    #[must_use]
    pub fn supported_types(&self) -> Vec<OperationType> {
        vec![
            OperationType::Install,
            OperationType::Update,
            OperationType::Repair,
            OperationType::Verify,
            OperationType::Uninstall,
        ]
    }
}

impl Default for ExecutorFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_new() {
        let factory = ExecutorFactory::new();
        assert!(factory.is_supported(OperationType::Install));
    }

    #[test]
    fn test_factory_default() {
        let factory = ExecutorFactory::default();
        assert!(factory.is_supported(OperationType::Install));
    }

    #[test]
    fn test_create_install_executor() {
        let factory = ExecutorFactory::new();
        let executor = factory.create(OperationType::Install);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_create_update_executor() {
        let factory = ExecutorFactory::new();
        let executor = factory.create(OperationType::Update);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_create_repair_executor() {
        let factory = ExecutorFactory::new();
        let executor = factory.create(OperationType::Repair);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_create_verify_executor() {
        let factory = ExecutorFactory::new();
        let executor = factory.create(OperationType::Verify);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_create_uninstall_executor() {
        let factory = ExecutorFactory::new();
        let executor = factory.create(OperationType::Uninstall);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_is_supported() {
        let factory = ExecutorFactory::new();

        assert!(factory.is_supported(OperationType::Install));
        assert!(factory.is_supported(OperationType::Update));
        assert!(factory.is_supported(OperationType::Repair));
        assert!(factory.is_supported(OperationType::Verify));
        assert!(factory.is_supported(OperationType::Uninstall));
    }

    #[test]
    fn test_supported_types() {
        let factory = ExecutorFactory::new();
        let supported = factory.supported_types();

        assert_eq!(supported.len(), 5);
        assert!(supported.contains(&OperationType::Install));
        assert!(supported.contains(&OperationType::Update));
        assert!(supported.contains(&OperationType::Repair));
        assert!(supported.contains(&OperationType::Verify));
        assert!(supported.contains(&OperationType::Uninstall));
    }

    #[test]
    fn test_factory_creates_different_instances() {
        let factory = ExecutorFactory::new();

        let executor1 = factory
            .create(OperationType::Install)
            .expect("Failed to create executor");
        let executor2 = factory
            .create(OperationType::Install)
            .expect("Failed to create executor");

        // Each call should create a new Arc, but they point to different instances
        assert!(!Arc::ptr_eq(&executor1, &executor2));
    }
}
