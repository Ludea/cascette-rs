//! Build metadata management for NGDP/CASC builds
//!
//! This module provides structures and functionality for storing and managing
//! build metadata locally. It supports both live NGDP queries and historic
//! data imports from wago.tools.

#[allow(dead_code)]
pub mod manager;
#[allow(dead_code)]
pub mod metadata;

// Re-exports for external use when this module is integrated
#[allow(unused_imports)]
pub use manager::BuildManager;
#[allow(unused_imports)]
pub use metadata::{
    BuildInfo, BuildMetadata, CatalogInfo, CdnInfo, CdnProtocol, ConfigInfo, DataSource,
    MetadataInfo, PatchInfo, ProductInfo, RegionInfo, parse_version_build,
};
