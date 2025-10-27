//! Execution context for operation executors
//!
//! Provides shared state and resources needed during operation execution.

use crate::config::AgentConfig;
use crate::executor::ProgressReporter;
use crate::observability::metrics::Metrics;
use crate::state::ProductRegistry;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Execution context for operations
///
/// Provides access to shared resources needed during operation execution:
/// - Configuration settings
/// - Progress reporting mechanism
/// - Product registry for state updates
/// - Cancellation token for graceful shutdown
/// - Metrics collection for observability
///
/// The context is immutable and can be shared across multiple operations.
///
/// # Examples
///
/// ```
/// use cascette_agent::executor::ExecutionContext;
/// use cascette_agent::config::AgentConfig;
/// use cascette_agent::observability::metrics::Metrics;
/// use tokio_util::sync::CancellationToken;
///
/// # struct MockProgressReporter;
/// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
/// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
/// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
/// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
/// # }
/// # use cascette_agent::state::{Database, ProductRegistry};
/// # use std::sync::{Arc, Mutex};
/// let config = AgentConfig::default();
/// let reporter = std::sync::Arc::new(MockProgressReporter);
/// let token = CancellationToken::new();
/// let metrics = Metrics::new();
/// # let db = Database::in_memory().unwrap();
/// # let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
///
/// let context = ExecutionContext::new(config, reporter, registry, metrics, token);
///
/// assert!(!context.is_cancelled());
/// ```
#[derive(Clone)]
pub struct ExecutionContext {
    /// Agent configuration
    pub config: AgentConfig,

    /// Progress reporter for state and progress updates
    pub progress_reporter: Arc<dyn ProgressReporter>,

    /// Product registry for managing product state (T099)
    pub registry: Arc<ProductRegistry>,

    /// Metrics collection for observability (T097, T100)
    pub metrics: Metrics,

    /// Cancellation token for graceful shutdown
    pub cancellation_token: CancellationToken,
}

// Future use: T078 (main.rs operation runner)
#[allow(dead_code)]
impl ExecutionContext {
    /// Create a new execution context
    ///
    /// # Arguments
    ///
    /// - `config`: Agent configuration
    /// - `progress_reporter`: Progress reporting implementation
    /// - `registry`: Product registry for state updates
    /// - `metrics`: Metrics collection for observability
    /// - `cancellation_token`: Token for cancellation signaling
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::ExecutionContext;
    /// use cascette_agent::config::AgentConfig;
    /// use cascette_agent::observability::metrics::Metrics;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// # struct MockProgressReporter;
    /// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
    /// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
    /// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
    /// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
    /// # }
    /// # use cascette_agent::state::{Database, ProductRegistry};
    /// # use std::sync::{Arc, Mutex};
    /// # let db = Database::in_memory().unwrap();
    /// # let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
    /// let context = ExecutionContext::new(
    ///     AgentConfig::default(),
    ///     std::sync::Arc::new(MockProgressReporter),
    ///     registry,
    ///     Metrics::new(),
    ///     CancellationToken::new(),
    /// );
    /// ```
    pub fn new(
        config: AgentConfig,
        progress_reporter: Arc<dyn ProgressReporter>,
        registry: Arc<ProductRegistry>,
        metrics: Metrics,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            config,
            progress_reporter,
            registry,
            metrics,
            cancellation_token,
        }
    }

    /// Check if cancellation has been requested
    ///
    /// Operations should check this periodically and return promptly when true.
    ///
    /// # Returns
    ///
    /// `true` if cancellation has been requested, `false` otherwise
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::ExecutionContext;
    /// use cascette_agent::config::AgentConfig;
    /// use cascette_agent::observability::metrics::Metrics;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// # struct MockProgressReporter;
    /// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
    /// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
    /// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
    /// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
    /// # }
    /// # use cascette_agent::state::{Database, ProductRegistry};
    /// # use std::sync::{Arc, Mutex};
    /// let token = CancellationToken::new();
    /// # let db = Database::in_memory().unwrap();
    /// # let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
    /// let context = ExecutionContext::new(
    ///     AgentConfig::default(),
    ///     std::sync::Arc::new(MockProgressReporter),
    ///     registry,
    ///     Metrics::new(),
    ///     token.clone(),
    /// );
    ///
    /// assert!(!context.is_cancelled());
    ///
    /// token.cancel();
    /// assert!(context.is_cancelled());
    /// ```
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Get a child cancellation token
    ///
    /// Creates a token that is cancelled when either the parent token or the
    /// child token is cancelled. Useful for sub-operations that need their own
    /// cancellation scope.
    ///
    /// # Returns
    ///
    /// New cancellation token that inherits from parent
    ///
    /// # Examples
    ///
    /// ```
    /// use cascette_agent::executor::ExecutionContext;
    /// use cascette_agent::config::AgentConfig;
    /// use cascette_agent::observability::metrics::Metrics;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// # struct MockProgressReporter;
    /// # impl cascette_agent::executor::ProgressReporter for MockProgressReporter {
    /// #     fn report_progress(&self, _: uuid::Uuid, _: cascette_agent::models::Progress) {}
    /// #     fn report_state_change(&self, _: uuid::Uuid, _: cascette_agent::models::OperationState) {}
    /// #     fn report_error(&self, _: uuid::Uuid, _: String, _: String, _: Option<serde_json::Value>) {}
    /// # }
    /// # use cascette_agent::state::{Database, ProductRegistry};
    /// # use std::sync::{Arc, Mutex};
    /// # let db = Database::in_memory().unwrap();
    /// # let registry = Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))));
    /// let context = ExecutionContext::new(
    ///     AgentConfig::default(),
    ///     std::sync::Arc::new(MockProgressReporter),
    ///     registry,
    ///     Metrics::new(),
    ///     CancellationToken::new(),
    /// );
    ///
    /// let child_token = context.child_token();
    ///
    /// // Cancelling parent cancels child
    /// context.cancellation_token.cancel();
    /// assert!(child_token.is_cancelled());
    /// ```
    #[must_use]
    pub fn child_token(&self) -> CancellationToken {
        self.cancellation_token.child_token()
    }

    /// Get maximum concurrent operations allowed
    ///
    /// # Returns
    ///
    /// Maximum number of concurrent operations from config
    #[must_use]
    pub fn max_concurrent_operations(&self) -> usize {
        self.config.operations.max_concurrent
    }

    /// Get operation retention period in days
    ///
    /// # Returns
    ///
    /// Number of days to retain operation history
    #[must_use]
    pub fn retention_days(&self) -> u32 {
        self.config.operations.retention_days
    }
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("config", &self.config)
            .field("is_cancelled", &self.is_cancelled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::ProgressReporter;
    use crate::models::{OperationState, Progress};
    use crate::state::Database;
    use std::sync::Mutex;

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

    fn create_test_registry() -> Arc<ProductRegistry> {
        let db = Database::in_memory().expect("Failed to create test database");
        Arc::new(ProductRegistry::new(Arc::new(Mutex::new(db))))
    }

    #[test]
    fn test_new_context() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context = ExecutionContext::new(config.clone(), reporter, registry, metrics, token);

        assert_eq!(
            context.config.network.bind_address,
            config.network.bind_address
        );
        assert!(!context.is_cancelled());
    }

    #[test]
    fn test_is_cancelled() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context = ExecutionContext::new(config, reporter, registry, metrics, token.clone());

        assert!(!context.is_cancelled());

        token.cancel();
        assert!(context.is_cancelled());
    }

    #[test]
    fn test_child_token() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let parent_token = CancellationToken::new();

        let context =
            ExecutionContext::new(config, reporter, registry, metrics, parent_token.clone());

        let child_token = context.child_token();

        // Parent not cancelled, child not cancelled
        assert!(!child_token.is_cancelled());

        // Cancel parent, child should be cancelled
        parent_token.cancel();
        assert!(child_token.is_cancelled());
    }

    #[test]
    fn test_child_token_independent_cancellation() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let parent_token = CancellationToken::new();

        let context =
            ExecutionContext::new(config, reporter, registry, metrics, parent_token.clone());

        let child_token = context.child_token();

        // Cancel child, parent should not be cancelled
        child_token.cancel();
        assert!(!parent_token.is_cancelled());
        assert!(child_token.is_cancelled());
    }

    #[test]
    fn test_max_concurrent_operations() {
        let mut config = AgentConfig::default();
        config.operations.max_concurrent = 5;

        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context = ExecutionContext::new(config, reporter, registry, metrics, token);

        assert_eq!(context.max_concurrent_operations(), 5);
    }

    #[test]
    fn test_retention_days() {
        let mut config = AgentConfig::default();
        config.operations.retention_days = 30;

        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context = ExecutionContext::new(config, reporter, registry, metrics, token);

        assert_eq!(context.retention_days(), 30);
    }

    #[test]
    fn test_context_clone() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context1 = ExecutionContext::new(config, reporter, registry, metrics, token.clone());
        let context2 = context1.clone();

        // Both contexts share the same cancellation token
        token.cancel();
        assert!(context1.is_cancelled());
        assert!(context2.is_cancelled());
    }

    #[test]
    fn test_debug_impl() {
        let config = AgentConfig::default();
        let reporter = Arc::new(MockProgressReporter);
        let registry = create_test_registry();
        let metrics = Metrics::new();
        let token = CancellationToken::new();

        let context = ExecutionContext::new(config, reporter, registry, metrics, token);

        let debug_str = format!("{:?}", context);
        assert!(debug_str.contains("ExecutionContext"));
        assert!(debug_str.contains("is_cancelled"));
    }
}
