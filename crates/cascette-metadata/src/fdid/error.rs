//! Error types for FileDataID operations

use thiserror::Error;

/// Result type for FileDataID operations
pub type FileDataIdResult<T> = Result<T, FileDataIdError>;

/// Errors that can occur during FileDataID operations
#[derive(Debug, Error)]
pub enum FileDataIdError {
    /// Error from import provider operations
    #[cfg(feature = "import")]
    #[error("Import provider error: {0}")]
    ImportProvider(#[from] cascette_import::ImportError),

    /// Error from import provider operations (when import feature is disabled)
    #[cfg(not(feature = "import"))]
    #[error("Import provider error: {0}")]
    ImportProvider(String),

    /// FileDataID not found
    #[error("FileDataID not found: {0}")]
    IdNotFound(u32),

    /// File path not found
    #[error("File path not found: {0}")]
    PathNotFound(String),

    /// Invalid FileDataID
    #[error("Invalid FileDataID: {0}")]
    InvalidId(u32),

    /// Invalid file path format
    #[error("Invalid file path format: {0}")]
    InvalidPath(String),

    /// Provider not available
    #[error("Provider not available: {0}")]
    ProviderUnavailable(String),

    /// Cache operation failed
    #[error("Cache operation failed: {0}")]
    Cache(String),

    /// Storage operation failed
    #[error("Storage operation failed: {0}")]
    Storage(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Generic error
    #[error("{0}")]
    Generic(String),
}
