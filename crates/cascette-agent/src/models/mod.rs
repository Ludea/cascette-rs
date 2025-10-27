//! Data models for agent service
//!
//! This module contains the core domain models for product management
//! and operation execution.

pub mod operation;
pub mod product;
pub mod progress;

// Re-export commonly used types
pub use operation::{ErrorInfo, Operation, OperationState, OperationType, Priority};
pub use product::{InstallationMode, Product, ProductStatus};
pub use progress::Progress;
