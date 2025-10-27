//! HTTP request handlers for agent service
//!
//! Organized by resource type:
//! - health: Service health check
//! - operations: Operation management
//! - products: Product installation and management
//! - progress: Progress monitoring (legacy)
//! - cancel: Operation cancellation (legacy)

pub mod cancel;
pub mod health;
pub mod metrics;
pub mod operations;
pub mod products;
pub mod progress;

// Re-export handler functions
pub use health::health_check;
pub use metrics::metrics;
pub use operations::{cancel_operation, get_operation, list_operations};
pub use products::{get_product, install_product, list_products, update_product};
pub use progress::{get_operation_progress, get_product_progress};
