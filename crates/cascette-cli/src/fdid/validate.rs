//! FileDataID validate command implementation
//!
//! Provides consistency checking and validation for FileDataID mappings including
//! orphaned entries detection, missing mappings identification, and integrity verification.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use comfy_table::{Table, presets::UTF8_FULL};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use super::OutputFormat;

/// Validation options
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct ValidationOptions {
    pub check_orphans: bool,
    pub check_duplicates: bool,
    pub check_path_format: bool,
    pub sample_size: Option<usize>,
    pub auto_fix: bool,
}

/// Validation results
#[derive(Debug)]
pub struct ValidationResults {
    pub total_checked: usize,
    pub valid_mappings: usize,
    pub orphaned_ids: Vec<u32>,
    pub duplicate_paths: HashMap<String, Vec<u32>>,
    pub invalid_paths: Vec<InvalidPath>,
    pub missing_ranges: Vec<(u32, u32)>,
    pub integrity_issues: Vec<IntegrityIssue>,
}

/// Invalid path entry
#[derive(Debug)]
pub struct InvalidPath {
    pub file_data_id: u32,
    pub path: String,
    pub issue: String,
}

/// Integrity issue
#[derive(Debug)]
pub struct IntegrityIssue {
    pub issue_type: String,
    pub description: String,
    pub affected_ids: Vec<u32>,
    pub severity: IssueSeverity,
}

/// Issue severity levels
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum IssueSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Execute the FileDataID validate command
pub async fn execute_validate(
    orchestrator: &mut MetadataOrchestrator,
    options: ValidationOptions,
    format: OutputFormat,
) -> Result<()> {
    // Load mappings for validation
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    println!("{} Starting validation process...", style("→").dim());

    // Perform validation
    let results = perform_validation(orchestrator, &options)?;

    // Display results
    display_validation_results(&results, format)?;

    // Show summary
    show_validation_summary(&results, &options);

    Ok(())
}

/// Perform comprehensive validation of FileDataID mappings
fn perform_validation(
    orchestrator: &mut MetadataOrchestrator,
    options: &ValidationOptions,
) -> Result<ValidationResults> {
    let mut results = ValidationResults {
        total_checked: 0,
        valid_mappings: 0,
        orphaned_ids: Vec::new(),
        duplicate_paths: HashMap::new(),
        invalid_paths: Vec::new(),
        missing_ranges: Vec::new(),
        integrity_issues: Vec::new(),
    };

    // Set up progress tracking
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("Valid spinner template")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );

    // Track paths for duplicate detection
    let mut path_to_ids: HashMap<String, Vec<u32>> = HashMap::new();
    let mut found_ids: HashSet<u32> = HashSet::new();

    let sample_size = options.sample_size.unwrap_or(500_000);
    let check_step = std::cmp::max(1, 3_000_000 / sample_size);

    println!(
        "  {} Checking mappings with step size: {}",
        style("→").dim(),
        check_step
    );

    // Check mappings for various issues
    for file_data_id in (1..=3_000_000_u32).step_by(check_step) {
        results.total_checked += 1;

        if results.total_checked % 10000 == 0 {
            progress.set_message(format!(
                "Validated {} mappings, found {} issues...",
                results.total_checked,
                results.orphaned_ids.len() + results.invalid_paths.len()
            ));
        }

        match orchestrator.resolve_file_path(file_data_id) {
            Ok(Some(file_path)) => {
                results.valid_mappings += 1;
                found_ids.insert(file_data_id);

                // Check for duplicate paths
                if options.check_duplicates {
                    path_to_ids
                        .entry(file_path.clone())
                        .or_default()
                        .push(file_data_id);
                }

                // Validate path format
                if options.check_path_format {
                    if let Some(issue) = validate_path_format(&file_path) {
                        results.invalid_paths.push(InvalidPath {
                            file_data_id,
                            path: file_path,
                            issue,
                        });
                    }
                }
            }
            Ok(None) => {
                // This is expected - not all IDs have mappings
            }
            Err(_) => {
                // This could indicate an integrity issue
                if options.check_orphans {
                    results.orphaned_ids.push(file_data_id);
                }
            }
        }

        if results.total_checked % 1000 == 0 {
            progress.tick();
        }
    }

    progress.finish_and_clear();

    // Process duplicate paths
    if options.check_duplicates {
        for (path, ids) in path_to_ids {
            if ids.len() > 1 {
                results.duplicate_paths.insert(path, ids);
            }
        }
    }

    // Find missing ID ranges (gaps in the ID space)
    find_missing_ranges(&found_ids, &mut results);

    // Perform integrity checks
    perform_integrity_checks(orchestrator, &mut results)?;

    Ok(results)
}

/// Validate path format for common issues
fn validate_path_format(path: &str) -> Option<String> {
    // Check for invalid characters
    if path.contains('\0') {
        return Some("Contains null character".to_string());
    }

    // Check for path traversal attempts
    if path.contains("..") {
        return Some("Contains path traversal sequence".to_string());
    }

    // Check for extremely long paths
    if path.len() > 260 {
        return Some("Path too long (>260 characters)".to_string());
    }

    // Check for invalid Windows filename characters
    let invalid_chars = ['<', '>', ':', '"', '|', '?', '*'];
    for &ch in &invalid_chars {
        if path.contains(ch) {
            return Some(format!("Contains invalid character: '{}'", ch));
        }
    }

    // Check for paths ending with space or period (Windows issue)
    if path.ends_with(' ') || path.ends_with('.') {
        return Some("Path ends with space or period".to_string());
    }

    // Check for empty path segments
    if path.contains("\\\\") || path.contains("//") {
        return Some("Contains empty path segments".to_string());
    }

    None
}

/// Find missing ranges in the ID space
fn find_missing_ranges(found_ids: &HashSet<u32>, results: &mut ValidationResults) {
    let mut ranges = Vec::new();
    let sorted_ids: Vec<u32> = {
        let mut ids: Vec<_> = found_ids.iter().copied().collect();
        ids.sort_unstable();
        ids
    };

    if sorted_ids.is_empty() {
        return;
    }

    let mut range_start: Option<u32> = None;
    let mut last_id = sorted_ids[0];

    for &current_id in sorted_ids.iter().skip(1) {
        let gap = current_id - last_id;

        if gap > 100 {
            // Only report significant gaps
            if range_start.is_none() {
                range_start = Some(last_id + 1);
            }

            if let Some(start) = range_start {
                ranges.push((start, current_id - 1));
                range_start = None;
            }
        }

        last_id = current_id;
    }

    // Only keep the largest gaps to avoid noise
    ranges.sort_by_key(|(start, end)| std::cmp::Reverse(end - start));
    results.missing_ranges = ranges.into_iter().take(10).collect();
}

/// Perform integrity checks on the data
fn perform_integrity_checks(
    orchestrator: &mut MetadataOrchestrator,
    results: &mut ValidationResults,
) -> Result<()> {
    // Check for metadata consistency
    let mut metadata_issues = 0;

    // Sample some known IDs for metadata validation
    let test_ids = [1, 100, 1000, 10000, 100_000];

    for &test_id in &test_ids {
        if let Ok(Some(_path)) = orchestrator.resolve_file_path(test_id) {
            match orchestrator.get_content_info(test_id) {
                Ok(Some(_content_info)) => {
                    // Content info is available - good
                }
                Ok(None) => {
                    metadata_issues += 1;
                }
                Err(_) => {
                    metadata_issues += 1;
                }
            }
        }
    }

    if metadata_issues > 0 {
        results.integrity_issues.push(IntegrityIssue {
            issue_type: "Metadata Inconsistency".to_string(),
            description: format!(
                "{} out of {} test IDs have missing or invalid metadata",
                metadata_issues,
                test_ids.len()
            ),
            affected_ids: test_ids.to_vec(),
            severity: IssueSeverity::Medium,
        });
    }

    // Check for service health
    let stats = orchestrator
        .get_stats()
        .context("Failed to get orchestrator stats")?;
    let fdid_stats = stats.fdid_stats;

    if fdid_stats.failed_lookups > fdid_stats.successful_lookups {
        results.integrity_issues.push(IntegrityIssue {
            issue_type: "High Failure Rate".to_string(),
            description: format!(
                "Failed lookups ({}) exceed successful lookups ({})",
                fdid_stats.failed_lookups, fdid_stats.successful_lookups
            ),
            affected_ids: Vec::new(),
            severity: IssueSeverity::High,
        });
    }

    // Check memory usage
    let memory_mb = fdid_stats.memory_usage_bytes as f64 / (1024.0 * 1024.0);
    if memory_mb > 500.0 {
        results.integrity_issues.push(IntegrityIssue {
            issue_type: "High Memory Usage".to_string(),
            description: format!(
                "Memory usage ({:.1} MB) is higher than recommended threshold (500 MB)",
                memory_mb
            ),
            affected_ids: Vec::new(),
            severity: IssueSeverity::Medium,
        });
    }

    Ok(())
}

/// Display validation results
fn display_validation_results(results: &ValidationResults, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let json_results = json!({
                "summary": {
                    "total_checked": results.total_checked,
                    "valid_mappings": results.valid_mappings,
                    "orphaned_ids": results.orphaned_ids.len(),
                    "duplicate_paths": results.duplicate_paths.len(),
                    "invalid_paths": results.invalid_paths.len(),
                    "missing_ranges": results.missing_ranges.len(),
                    "integrity_issues": results.integrity_issues.len()
                },
                "orphaned_ids": results.orphaned_ids,
                "duplicate_paths": results.duplicate_paths,
                "invalid_paths": results.invalid_paths.iter()
                    .map(|p| json!({
                        "file_data_id": p.file_data_id,
                        "path": p.path,
                        "issue": p.issue
                    }))
                    .collect::<Vec<_>>(),
                "missing_ranges": results.missing_ranges,
                "integrity_issues": results.integrity_issues.iter()
                    .map(|issue| json!({
                        "type": issue.issue_type,
                        "description": issue.description,
                        "severity": format!("{:?}", issue.severity),
                        "affected_ids": issue.affected_ids
                    }))
                    .collect::<Vec<_>>()
            });
            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        OutputFormat::Csv => {
            println!("IssueType,FileDataID,Path,Description,Severity");

            for id in &results.orphaned_ids {
                println!("Orphaned,{},,Orphaned FileDataID,Medium", id);
            }

            for (path, ids) in &results.duplicate_paths {
                for id in ids {
                    println!("Duplicate,{},\"{}\",Duplicate path mapping,Low", id, path);
                }
            }

            for invalid in &results.invalid_paths {
                println!(
                    "InvalidPath,{},\"{}\",\"{}\",Medium",
                    invalid.file_data_id, invalid.path, invalid.issue
                );
            }
        }
        OutputFormat::Table => {
            display_validation_table(results);
        }
    }

    Ok(())
}

/// Display validation results in table format
fn display_validation_table(results: &ValidationResults) {
    println!("{}", style("FileDataID Validation Report").cyan().bold());
    println!("{}", style("═".repeat(60)).dim());

    // Overview
    println!("\n{}", style("VALIDATION SUMMARY").yellow().bold());
    println!(
        "  {} {}",
        style("Total Checked:").bold(),
        style(results.total_checked).green()
    );
    println!(
        "  {} {}",
        style("Valid Mappings:").bold(),
        style(results.valid_mappings).green()
    );

    let error_count =
        results.orphaned_ids.len() + results.invalid_paths.len() + results.integrity_issues.len();

    if error_count == 0 {
        println!(
            "  {} {}",
            style("Issues Found:").bold(),
            style("None").green()
        );
    } else {
        println!(
            "  {} {}",
            style("Issues Found:").bold(),
            style(error_count).red()
        );
    }

    // Orphaned IDs
    if !results.orphaned_ids.is_empty() {
        println!("\n{}", style("ORPHANED FILEDATAIDS").yellow().bold());
        println!(
            "  {} {} orphaned IDs detected",
            style("Count:").bold(),
            results.orphaned_ids.len()
        );

        if results.orphaned_ids.len() <= 20 {
            print!("  {} ", style("IDs:").dim());
            for (i, id) in results.orphaned_ids.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                print!("{}", id);
            }
            println!();
        } else {
            println!(
                "  {} {} ... {} (showing first/last)",
                style("Range:").dim(),
                results.orphaned_ids[0],
                results.orphaned_ids[results.orphaned_ids.len() - 1]
            );
        }
    }

    // Duplicate paths
    if !results.duplicate_paths.is_empty() {
        println!("\n{}", style("DUPLICATE PATHS").yellow().bold());
        println!(
            "  {} {} paths have multiple IDs",
            style("Count:").bold(),
            results.duplicate_paths.len()
        );

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["Path", "FileDataIDs", "Count"]);

        for (path, ids) in results.duplicate_paths.iter().take(10) {
            let ids_str = if ids.len() <= 5 {
                ids.iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                format!("{}, {} ... ({} total)", ids[0], ids[1], ids.len())
            };

            table.add_row(vec![
                truncate_path(path, 40),
                ids_str,
                ids.len().to_string(),
            ]);
        }

        println!("{}", table);
    }

    // Invalid paths
    if !results.invalid_paths.is_empty() {
        println!("\n{}", style("INVALID PATHS").yellow().bold());
        println!(
            "  {} {} paths have format issues",
            style("Count:").bold(),
            results.invalid_paths.len()
        );

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["FileDataID", "Path", "Issue"]);

        for invalid in results.invalid_paths.iter().take(10) {
            table.add_row(vec![
                invalid.file_data_id.to_string(),
                truncate_path(&invalid.path, 40),
                invalid.issue.clone(),
            ]);
        }

        println!("{}", table);
    }

    // Missing ranges
    if !results.missing_ranges.is_empty() {
        println!("\n{}", style("MISSING ID RANGES").yellow().bold());
        println!(
            "  {} Large gaps in FileDataID space:",
            style("Count:").bold()
        );

        for (start, end) in results.missing_ranges.iter().take(5) {
            let gap_size = end - start + 1;
            println!(
                "    {} {} - {} ({} IDs)",
                style("•").dim(),
                start,
                end,
                gap_size
            );
        }
    }

    // Integrity issues
    if !results.integrity_issues.is_empty() {
        println!("\n{}", style("INTEGRITY ISSUES").yellow().bold());

        for issue in &results.integrity_issues {
            let severity_color = match issue.severity {
                IssueSeverity::Low => style("LOW").green(),
                IssueSeverity::Medium => style("MEDIUM").yellow(),
                IssueSeverity::High => style("HIGH").red(),
                IssueSeverity::Critical => style("CRITICAL").red().bold(),
            };

            println!(
                "  {} {} - {}",
                severity_color, issue.issue_type, issue.description
            );
        }
    }

    println!("\n{}", style("─".repeat(60)).dim());

    if error_count == 0 {
        println!("{} No validation issues found", style("✓").green().bold());
    } else {
        println!(
            "{} {} validation issues require attention",
            style("⚠").yellow().bold(),
            error_count
        );
    }
}

/// Show validation summary
fn show_validation_summary(results: &ValidationResults, options: &ValidationOptions) {
    let total_issues = results.orphaned_ids.len()
        + results.duplicate_paths.len()
        + results.invalid_paths.len()
        + results.integrity_issues.len();

    if total_issues == 0 {
        println!(
            "\n{} All {} mappings passed validation",
            style("✓").green().bold(),
            results.valid_mappings
        );
    } else {
        println!(
            "\n{} Validation found {} issues across {} mappings",
            style("⚠").yellow().bold(),
            total_issues,
            results.total_checked
        );
    }

    if options.auto_fix {
        println!(
            "{} Auto-fix is enabled but not yet implemented",
            style("Note:").cyan()
        );
        println!(
            "  {} Manual resolution of issues is currently required",
            style("•").dim()
        );
    }

    println!(
        "\n{} Use '{}' for detailed issue information",
        style("Tip:").cyan(),
        style("cascette fdid validate --format json").bold()
    );
}

/// Truncate long paths for display
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    }
}
