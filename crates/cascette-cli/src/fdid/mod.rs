//! FileDataID command implementations
//!
//! This module provides comprehensive FileDataID operations including:
//! - File resolution (ID ↔ path bidirectional lookup)
//! - Content discovery and browsing with filters
//! - Advanced pattern-based search functionality
//! - Batch operations for processing multiple files
//! - Content analysis and statistical reporting
//! - Validation and consistency checking
//! - Performance profiling and optimization

#[allow(dead_code)]
mod analyze;
#[allow(dead_code)]
mod batch;
#[allow(dead_code)]
mod browse;
pub mod commands;
#[allow(dead_code)]
mod profile;
#[allow(dead_code)]
mod resolve;
#[allow(dead_code)]
mod search;
#[allow(dead_code)]
mod stats;
#[allow(dead_code)]
mod validate;

// Commands are accessed directly via fdid::commands:: from main.rs

use crate::paths;
use anyhow::{Context, Result};
use cascette_metadata::fdid::{CachedStorage, FileDataIdImporter, FileDataIdStorage};

/// Initialize FileDataID storage with caching
pub async fn create_storage() -> Result<CachedStorage> {
    let data_dir = paths::data_dir().context("Failed to determine data directory")?;
    let cache_path = data_dir.join("filedataid_cache.bin");

    // Create storage with cache file
    let mut storage = CachedStorage::new(Some(cache_path.clone()));

    // Try to load from cache first
    if storage.initialize().await.is_err() {
        // If cache doesn't exist or fails to load, import fresh data
        println!("Cache not found or invalid, importing FileDataID mappings...");

        let importer = FileDataIdImporter::new();
        let cache_path_for_import = Some(cache_path.as_path());

        storage = importer
            .create_populated_storage(cache_path_for_import)
            .await
            .context("Failed to import FileDataID mappings")?;

        println!("✓ FileDataID mappings imported and cached");
    } else if storage.mapping_count() == 0 {
        // Cache exists but is empty, reimport
        println!("Cache is empty, importing FileDataID mappings...");

        let importer = FileDataIdImporter::new();
        let mappings = importer
            .import_from_wowdev()
            .await
            .context("Failed to download FileDataID mappings")?;

        let mappings_iter = mappings.id_to_path.into_iter();
        storage
            .load_and_cache_mappings(mappings_iter)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to cache mappings: {}", e))?;

        println!(
            "✓ FileDataID mappings imported and cached ({})",
            storage.mapping_count()
        );
    } else {
        println!(
            "✓ Loaded {} FileDataID mappings from cache",
            storage.mapping_count()
        );
    }

    Ok(storage)
}

/// Common output formats supported by FileDataID commands
#[derive(Copy, Clone, Debug)]
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

impl From<&str> for OutputFormat {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => Self::Json,
            "csv" => Self::Csv,
            _ => Self::Table,
        }
    }
}

/// File category for content classification
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileCategory {
    Unknown,
    Model,
    Texture,
    Audio,
    Music,
    Video,
    Database,
    Interface,
    Map,
    Animation,
    Shader,
    Script,
    Configuration,
    Font,
    Other(String),
}

impl From<&str> for FileCategory {
    fn from(path: &str) -> Self {
        use std::path::Path;

        let path_obj = Path::new(path);
        let path_lower = path.to_lowercase();

        // Get extension if it exists
        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);

        // Check extension first
        match ext.as_deref() {
            Some("m2" | "wmo" | "mdx") => Self::Model,
            Some("blp" | "dds" | "tga" | "jpg" | "png") => Self::Texture,
            Some("ogg" | "wav" | "mp3") => Self::Audio,
            Some("avi" | "bik" | "webm") => Self::Video,
            Some("dbc" | "db2" | "adb") => Self::Database,
            Some("toc" | "xml" | "lua") => Self::Interface,
            Some("adt" | "wdt") => Self::Map,
            Some("anim") => Self::Animation,
            Some("hlsl" | "fx") => Self::Shader,
            Some("py" | "js") => Self::Script,
            Some("cfg" | "conf" | "ini" | "wtf") => Self::Configuration,
            Some("ttf" | "otf") => Self::Font,
            _ => {
                // Check path contents for categories without specific extensions
                if path_lower.contains("music") || path_lower.contains("sound\\music") {
                    Self::Music
                } else if path_lower.contains("interface") {
                    Self::Interface
                } else if path_lower.contains("animation") {
                    Self::Animation
                } else if path_lower.contains("shaders") {
                    Self::Shader
                } else if path_lower.contains("fonts") {
                    Self::Font
                } else if let Some(ext_str) = ext {
                    if ext_str.len() <= 10 && ext_str.chars().all(|c| c.is_ascii_alphanumeric()) {
                        Self::Other(ext_str.to_uppercase())
                    } else {
                        Self::Unknown
                    }
                } else {
                    Self::Unknown
                }
            }
        }
    }
}

impl std::fmt::Display for FileCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::Model => write!(f, "Model"),
            Self::Texture => write!(f, "Texture"),
            Self::Audio => write!(f, "Audio"),
            Self::Music => write!(f, "Music"),
            Self::Video => write!(f, "Video"),
            Self::Database => write!(f, "Database"),
            Self::Interface => write!(f, "Interface"),
            Self::Map => write!(f, "Map"),
            Self::Animation => write!(f, "Animation"),
            Self::Shader => write!(f, "Shader"),
            Self::Script => write!(f, "Script"),
            Self::Configuration => write!(f, "Configuration"),
            Self::Font => write!(f, "Font"),
            Self::Other(ext) => write!(f, "{}", ext),
        }
    }
}

/// File information structure for display
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct FileInfo {
    pub file_data_id: u32,
    pub path: String,
    pub category: FileCategory,
    #[allow(dead_code)]
    pub estimated_size: Option<u64>,
    pub requires_encryption: bool,
    pub compression_level: u8,
}

impl FileInfo {
    pub fn new(file_data_id: u32, path: String) -> Self {
        let category = FileCategory::from(path.as_str());
        Self {
            file_data_id,
            path,
            category,
            estimated_size: None,
            requires_encryption: false,
            compression_level: 0,
        }
    }
}
