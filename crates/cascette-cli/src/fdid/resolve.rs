//! FileDataID resolve command implementation
//!
//! Provides bidirectional resolution between FileDataIDs and file paths with
//! support for batch processing, multiple output formats, and metadata display.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use comfy_table::{Table, presets::UTF8_FULL};
use console::style;
use serde_json::json;

use super::{FileInfo, OutputFormat};

/// Execute the FileDataID resolve command
pub async fn execute_resolve(
    orchestrator: &mut MetadataOrchestrator,
    ids: Option<String>,
    paths: Option<String>,
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    // Ensure mappings are loaded
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    if let Some(id_str) = ids {
        resolve_ids_to_paths(orchestrator, &id_str, format, show_metadata).await
    } else if let Some(path_str) = paths {
        resolve_paths_to_ids(orchestrator, &path_str, format, show_metadata)
    } else {
        anyhow::bail!("Please specify either --id or --path");
    }
}

/// Resolve FileDataIDs to file paths
async fn resolve_ids_to_paths(
    orchestrator: &mut MetadataOrchestrator,
    id_str: &str,
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    // Parse comma-separated IDs
    let ids: Result<Vec<u32>, _> = id_str.split(',').map(|s| s.trim().parse::<u32>()).collect();

    let ids = ids.context("Invalid FileDataID format")?;

    let mut results = Vec::new();
    for &file_data_id in &ids {
        // Use async resolution to support lazy loading from provider
        if let Ok(Some(file_path)) = orchestrator.resolve_file_path_async(file_data_id).await {
            let mut file_info = FileInfo::new(file_data_id, file_path);

            // Add metadata if requested
            if show_metadata {
                if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                    file_info.requires_encryption = content_info.requires_encryption;
                    file_info.compression_level = content_info.compression_level;
                }
            }
            results.push(Some(file_info));
        } else {
            results.push(None);
        }
    }

    output_id_to_path_results(&results, &ids, format, show_metadata)
}

/// Resolve file paths to FileDataIDs
fn resolve_paths_to_ids(
    orchestrator: &mut MetadataOrchestrator,
    path_str: &str,
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    // Parse comma-separated paths (handle paths with commas in quotes)
    let paths = parse_comma_separated_paths(path_str);

    let mut results = Vec::new();
    for path in &paths {
        match orchestrator.resolve_file_data_id(path) {
            Ok(Some(file_data_id)) => {
                let mut file_info = FileInfo::new(file_data_id, path.clone());

                // Add metadata if requested
                if show_metadata {
                    if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                        file_info.requires_encryption = content_info.requires_encryption;
                        file_info.compression_level = content_info.compression_level;
                    }
                }
                results.push(Some(file_info));
            }
            Ok(None) => results.push(None),
            Err(_) => results.push(None),
        }
    }

    output_path_to_id_results(&results, &paths, format, show_metadata)
}

/// Parse comma-separated paths, handling quoted strings
fn parse_comma_separated_paths(input: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars = input.chars();

    for ch in chars {
        match ch {
            '"' if !in_quotes => in_quotes = true,
            '"' if in_quotes => in_quotes = false,
            ',' if !in_quotes => {
                if !current.trim().is_empty() {
                    paths.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        paths.push(current.trim().to_string());
    }

    paths
}

/// Output results for ID to path resolution
fn output_id_to_path_results(
    results: &[Option<FileInfo>],
    original_ids: &[u32],
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let json_results: Vec<_> = results
                .iter()
                .zip(original_ids.iter())
                .map(|(result, &id)| {
                    if let Some(file_info) = result {
                        let mut obj = json!({
                            "file_data_id": file_info.file_data_id,
                            "path": file_info.path
                        });

                        if show_metadata {
                            obj["category"] = json!(file_info.category.to_string());
                            obj["requires_encryption"] = json!(file_info.requires_encryption);
                            obj["compression_level"] = json!(file_info.compression_level);
                        }
                        obj
                    } else {
                        json!({
                            "file_data_id": id,
                            "path": null,
                            "error": "Not found"
                        })
                    }
                })
                .collect();

            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        OutputFormat::Csv => {
            if show_metadata {
                println!("FileDataID,Path,Category,RequiresEncryption,CompressionLevel");
            } else {
                println!("FileDataID,Path");
            }

            for (result, &id) in results.iter().zip(original_ids.iter()) {
                if let Some(file_info) = result {
                    if show_metadata {
                        println!(
                            "{},{},{},{},{}",
                            file_info.file_data_id,
                            file_info.path,
                            file_info.category,
                            file_info.requires_encryption,
                            file_info.compression_level
                        );
                    } else {
                        println!("{},{}", file_info.file_data_id, file_info.path);
                    }
                } else if show_metadata {
                    println!("{},\"[Not Found]\",Unknown,false,0", id);
                } else {
                    println!("{},\"[Not Found]\"", id);
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
                table.set_header(vec!["FileDataID", "Path"]);
            }

            for (result, &id) in results.iter().zip(original_ids.iter()) {
                if let Some(file_info) = result {
                    if show_metadata {
                        table.add_row(vec![
                            file_info.file_data_id.to_string(),
                            file_info.path.clone(),
                            file_info.category.to_string(),
                            file_info.requires_encryption.to_string(),
                            file_info.compression_level.to_string(),
                        ]);
                    } else {
                        table.add_row(vec![
                            file_info.file_data_id.to_string(),
                            file_info.path.clone(),
                        ]);
                    }
                } else if show_metadata {
                    table.add_row(vec![
                        id.to_string(),
                        style("[Not Found]").red().to_string(),
                        "-".to_string(),
                        "-".to_string(),
                        "-".to_string(),
                    ]);
                } else {
                    table.add_row(vec![id.to_string(), style("[Not Found]").red().to_string()]);
                }
            }

            println!("{}", table);
        }
    }

    Ok(())
}

/// Output results for path to ID resolution
fn output_path_to_id_results(
    results: &[Option<FileInfo>],
    original_paths: &[String],
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let json_results: Vec<_> = results
                .iter()
                .zip(original_paths.iter())
                .map(|(result, path)| {
                    if let Some(file_info) = result {
                        let mut obj = json!({
                            "path": file_info.path,
                            "file_data_id": file_info.file_data_id
                        });

                        if show_metadata {
                            obj["category"] = json!(file_info.category.to_string());
                            obj["requires_encryption"] = json!(file_info.requires_encryption);
                            obj["compression_level"] = json!(file_info.compression_level);
                        }
                        obj
                    } else {
                        json!({
                            "path": path,
                            "file_data_id": null,
                            "error": "Not found"
                        })
                    }
                })
                .collect();

            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        OutputFormat::Csv => {
            if show_metadata {
                println!("Path,FileDataID,Category,RequiresEncryption,CompressionLevel");
            } else {
                println!("Path,FileDataID");
            }

            for (result, path) in results.iter().zip(original_paths.iter()) {
                if let Some(file_info) = result {
                    if show_metadata {
                        println!(
                            "\"{}\",{},{},{},{}",
                            file_info.path,
                            file_info.file_data_id,
                            file_info.category,
                            file_info.requires_encryption,
                            file_info.compression_level
                        );
                    } else {
                        println!("\"{}\",{}", file_info.path, file_info.file_data_id);
                    }
                } else if show_metadata {
                    println!("\"{}\",,Unknown,false,0", path);
                } else {
                    println!("\"{}\"", path);
                }
            }
        }
        OutputFormat::Table => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);

            if show_metadata {
                table.set_header(vec![
                    "Path",
                    "FileDataID",
                    "Category",
                    "Encrypted",
                    "Compression",
                ]);
            } else {
                table.set_header(vec!["Path", "FileDataID"]);
            }

            for (result, path) in results.iter().zip(original_paths.iter()) {
                if let Some(file_info) = result {
                    if show_metadata {
                        table.add_row(vec![
                            file_info.path.clone(),
                            file_info.file_data_id.to_string(),
                            file_info.category.to_string(),
                            file_info.requires_encryption.to_string(),
                            file_info.compression_level.to_string(),
                        ]);
                    } else {
                        table.add_row(vec![
                            file_info.path.clone(),
                            file_info.file_data_id.to_string(),
                        ]);
                    }
                } else if show_metadata {
                    table.add_row(vec![
                        path.clone(),
                        style("[Not Found]").red().to_string(),
                        "-".to_string(),
                        "-".to_string(),
                        "-".to_string(),
                    ]);
                } else {
                    table.add_row(vec![path.clone(), style("[Not Found]").red().to_string()]);
                }
            }

            println!("{}", table);
        }
    }

    Ok(())
}
