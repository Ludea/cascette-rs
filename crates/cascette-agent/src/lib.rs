//! # cascette-agent
//!
//! Agent service for managing product installations, updates, and operations.
//!
//! This library provides the core functionality for the cascette agent service,
//! enabling programmatic control of product lifecycle management.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Public modules
pub mod config;
pub mod error;
pub mod executor;
pub mod models;
pub mod observability;
pub mod server;
pub mod state;

// Re-export commonly used types
pub use config::AgentConfig;
pub use error::{AgentError, OperationError, ProductError, Result};
pub use executor::{ExecutionContext, OperationExecutor, ProgressReporter};
pub use models::{Operation, OperationState, OperationType, Priority, Product, ProductStatus};
pub use observability::{Observability, ObservabilityConfig, init as init_observability};
pub use server::create_router;
pub use state::{AppState, Database, OperationQueue, ProductRegistry};
