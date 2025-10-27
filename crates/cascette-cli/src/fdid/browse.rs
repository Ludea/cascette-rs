//! FileDataID browse command implementation
//!
//! Provides content discovery and browsing capabilities with advanced filtering
//! options including extension, path patterns, size ranges, and category-based filtering.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use comfy_table::{Table, presets::UTF8_FULL};
use console::style;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;

use super::{FileCategory, FileInfo, OutputFormat};

/// Filtering options for browse command
#[derive(Default)]
pub struct BrowseFilter {
    pub extension: Option<String>,
    pub path_pattern: Option<String>,
    pub category: Option<FileCategory>,
    pub min_id: Option<u32>,
    pub max_id: Option<u32>,
    pub limit: Option<usize>,
}

/// Execute the FileDataID browse command
pub async fn execute_browse(
    orchestrator: &mut MetadataOrchestrator,
    filter: BrowseFilter,
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    // Load mappings to enable browsing
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    println!("{} Discovering content with filters...", style("→").dim());

    // Get all mappings and apply filters
    let filtered_files = apply_browse_filters(orchestrator, &filter)?;

    if filtered_files.is_empty() {
        println!(
            "{} No files found matching the specified filters",
            style("⚠").yellow()
        );
        return Ok(());
    }

    // Display results
    display_browse_results(&filtered_files, format, show_metadata)?;

    // Show summary
    let total_count = filtered_files.len();
    println!(
        "\n{} Found {} files matching filters",
        style("✓").green().bold(),
        style(total_count).yellow()
    );

    if total_count >= 1000 {
        println!(
            "{} Use --limit to show fewer results or more specific filters",
            style("Tip:").cyan()
        );
    }

    Ok(())
}

/// Apply filtering logic to discover matching files
fn apply_browse_filters(
    orchestrator: &MetadataOrchestrator,
    filter: &BrowseFilter,
) -> Result<Vec<FileInfo>> {
    const CHUNK_SIZE: u32 = 10000;

    let mut results = Vec::new();
    let regex_pattern = if let Some(ref pattern) = filter.path_pattern {
        Some(Regex::new(pattern).context("Invalid regex pattern")?)
    } else {
        None
    };

    // Since we don't have a direct API to iterate all mappings,
    // we'll simulate browsing by checking a range of FileDataIDs
    let start_id = filter.min_id.unwrap_or(1);
    let end_id = filter.max_id.unwrap_or(3_000_000); // Reasonable upper bound for WoW FileDataIDs
    let limit = filter.limit.unwrap_or(1000);

    let mut processed_count = 0;
    let mut found_count = 0;

    // Process in chunks to avoid overwhelming the system
    for chunk_start in (start_id..=end_id).step_by(CHUNK_SIZE as usize) {
        let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE - 1, end_id);

        for file_data_id in chunk_start..=chunk_end {
            if found_count >= limit {
                break;
            }

            processed_count += 1;
            if processed_count % 50000 == 0 {
                println!(
                    "  {} Processed {} IDs...",
                    style("→").dim(),
                    processed_count
                );
            }

            // Try to resolve this FileDataID
            if let Ok(Some(file_path)) = orchestrator.resolve_file_path(file_data_id) {
                let mut file_info = FileInfo::new(file_data_id, file_path.clone());

                // Apply filters
                if !matches_filters(&file_info, filter, regex_pattern.as_ref()) {
                    continue;
                }

                // Add metadata if needed
                if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                    file_info.requires_encryption = content_info.requires_encryption;
                    file_info.compression_level = content_info.compression_level;
                }

                results.push(file_info);
                found_count += 1;
            }
        }

        if found_count >= limit {
            break;
        }
    }

    Ok(results)
}

/// Check if a file matches the specified filters
fn matches_filters(
    file_info: &FileInfo,
    filter: &BrowseFilter,
    regex_pattern: Option<&Regex>,
) -> bool {
    // Extension filter
    if let Some(ref ext) = filter.extension {
        let file_ext = std::path::Path::new(&file_info.path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);

        let target_ext = ext.trim_start_matches('.').to_lowercase();
        if file_ext.as_ref() != Some(&target_ext) {
            return false;
        }
    }

    // Path pattern filter
    if let Some(regex) = regex_pattern {
        if !regex.is_match(&file_info.path) {
            return false;
        }
    }

    // Category filter
    if let Some(ref target_category) = filter.category {
        if &file_info.category != target_category {
            return false;
        }
    }

    true
}

/// Display browse results in the specified format
fn display_browse_results(
    files: &[FileInfo],
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let json_results: Vec<_> = files
                .iter()
                .map(|file_info| {
                    let mut obj = json!({
                        "file_data_id": file_info.file_data_id,
                        "path": file_info.path,
                        "category": file_info.category.to_string()
                    });

                    if show_metadata {
                        obj["requires_encryption"] = json!(file_info.requires_encryption);
                        obj["compression_level"] = json!(file_info.compression_level);
                    }
                    obj
                })
                .collect();

            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        OutputFormat::Csv => {
            if show_metadata {
                println!("FileDataID,Path,Category,RequiresEncryption,CompressionLevel");
            } else {
                println!("FileDataID,Path,Category");
            }

            for file_info in files {
                if show_metadata {
                    println!(
                        "{},\"{}\",{},{},{}",
                        file_info.file_data_id,
                        file_info.path,
                        file_info.category,
                        file_info.requires_encryption,
                        file_info.compression_level
                    );
                } else {
                    println!(
                        "{},\"{}\",{}",
                        file_info.file_data_id, file_info.path, file_info.category
                    );
                }
            }
        }
        OutputFormat::Table => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);

            if show_metadata {
                table.set_header(vec![
                    "FileDataID",
                    "Path",
                    "Category",
                    "Encrypted",
                    "Compression",
                ]);
            } else {
                table.set_header(vec!["FileDataID", "Path", "Category"]);
            }

            for file_info in files {
                if show_metadata {
                    table.add_row(vec![
                        file_info.file_data_id.to_string(),
                        truncate_path(&file_info.path, 60),
                        file_info.category.to_string(),
                        file_info.requires_encryption.to_string(),
                        file_info.compression_level.to_string(),
                    ]);
                } else {
                    table.add_row(vec![
                        file_info.file_data_id.to_string(),
                        truncate_path(&file_info.path, 80),
                        file_info.category.to_string(),
                    ]);
                }
            }

            println!("{}", table);

            // Show category distribution summary
            show_category_distribution(files);
        }
    }

    Ok(())
}

/// Show a summary of file categories found
fn show_category_distribution(files: &[FileInfo]) {
    let mut category_counts: HashMap<String, usize> = HashMap::new();

    for file_info in files {
        *category_counts
            .entry(file_info.category.to_string())
            .or_insert(0) += 1;
    }

    if category_counts.len() > 1 {
        println!("\n{}", style("Category Distribution:").cyan().bold());
        let mut categories: Vec<_> = category_counts.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count descending

        for (category, count) in categories.iter().take(10) {
            let percentage = (*count as f64 / files.len() as f64) * 100.0;
            println!(
                "  {} {} ({:.1}%)",
                style(format!("{:12}", category)).cyan(),
                style(count).yellow(),
                percentage
            );
        }

        if categories.len() > 10 {
            println!(
                "  {} ... and {} more categories",
                style("").dim(),
                categories.len() - 10
            );
        }
    }
}

/// Truncate long paths for table display
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else if path.split(['\\', '/']).count() > 2 {
        format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    } else {
        format!("{}...", &path[..max_len.saturating_sub(3)])
    }
}

/// Parse category from string
pub fn parse_category(category_str: &str) -> FileCategory {
    match category_str.to_lowercase().as_str() {
        "model" => FileCategory::Model,
        "texture" => FileCategory::Texture,
        "audio" => FileCategory::Audio,
        "music" => FileCategory::Music,
        "video" => FileCategory::Video,
        "database" => FileCategory::Database,
        "interface" => FileCategory::Interface,
        "map" => FileCategory::Map,
        "animation" => FileCategory::Animation,
        "shader" => FileCategory::Shader,
        "script" => FileCategory::Script,
        "configuration" => FileCategory::Configuration,
        "font" => FileCategory::Font,
        "unknown" => FileCategory::Unknown,
        other => FileCategory::Other(other.to_uppercase()),
    }
}
