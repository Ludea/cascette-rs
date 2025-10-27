//! # cascette-installation
//!
//! Installation library for managing product installations, updates, and verification.
//!
//! This library provides the core installation logic extracted from cascette-cli,
//! enabling reuse across different contexts (CLI, agent service, etc.).

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Public modules
/// Archive index optimization for Battle.net mode installations
pub mod archive_optimizer;
/// Atomic file writing with crash safety
pub mod atomic_writer;
/// Battle.net installation mode support
pub mod battlenet;
/// Archive-group index generation for Battle.net mode
pub mod battlenet_archive_group;
/// Build metadata management and querying
pub mod builds;
/// Configuration types for installation parameters
pub mod config;
/// Error types for installation operations
pub mod error;
/// Installation plan execution and file operations
pub mod executor;
/// Manifest file fetching from CDN
pub mod manifest_fetcher;
/// Installation metadata generation
pub mod metadata;
/// Data models for installation plans and requests
pub mod models;
/// Installation plan creation and validation
pub mod plan;
/// NGDP-specific plan building from CDN manifests
pub mod plan_ngdp;
/// Progress reporting types and callbacks
pub mod progress;
/// Progress tracking during installations
pub mod progress_tracker;
/// Resume support for interrupted installations
pub mod resume;
/// Retry logic with exponential backoff
pub mod retry;

// Re-export commonly used types
pub use error::{InstallationError, Result};
pub use models::InstallationPlan;
