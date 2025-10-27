//! Error types for installation module

use thiserror::Error;

/// Installation-specific errors
#[derive(Debug, Error)]
pub enum InstallationError {
    /// Product not found in NGDP or build database
    #[error("Product not found: {0}")]
    ProductNotFound(String),

    /// Build not found (historic builds require import)
    #[error(
        "Build {0} not found. Historic builds must be imported first using 'cascette builds import'."
    )]
    BuildNotFound(u32),

    /// Invalid configuration data or format
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Network or CDN download error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Filesystem I/O error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON serialization or deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Installation plan is incomplete or invalid
    #[error("Plan not ready for execution")]
    #[allow(dead_code)] // Future error case
    PlanNotReady,

    /// Installation was cancelled by user
    #[error("Installation cancelled")]
    #[allow(dead_code)] // Future error case
    Cancelled,

    /// Other unspecified error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<anyhow::Error> for InstallationError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

/// Result type for installation operations
pub type Result<T> = std::result::Result<T, InstallationError>;
