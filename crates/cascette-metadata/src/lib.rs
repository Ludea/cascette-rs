//! # cascette-metadata - Content Metadata Orchestration Layer
//!
//! This crate provides the central orchestration layer for NGDP/CASC content metadata,
//! managing the lifecycle of content from source files to encrypted BLTE blocks.
//! It serves as the brain for both client operations (content resolution) and
//! server operations (build generation).
//!
//! ## Core Responsibilities
//!
//! - **`FileDataID` Management**: Mapping between numeric IDs and file paths
//! - **TACT Key Orchestration**: Managing which keys encrypt which content
//! - **Content Metadata**: Tracking transformations through the pipeline
//! - **Manifest Generation**: Coordinating creation of CASC manifests
//! - **Policy Management**: Compression and encryption decisions
//!
//! ## Architecture
//!
//! This crate sits between the application layer (CLI, launcher) and the
//! implementation layers (crypto, formats), providing orchestration without
//! handling the actual cryptographic operations or format parsing.
//!
//! ```text
//! Applications (CLI, Launcher)
//!          ↓
//!    cascette-metadata (orchestration)
//!          ↓
//! cascette-crypto + cascette-formats (implementation)
//! ```

#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::case_sensitive_file_extension_comparisons)]

pub mod error;
pub mod fdid;
pub mod orchestrator;
pub mod tact;

// Re-export main types
pub use error::{MetadataError, MetadataResult};

// Re-export FileDataID management
pub use fdid::{
    FileDataIdMapping, FileDataIdProvider, FileDataIdService, FileDataIdStats, MemoryProvider,
};

// Re-export import-specific types when available
#[cfg(feature = "import")]
pub use fdid::ListfileProviderAdapter;

// Re-export orchestration
pub use orchestrator::{
    ContentCategory, ContentInfo, FullContentInfo, HealthStatus, MetadataOrchestrator,
    OrchestratorBuilder, OrchestratorConfig, OrchestratorStats, ValidationCacheStats,
    ValidationCategory, ValidationIssue, ValidationResult, ValidationSeverity,
};

// Re-export TACT management
pub use tact::{TactKeyManager, TactKeyStats};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get version string with build information
pub fn version_string() -> String {
    format!("cascette-metadata {}", VERSION)
}
