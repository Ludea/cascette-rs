//! Error types for metadata operations

use thiserror::Error;

/// Result type for metadata operations
pub type MetadataResult<T> = Result<T, MetadataError>;

/// Errors that can occur during metadata operations
#[derive(Debug, Error)]
pub enum MetadataError {
    /// Error from crypto operations
    #[error("Crypto error: {0}")]
    Crypto(#[from] cascette_crypto::CryptoError),

    /// Error from file store operations
    #[error("File store error: {0}")]
    FileStore(#[from] cascette_crypto::FileStoreError),

    /// Error from format operations
    #[error("Format error: {0}")]
    Format(String),

    /// Invalid key format
    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),

    /// Key not found
    #[error("Key not found: {0:016X}")]
    KeyNotFound(u64),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// `FileDataID` not found
    #[error("FileDataID not found: {0}")]
    FileDataIdNotFound(u32),

    /// File path not found
    #[error("File path not found: {0}")]
    FilePathNotFound(String),

    /// Invalid `FileDataID` mapping format
    #[error("Invalid FileDataID mapping format: {0}")]
    InvalidMappingFormat(String),

    /// Provider error
    #[error("Provider error: {0}")]
    Provider(String),

    /// Cache operation error
    #[error("Cache error: {0}")]
    CacheError(String),

    /// Invalid cache configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Generic error
    #[error("{0}")]
    Generic(String),
}
