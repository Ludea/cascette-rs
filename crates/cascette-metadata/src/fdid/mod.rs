//! `FileDataID` orchestration and management
//!
//! This module provides high-level orchestration for `FileDataID` mappings,
//! managing bidirectional lookups between numeric `FileDataID` values and file paths
//! while maintaining high performance for large datasets (500k+ mappings).
//!
//! ## Core Responsibilities
//!
//! - **Bidirectional Lookups**: Map FileDataID ↔ File Path efficiently
//! - **Provider Abstraction**: Support multiple data sources (listfile, CSV, database)
//! - **Performance Optimization**: Handle 500k+ mappings with fast lookups
//! - **Statistics Tracking**: Monitor usage patterns and performance metrics
//! - **Thread Safety**: Support concurrent access patterns
//!
//! ## Architecture
//!
//! The FileDataID system uses a provider-based architecture where different
//! data sources can be plugged in through the `FileDataIdProvider` trait.
//! The main `FileDataIdService` orchestrates these providers and maintains
//! efficient in-memory indices for fast lookups.
//!
//! ## Usage Example
//!
//! ```rust
//! use cascette_metadata::fdid::{FileDataIdService, MemoryProvider, FileDataIdMapping};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a memory provider with test data
//! let mut provider = MemoryProvider::empty();
//! provider.add_mapping(FileDataIdMapping::new(
//!     12345,
//!     "Interface/AddOns/MyAddon/MyAddon.toc".to_string()
//! ));
//!
//! // Create service and load mappings
//! let mut service = FileDataIdService::new(Box::new(provider));
//! service.load_from_provider().await?;
//!
//! // Lookup operations
//! if let Some(path) = service.get_file_path(12345)? {
//!     println!("FileDataID 12345 maps to: {}", path);
//! }
//!
//! if let Some(id) = service.get_file_data_id("Interface/AddOns/MyAddon/MyAddon.toc")? {
//!     println!("File maps to FileDataID: {}", id);
//! }
//! # Ok(())
//! # }
//! ```

pub mod adapter;
pub mod cache;
pub mod error;
pub mod import_utils;
pub mod provider;
pub mod service;
pub mod storage;
pub mod types;

// Re-export main types from new provider system
pub use provider::{
    FileDataIdMapping, FileDataIdProvider, FileDataIdQuery, MemoryProvider, ProviderCapabilities,
    ProviderInfo, ResolutionStats, SourceType, UnifiedFileDataIdProvider,
};

// Re-export adapter functionality
pub use adapter::AdapterConfig;

// Re-export import-specific functionality only when enabled
#[cfg(feature = "import")]
pub use adapter::ListfileProviderAdapter;

// Re-export error types
pub use error::{FileDataIdError, FileDataIdResult};

// Re-export service
pub use service::FileDataIdService;

// Re-export cache types
pub use cache::{CacheConfig, CacheStats, FileDataIdCache};

// Re-export storage types
pub use storage::{CachedStorage, FileDataIdMappings, FileDataIdStorage, HashMapStorage};

// Re-export import utilities
pub use import_utils::{FileDataIdImporter, ImportError, ImportResult};

// Re-export legacy types for compatibility
pub use types::{CacheMetrics, FileDataIdStats};
