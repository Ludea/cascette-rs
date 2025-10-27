//! FileDataID batch command implementation
//!
//! Provides bulk processing capabilities for FileDataID operations, supporting
//! input from files, stdin, and various output formats with progress tracking.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use comfy_table::{Table, presets::UTF8_FULL};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

use super::{FileInfo, OutputFormat};

/// Batch operation types
#[derive(Copy, Clone, Debug)]
pub enum BatchOperation {
    ResolveIds,
    ResolvePaths,
    ValidateIds,
    ValidatePaths,
}

/// Batch processing options
#[derive(Default)]
pub struct BatchOptions {
    pub show_progress: bool,
    pub show_metadata: bool,
    pub continue_on_error: bool,
    pub output_file: Option<String>,
}

/// Batch processing result
#[derive(Clone, Debug)]
pub struct BatchResult {
    pub successful: usize,
    pub failed: usize,
    pub total: usize,
    pub results: Vec<BatchItem>,
}

/// Individual batch item result
#[derive(Clone, Debug)]
pub struct BatchItem {
    pub input: String,
    pub file_info: Option<FileInfo>,
    pub error: Option<String>,
}

/// Execute the FileDataID batch command
pub async fn execute_batch(
    orchestrator: &mut MetadataOrchestrator,
    operation: BatchOperation,
    input_source: BatchInputSource,
    options: BatchOptions,
    format: OutputFormat,
) -> Result<()> {
    // Load mappings for batch operations
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    // Read input data
    let input_items = read_batch_input(input_source)?;

    if input_items.is_empty() {
        println!("{} No input items to process", style("⚠").yellow());
        return Ok(());
    }

    println!(
        "{} Processing {} items in batch mode...",
        style("→").dim(),
        style(input_items.len()).yellow()
    );

    // Process batch
    let result = process_batch(orchestrator, operation, input_items, &options);

    // Display results
    display_batch_results(&result, format, &options)?;

    // Show summary
    show_batch_summary(&result);

    Ok(())
}

/// Input source for batch operations
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum BatchInputSource {
    File(String),
    Stdin,
    Items(Vec<String>),
}

/// Read input items from the specified source
fn read_batch_input(source: BatchInputSource) -> Result<Vec<String>> {
    match source {
        BatchInputSource::File(file_path) => {
            let file = File::open(&file_path)
                .with_context(|| format!("Failed to open input file: {}", file_path))?;
            let reader = BufReader::new(file);
            let mut items = Vec::new();

            for (line_num, line) in reader.lines().enumerate() {
                let line = line.with_context(|| format!("Failed to read line {}", line_num + 1))?;
                let trimmed = line.trim();

                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    items.push(trimmed.to_string());
                }
            }

            Ok(items)
        }
        BatchInputSource::Stdin => {
            println!(
                "{} Reading from stdin (press Ctrl+D to finish)...",
                style("→").dim()
            );
            let stdin = io::stdin();
            let mut items = Vec::new();

            for line in stdin.lock().lines() {
                let line = line.context("Failed to read from stdin")?;
                let trimmed = line.trim();

                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    items.push(trimmed.to_string());
                }
            }

            Ok(items)
        }
        BatchInputSource::Items(items) => Ok(items),
    }
}

/// Process a batch of items
fn process_batch(
    orchestrator: &MetadataOrchestrator,
    operation: BatchOperation,
    input_items: Vec<String>,
    options: &BatchOptions,
) -> BatchResult {
    let total = input_items.len();
    let mut results = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    // Set up progress bar if requested
    let progress = if options.show_progress {
        let pb = ProgressBar::new(total as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{bar:40.cyan/blue} {pos}/{len} {msg} [{elapsed_precise}]")
                .expect("Valid progress template")
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        Some(pb)
    } else {
        None
    };

    // Process each item
    for (index, input_item) in input_items.into_iter().enumerate() {
        if let Some(ref pb) = progress {
            pb.set_message(format!("Processing: {}", truncate_string(&input_item, 30)));
        }

        let batch_item = process_single_item(orchestrator, operation, &input_item, options);

        match batch_item.error {
            None => successful += 1,
            Some(_) => {
                failed += 1;
                if !options.continue_on_error {
                    if let Some(ref pb) = progress {
                        pb.abandon_with_message(format!(
                            "{} Stopped on error at item {}",
                            style("✗").red(),
                            index + 1
                        ));
                    }
                    results.push(batch_item);
                    break;
                }
            }
        }

        results.push(batch_item);

        if let Some(ref pb) = progress {
            pb.inc(1);
        }
    }

    if let Some(pb) = progress {
        pb.finish_with_message(format!("{} Batch processing completed", style("✓").green()));
    }

    BatchResult {
        successful,
        failed,
        total,
        results,
    }
}

/// Process a single batch item
fn process_single_item(
    orchestrator: &MetadataOrchestrator,
    operation: BatchOperation,
    input: &str,
    options: &BatchOptions,
) -> BatchItem {
    let result = match operation {
        BatchOperation::ResolveIds => {
            if let Ok(file_data_id) = input.parse::<u32>() {
                match orchestrator.resolve_file_path(file_data_id) {
                    Ok(Some(path)) => {
                        let mut file_info = FileInfo::new(file_data_id, path);

                        if options.show_metadata {
                            if let Ok(Some(content_info)) =
                                orchestrator.get_content_info(file_data_id)
                            {
                                file_info.requires_encryption = content_info.requires_encryption;
                                file_info.compression_level = content_info.compression_level;
                            }
                        }

                        Ok(Some(file_info))
                    }
                    Ok(None) => Err("FileDataID not found".to_string()),
                    Err(e) => Err(format!("Resolution error: {}", e)),
                }
            } else {
                Err("Invalid FileDataID format".to_string())
            }
        }
        BatchOperation::ResolvePaths => match orchestrator.resolve_file_data_id(input) {
            Ok(Some(file_data_id)) => {
                let mut file_info = FileInfo::new(file_data_id, input.to_string());

                if options.show_metadata {
                    if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                        file_info.requires_encryption = content_info.requires_encryption;
                        file_info.compression_level = content_info.compression_level;
                    }
                }

                Ok(Some(file_info))
            }
            Ok(None) => Err("Path not found".to_string()),
            Err(e) => Err(format!("Resolution error: {}", e)),
        },
        BatchOperation::ValidateIds => {
            if let Ok(file_data_id) = input.parse::<u32>() {
                match orchestrator.resolve_file_path(file_data_id) {
                    Ok(Some(_)) => Ok(None), // Valid ID
                    Ok(None) => Err("FileDataID not found".to_string()),
                    Err(e) => Err(format!("Validation error: {}", e)),
                }
            } else {
                Err("Invalid FileDataID format".to_string())
            }
        }
        BatchOperation::ValidatePaths => {
            match orchestrator.resolve_file_data_id(input) {
                Ok(Some(_)) => Ok(None), // Valid path
                Ok(None) => Err("Path not found".to_string()),
                Err(e) => Err(format!("Validation error: {}", e)),
            }
        }
    };

    match result {
        Ok(file_info) => BatchItem {
            input: input.to_string(),
            file_info,
            error: None,
        },
        Err(error) => BatchItem {
            input: input.to_string(),
            file_info: None,
            error: Some(error),
        },
    }
}

/// Display batch processing results
fn display_batch_results(
    result: &BatchResult,
    format: OutputFormat,
    options: &BatchOptions,
) -> Result<()> {
    let output: Box<dyn Write> = if let Some(ref output_file) = options.output_file {
        Box::new(
            File::create(output_file)
                .with_context(|| format!("Failed to create output file: {}", output_file))?,
        )
    } else {
        Box::new(io::stdout())
    };

    let mut writer = io::BufWriter::new(output);

    match format {
        OutputFormat::Json => {
            let json_results: Vec<_> = result
                .results
                .iter()
                .map(|item| {
                    let mut obj = json!({
                        "input": item.input
                    });

                    if let Some(ref file_info) = item.file_info {
                        obj["file_data_id"] = json!(file_info.file_data_id);
                        obj["path"] = json!(file_info.path);
                        obj["category"] = json!(file_info.category.to_string());

                        if options.show_metadata {
                            obj["requires_encryption"] = json!(file_info.requires_encryption);
                            obj["compression_level"] = json!(file_info.compression_level);
                        }
                    }

                    if let Some(ref error) = item.error {
                        obj["error"] = json!(error);
                    }

                    obj
                })
                .collect();

            writeln!(writer, "{}", serde_json::to_string_pretty(&json_results)?)?;
        }
        OutputFormat::Csv => {
            // Write header
            if options.show_metadata {
                writeln!(
                    writer,
                    "Input,FileDataID,Path,Category,RequiresEncryption,CompressionLevel,Error"
                )?;
            } else {
                writeln!(writer, "Input,FileDataID,Path,Category,Error")?;
            }

            // Write data
            for item in &result.results {
                if let Some(ref file_info) = item.file_info {
                    if options.show_metadata {
                        writeln!(
                            writer,
                            "\"{}\",{},\"{}\",{},{},{}",
                            item.input,
                            file_info.file_data_id,
                            file_info.path,
                            file_info.category,
                            file_info.requires_encryption,
                            file_info.compression_level
                        )?;
                    } else {
                        writeln!(
                            writer,
                            "\"{}\",{},\"{}\",{}",
                            item.input, file_info.file_data_id, file_info.path, file_info.category
                        )?;
                    }
                } else if let Some(ref error) = item.error {
                    if options.show_metadata {
                        writeln!(writer, "\"{}\",,,,,,\"{}\"", item.input, error)?;
                    } else {
                        writeln!(writer, "\"{}\",,,,\"{}\"", item.input, error)?;
                    }
                }
            }
        }
        OutputFormat::Table => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);

            if options.show_metadata {
                table.set_header(vec![
                    "Input",
                    "FileDataID",
                    "Path",
                    "Category",
                    "Encrypted",
                    "Compression",
                    "Status",
                ]);
            } else {
                table.set_header(vec!["Input", "FileDataID", "Path", "Category", "Status"]);
            }

            for item in &result.results {
                if let Some(ref file_info) = item.file_info {
                    if options.show_metadata {
                        table.add_row(vec![
                            truncate_string(&item.input, 20),
                            file_info.file_data_id.to_string(),
                            truncate_string(&file_info.path, 40),
                            file_info.category.to_string(),
                            file_info.requires_encryption.to_string(),
                            file_info.compression_level.to_string(),
                            style("OK").green().to_string(),
                        ]);
                    } else {
                        table.add_row(vec![
                            truncate_string(&item.input, 25),
                            file_info.file_data_id.to_string(),
                            truncate_string(&file_info.path, 50),
                            file_info.category.to_string(),
                            style("OK").green().to_string(),
                        ]);
                    }
                } else if let Some(ref error) = item.error {
                    if options.show_metadata {
                        table.add_row(vec![
                            truncate_string(&item.input, 20),
                            "-".to_string(),
                            "-".to_string(),
                            "-".to_string(),
                            "-".to_string(),
                            "-".to_string(),
                            style(truncate_string(error, 20)).red().to_string(),
                        ]);
                    } else {
                        table.add_row(vec![
                            truncate_string(&item.input, 25),
                            "-".to_string(),
                            "-".to_string(),
                            "-".to_string(),
                            style(truncate_string(error, 25)).red().to_string(),
                        ]);
                    }
                }
            }

            writeln!(writer, "{}", table)?;
        }
    }

    writer.flush()?;
    Ok(())
}

/// Show batch processing summary
fn show_batch_summary(result: &BatchResult) {
    println!("\n{}", style("Batch Processing Summary").cyan().bold());
    println!("{}", style("─".repeat(50)).dim());

    println!(
        "{} {}",
        style("Total processed:").bold(),
        style(result.total).yellow()
    );
    println!(
        "{} {}",
        style("Successful:").bold(),
        style(result.successful).green()
    );

    if result.failed > 0 {
        println!("{} {}", style("Failed:").bold(), style(result.failed).red());

        let success_rate = (result.successful as f64 / result.total as f64) * 100.0;
        println!("{} {:.1}%", style("Success rate:").bold(), success_rate);
    } else {
        println!(
            "{} {}",
            style("Success rate:").bold(),
            style("100.0%").green()
        );
    }
}

/// Truncate strings for display
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
