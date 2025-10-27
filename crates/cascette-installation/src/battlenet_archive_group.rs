/// Simplified archive-group generation for Battle.net installations
use std::fs;
use std::path::Path;

use cascette_formats::archive::{ArchiveGroupBuilder, ArchiveIndex};

use crate::{InstallationError, InstallationPlan};

type Result<T> = std::result::Result<T, InstallationError>;

/// Generate archive-group index by merging all archive indices
/// This creates a mega-index for faster lookups across all archives
pub fn generate_archive_group(plan: &InstallationPlan, target_dir: &Path) -> Result<()> {
    println!("→ Generating archive-group mega-index...");

    let indices_dir = target_dir.join("Data").join("indices");

    // Get CDN archive list to determine correct archive indices
    // Archive index = position in CDN archive list (CONFIRMED)
    let cdn_archives: Vec<String> = plan
        .archives
        .archives
        .iter()
        .map(|a| a.hash.clone())
        .collect();
    println!("  → CDN has {} archives defined", cdn_archives.len());

    // Build a map from archive hash to CDN position
    let mut hash_to_index = std::collections::HashMap::new();
    for (idx, archive_hash) in cdn_archives.iter().enumerate() {
        hash_to_index.insert(archive_hash.clone(), idx as u16);
    }

    // Scan for all .index files in the directory
    // Only process archive indices (38 chars = 32 hex + 6 ".index")
    // Skip existing archive-groups which are larger
    let index_files = fs::read_dir(&indices_dir)
        .map_err(|e| InstallationError::Other(format!("Failed to read indices directory: {e}")))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            if let Some(name) = entry.file_name().to_str() {
                // Only process normal archive indices (not archive-groups)
                // Archive indices are exactly 38 chars
                name.ends_with(".index") && name.len() == 38
            } else {
                false
            }
        })
        .filter(|entry| {
            // Additional filter: skip files larger than 1MB (likely archive-groups)
            if let Ok(metadata) = entry.metadata() {
                metadata.len() < 1024 * 1024 // Archive indices are typically <1MB
            } else {
                true
            }
        })
        .collect::<Vec<_>>();

    println!(
        "  → Found {} archive index files to process",
        index_files.len()
    );

    // Use the proper ArchiveGroupBuilder from cascette-formats
    let mut builder = ArchiveGroupBuilder::new();

    // Add each archive index to the builder
    // IMPORTANT: Battle.net's archive-group generation is complex:
    // - It creates multiple groups with size limits (~50MB)
    // - Archive indices are NOT sequential (0, 4, 7, 20, etc.)
    // - Groups include historical data from previous builds
    // - For now, we use index 0 for all entries (simplified approach)

    for index_file_entry in &index_files {
        let index_file_path = index_file_entry.path();
        let file_name = index_file_entry.file_name();
        let hash_str = file_name
            .to_str()
            .and_then(|s| s.strip_suffix(".index"))
            .ok_or_else(|| anyhow::anyhow!("Invalid archive index filename: {file_name:?}"))?;

        // Determine archive index from CDN position
        let archive_idx = hash_to_index.get(hash_str).copied().unwrap_or_else(|| {
            // If not in main CDN list, it might be a patch archive or historical
            // For now, assign high indices to these
            println!("  ⚠ Archive {hash_str} not in CDN list, assigning index 9999");
            9999
        });

        // Read and parse the archive index
        let index_data = fs::read(&index_file_path).map_err(|e| {
            InstallationError::Other(format!("Failed to read index file {hash_str}: {e}"))
        })?;

        let mut cursor = std::io::Cursor::new(index_data);
        match ArchiveIndex::parse(&mut cursor) {
            Ok(index) => {
                // Use correct archive index based on CDN position
                builder.add_archive(archive_idx, &index);
            }
            Err(e) => {
                println!("  ⚠ Skipping unparseable index file {hash_str}: {e}");
            }
        }
    }

    // Build the archive-group and write it
    let mut output_data = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut output_data);
    let archive_group = builder
        .build(&mut cursor)
        .map_err(|e| InstallationError::Other(format!("Failed to build archive-group: {e}")))?;

    // Calculate hash of the complete archive-group data
    let mega_index_hash = md5::compute(&output_data);
    let mega_index_hash_hex = format!("{mega_index_hash:x}");

    // Store the archive-group file
    let mega_index_path = indices_dir.join(format!("{mega_index_hash_hex}.index"));
    fs::write(&mega_index_path, &output_data)
        .map_err(|e| InstallationError::Other(format!("Failed to write archive-group: {e}")))?;

    println!("✓ Archive-group mega-index generated: {mega_index_hash_hex}");
    println!(
        "  Size: {} bytes, Entries: {}",
        output_data.len(),
        archive_group.entries.len()
    );

    Ok(())
}
