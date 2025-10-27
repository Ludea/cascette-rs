//! HTTP server for agent service
//!
//! Provides REST API for product installation and operation management.
//! Implements axum-based HTTP server with proper error handling, middleware,
//! and observability integration.

pub mod handlers;
pub mod models;
pub mod router;

pub use models::{
    ErrorResponse, HealthResponse, InstallRequest, OperationListResponse, OperationResponse,
    ProductResponse, UpdateRequest,
};
pub use router::create_router;
