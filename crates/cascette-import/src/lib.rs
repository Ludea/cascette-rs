//! # cascette-import - Community Data Source Integration
//!
//! This crate provides integration with community-maintained data sources for NGDP/CASC
//! operations, enabling access to historical build information, file listings, encryption
//! keys, and alternative content distribution networks.
//!
//! ## Supported Data Sources
//!
//! ### wago.tools Integration
//! - Historical build information via `/api/builds` endpoint
//! - Product version tracking across all Blizzard games
//! - Community-maintained build archives
//!
//! ### `WoWDev` Listfile
//! - Community-maintained file ID to path mappings
//! - Regular updates from `github.com/wowdev/wow-listfile`
//! - Support for multiple game versions and expansions
//!
//! ### TACT Keys Repository
//! - Encryption key database from `github.com/wowdev/TACTKeys`
//! - Automated key discovery and validation
//! - Integration with cascette-crypto for decryption
//!
//! ### Future Providers
//! - Peer-to-peer content distribution
//! - Custom CDN endpoints and mirrors
//! - Private server data sources
//!
//! ## Usage Examples
//!
//! ### Basic Build Information
//!
//! ```rust,ignore
//! use cascette_import::{ImportManager, wago::WagoProvider};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut manager = ImportManager::new();
//! let wago = WagoProvider::new().await?;
//! manager.add_provider("wago", Box::new(wago));
//!
//! let builds = manager.get_builds("wow").await?;
//! println!("Found {} builds for WoW", builds.len());
//! # Ok(())
//! # }
//! ```
//!
//! ### File Listfile Integration
//!
//! ```rust,ignore
//! use cascette_import::{ImportManager, listfile::create_listfile_provider};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut manager = ImportManager::new();
//! let listfile = create_listfile_provider().await?;
//! manager.add_provider("listfile", Box::new(listfile));
//!
//! let path = manager.resolve_file_id(123456).await?;
//! println!("File ID 123456 maps to: {}", path.unwrap_or_default());
//! # Ok(())
//! # }
//! ```
//!
//! ### TACT Key Management
//!
//! TACT keys are now managed through the CLI key manager for unified security.
//! Use the `cascette keys import-github` command to import keys from the
//! community repository with deduplication support.
//!
//! ## Architecture
//!
//! The import system uses a provider-based architecture where each data source
//! implements the `ImportProvider` trait. This allows for:
//!
//! - **Pluggable Sources**: Easy addition of new community data sources
//! - **Caching Integration**: Automatic caching using cascette-cache
//! - **Fallback Support**: Multiple providers for the same data type
//! - **Rate Limiting**: Respectful API usage with built-in limits
//! - **Offline Mode**: Local caching for network-independent operation

#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::use_self)]

pub mod error;
pub mod manager;
pub mod providers;
pub mod tactkeys_fetch;

// Data source modules
#[cfg(feature = "wago")]
pub mod wago;

#[cfg(feature = "listfile")]
pub mod listfile;

#[cfg(feature = "tactkeys")]
pub mod tactkeys;

// Re-export main types
pub use error::{ImportError, ImportResult};
pub use manager::{ImportManager, ImportManagerConfig};
pub use providers::{DataSource, ImportProvider, ImportProviderInfo};

// Re-export provider types based on features
#[cfg(feature = "wago")]
pub use wago::WagoProvider;

#[cfg(feature = "listfile")]
pub use listfile::ListfileProvider;

#[cfg(feature = "tactkeys")]
pub use tactkeys::TactKeysProvider;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get version string with build information
pub fn version_string() -> String {
    format!("cascette-import {}", VERSION)
}

/// Common data types used across providers
pub mod types {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    /// Build information from community sources
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct BuildInfo {
        /// Product identifier (wow, diablo4, etc.)
        pub product: String,

        /// Version string (e.g., "11.0.7.57212")
        pub version: String,

        /// Build number
        pub build: u32,

        /// Version type (live, beta, alpha, etc.)
        pub version_type: String,

        /// Region if applicable
        pub region: Option<String>,

        /// Release timestamp if known
        pub timestamp: Option<chrono::DateTime<chrono::Utc>>,

        /// Additional metadata
        pub metadata: HashMap<String, String>,
    }

    /// File ID to path mapping
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileMapping {
        /// File data ID
        pub file_id: u32,

        /// Full file path
        pub path: String,

        /// File name only
        pub filename: String,

        /// Directory path
        pub directory: String,
    }

    /// Import provider capabilities
    #[derive(Debug, Clone)]
    #[allow(clippy::struct_excessive_bools)]
    pub struct ProviderCapabilities {
        /// Can provide build information
        pub builds: bool,

        /// Can provide file mappings
        pub file_mappings: bool,

        /// Supports real-time updates
        pub real_time: bool,

        /// Requires authentication
        pub requires_auth: bool,
    }
}
