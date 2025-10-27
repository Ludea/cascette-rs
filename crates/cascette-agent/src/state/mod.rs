//! State management for agent service
//!
//! Provides database schema, migrations, and CRUD operations for products and operations.
//! Enforces business rules like single active operation per product (FR-022) and
//! automatic cleanup of old operations (FR-031: 90-day retention).

pub mod db;
pub mod queue;
pub mod registry;

use std::sync::Arc;

// Re-export commonly used types
// Note: Database will be used by main.rs for file-based persistence (T078)
#[allow(unused_imports)]
pub use db::Database;
pub use queue::OperationQueue;
pub use registry::ProductRegistry;

// Future use: T078 (main.rs server state)
#[allow(dead_code)]
/// Application state shared across HTTP handlers
///
/// Contains references to all stateful components needed for request processing.
#[derive(Clone)]
pub struct AppState {
    /// Operation queue for managing operation lifecycle
    pub queue: Arc<OperationQueue>,

    /// Product registry for managing product state
    pub registry: Arc<ProductRegistry>,

    /// Metrics for observability
    pub metrics: Arc<crate::observability::Metrics>,
}

// Future use: T078 (main.rs server initialization)
#[allow(dead_code)]
impl AppState {
    /// Create new application state
    #[must_use]
    pub fn new(
        queue: Arc<OperationQueue>,
        registry: Arc<ProductRegistry>,
        metrics: Arc<crate::observability::Metrics>,
    ) -> Self {
        Self {
            queue,
            registry,
            metrics,
        }
    }
}
