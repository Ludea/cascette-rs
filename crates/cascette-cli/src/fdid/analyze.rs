//! FileDataID analyze command implementation
//!
//! Provides comprehensive content analysis and statistical reporting including
//! file type distribution, size analysis, category breakdowns, and data insights.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use console::style;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};

use super::{FileInfo, OutputFormat};

/// Analysis options
#[derive(Default)]
pub struct AnalysisOptions {
    pub sample_size: Option<usize>,
    pub detailed_breakdown: bool,
    pub include_paths: bool,
    pub output_file: Option<String>,
}

/// Analysis results structure
#[derive(Debug)]
pub struct AnalysisResults {
    pub total_files: usize,
    pub category_distribution: HashMap<String, CategoryStats>,
    pub extension_distribution: HashMap<String, usize>,
    pub path_analysis: PathAnalysis,
    pub id_range_analysis: IdRangeAnalysis,
    pub content_metadata: ContentMetadata,
}

/// Statistics for a specific category
#[derive(Debug)]
pub struct CategoryStats {
    pub count: usize,
    pub percentage: f64,
    pub common_extensions: Vec<(String, usize)>,
    pub sample_paths: Vec<String>,
}

/// Path structure analysis
#[derive(Debug)]
pub struct PathAnalysis {
    pub avg_path_length: f64,
    pub max_path_length: usize,
    pub common_prefixes: Vec<(String, usize)>,
    pub directory_depth_distribution: HashMap<usize, usize>,
}

/// FileDataID range analysis
#[derive(Debug)]
pub struct IdRangeAnalysis {
    pub min_id: u32,
    pub max_id: u32,
    pub id_density: f64,
    pub range_distribution: Vec<(u32, u32, usize)>, // (start, end, count)
}

/// Content metadata analysis
#[derive(Debug)]
pub struct ContentMetadata {
    pub encrypted_files: usize,
    pub compression_distribution: HashMap<u8, usize>,
    pub estimated_total_size: u64,
}

/// Execute the FileDataID analyze command
pub async fn execute_analyze(
    orchestrator: &mut MetadataOrchestrator,
    options: AnalysisOptions,
    format: OutputFormat,
) -> Result<()> {
    // Load mappings for analysis
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    println!(
        "{} Analyzing content structure and distribution...",
        style("→").dim()
    );

    // Perform comprehensive analysis
    let results = perform_analysis(orchestrator, &options);

    // Display results
    display_analysis_results(&results, format, &options)?;

    Ok(())
}

/// Perform comprehensive content analysis
fn perform_analysis(
    orchestrator: &MetadataOrchestrator,
    options: &AnalysisOptions,
) -> AnalysisResults {
    let mut category_counts: HashMap<String, Vec<FileInfo>> = HashMap::new();
    let mut extension_counts: HashMap<String, usize> = HashMap::new();
    let mut path_lengths = Vec::new();
    let mut directory_depths: HashMap<usize, usize> = HashMap::new();
    let mut prefix_counts: HashMap<String, usize> = HashMap::new();
    let mut all_file_infos = Vec::new();

    let mut min_id = u32::MAX;
    let mut max_id = 0u32;
    let mut encrypted_count = 0;
    let mut compression_distribution: HashMap<u8, usize> = HashMap::new();

    // Sample a range of FileDataIDs for analysis
    let sample_size = options.sample_size.unwrap_or(100_000);
    let id_step = std::cmp::max(1, 3_000_000 / sample_size);
    let mut processed = 0;
    let mut found = 0;

    println!(
        "  {} Sampling FileDataIDs (step size: {})...",
        style("→").dim(),
        id_step
    );

    for file_data_id in (1..=3_000_000_u32).step_by(id_step) {
        processed += 1;

        if processed % 10000 == 0 {
            println!(
                "  {} Processed {} IDs, found {} files...",
                style("→").dim(),
                processed,
                found
            );
        }

        if let Ok(Some(file_path)) = orchestrator.resolve_file_path(file_data_id) {
            found += 1;
            let mut file_info = FileInfo::new(file_data_id, file_path.clone());

            // Update ID range
            min_id = min_id.min(file_data_id);
            max_id = max_id.max(file_data_id);

            // Get metadata
            if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                file_info.requires_encryption = content_info.requires_encryption;
                file_info.compression_level = content_info.compression_level;

                if content_info.requires_encryption {
                    encrypted_count += 1;
                }
                *compression_distribution
                    .entry(content_info.compression_level)
                    .or_insert(0) += 1;
            }

            // Analyze path structure
            let path_len = file_path.len();
            path_lengths.push(path_len);

            let depth = file_path.matches(['\\', '/']).count();
            *directory_depths.entry(depth).or_insert(0) += 1;

            // Extract file extension
            if let Some(ext) = std::path::Path::new(&file_path).extension() {
                if let Some(ext_str) = ext.to_str() {
                    *extension_counts.entry(ext_str.to_lowercase()).or_insert(0) += 1;
                }
            }

            // Extract path prefix (first directory or first 20 chars)
            let prefix = if let Some(sep_pos) = file_path.find(['\\', '/']) {
                file_path[..sep_pos].to_string()
            } else if file_path.len() > 20 {
                file_path[..20].to_string()
            } else {
                file_path.clone()
            };
            *prefix_counts.entry(prefix).or_insert(0) += 1;

            // Group by category
            let category = file_info.category.to_string();
            category_counts
                .entry(category)
                .or_default()
                .push(file_info.clone());
            all_file_infos.push(file_info);
        }
    }

    let total_files = found;

    // Build category statistics
    let mut category_distribution = HashMap::new();
    for (category, files) in category_counts {
        let count = files.len();
        let percentage = (count as f64 / total_files as f64) * 100.0;

        // Find common extensions for this category
        let mut ext_counts: HashMap<String, usize> = HashMap::new();
        for file_info in &files {
            if let Some(ext) = std::path::Path::new(&file_info.path).extension() {
                if let Some(ext_str) = ext.to_str() {
                    *ext_counts.entry(ext_str.to_lowercase()).or_insert(0) += 1;
                }
            }
        }

        let mut common_extensions: Vec<_> = ext_counts.into_iter().collect();
        common_extensions.sort_by(|a, b| b.1.cmp(&a.1));
        common_extensions.truncate(5);

        // Sample paths
        let sample_paths = if options.include_paths {
            files.iter().take(5).map(|f| f.path.clone()).collect()
        } else {
            Vec::new()
        };

        category_distribution.insert(
            category,
            CategoryStats {
                count,
                percentage,
                common_extensions,
                sample_paths,
            },
        );
    }

    // Build path analysis
    let avg_path_length = if !path_lengths.is_empty() {
        path_lengths.iter().sum::<usize>() as f64 / path_lengths.len() as f64
    } else {
        0.0
    };

    let max_path_length = path_lengths.into_iter().max().unwrap_or(0);

    let mut common_prefixes: Vec<_> = prefix_counts.into_iter().collect();
    common_prefixes.sort_by(|a, b| b.1.cmp(&a.1));
    common_prefixes.truncate(10);

    // Build ID range analysis
    let id_range = if max_id > min_id { max_id - min_id } else { 1 };
    let id_density = (total_files as f64 / id_range as f64) * 100.0;

    // Create range distribution (divide into 10 ranges)
    let mut range_distribution = Vec::new();
    if max_id > min_id {
        let range_size = id_range / 10;
        for i in 0..10 {
            let start = min_id + (i * range_size);
            let end = if i == 9 {
                max_id
            } else {
                start + range_size - 1
            };
            let count = all_file_infos
                .iter()
                .filter(|f| f.file_data_id >= start && f.file_data_id <= end)
                .count();
            range_distribution.push((start, end, count));
        }
    }

    // Content metadata
    let estimated_total_size = total_files as u64 * 50000; // Rough estimate: 50KB per file

    AnalysisResults {
        total_files,
        category_distribution,
        extension_distribution: extension_counts,
        path_analysis: PathAnalysis {
            avg_path_length,
            max_path_length,
            common_prefixes,
            directory_depth_distribution: directory_depths,
        },
        id_range_analysis: IdRangeAnalysis {
            min_id: if min_id == u32::MAX { 0 } else { min_id },
            max_id,
            id_density,
            range_distribution,
        },
        content_metadata: ContentMetadata {
            encrypted_files: encrypted_count,
            compression_distribution,
            estimated_total_size,
        },
    }
}

/// Display analysis results
fn display_analysis_results(
    results: &AnalysisResults,
    format: OutputFormat,
    options: &AnalysisOptions,
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
            let json_result = json!({
                "summary": {
                    "total_files": results.total_files,
                    "categories": results.category_distribution.len(),
                    "extensions": results.extension_distribution.len()
                },
                "category_distribution": results.category_distribution.iter()
                    .map(|(name, stats)| (name.clone(), json!({
                        "count": stats.count,
                        "percentage": stats.percentage,
                        "common_extensions": stats.common_extensions,
                        "sample_paths": if options.include_paths {
                            &stats.sample_paths
                        } else {
                            &[] as &[String]
                        }
                    })))
                    .collect::<serde_json::Map<_, _>>(),
                "path_analysis": {
                    "avg_path_length": results.path_analysis.avg_path_length,
                    "max_path_length": results.path_analysis.max_path_length,
                    "common_prefixes": results.path_analysis.common_prefixes,
                    "directory_depth_distribution": results.path_analysis.directory_depth_distribution
                },
                "id_range_analysis": {
                    "min_id": results.id_range_analysis.min_id,
                    "max_id": results.id_range_analysis.max_id,
                    "id_density": results.id_range_analysis.id_density,
                    "range_distribution": results.id_range_analysis.range_distribution
                },
                "content_metadata": {
                    "encrypted_files": results.content_metadata.encrypted_files,
                    "compression_distribution": results.content_metadata.compression_distribution,
                    "estimated_total_size": results.content_metadata.estimated_total_size
                }
            });
            writeln!(writer, "{}", serde_json::to_string_pretty(&json_result)?)?;
        }
        OutputFormat::Csv => {
            writeln!(writer, "Analysis Type,Key,Value,Percentage")?;

            // Category distribution
            for (category, stats) in &results.category_distribution {
                writeln!(
                    writer,
                    "Category,{},{},{:.2}",
                    category, stats.count, stats.percentage
                )?;
            }

            // Top extensions
            let mut extensions: Vec<_> = results.extension_distribution.iter().collect();
            extensions.sort_by(|a, b| b.1.cmp(a.1));
            for (ext, count) in extensions.iter().take(20) {
                let percentage = (**count as f64 / results.total_files as f64) * 100.0;
                writeln!(writer, "Extension,{},{},{:.2}", ext, count, percentage)?;
            }
        }
        OutputFormat::Table => {
            display_table_analysis(&mut writer, results, options)?;
        }
    }

    writer.flush()?;
    Ok(())
}

/// Display analysis results in table format
fn display_table_analysis(
    writer: &mut dyn Write,
    results: &AnalysisResults,
    options: &AnalysisOptions,
) -> Result<()> {
    writeln!(
        writer,
        "{}",
        style("FileDataID Content Analysis Report").cyan().bold()
    )?;
    writeln!(writer, "{}", style("═".repeat(60)).dim())?;

    // Overview
    writeln!(writer, "\n{}", style("OVERVIEW").yellow().bold())?;
    writeln!(
        writer,
        "  {} {}",
        style("Total Files Analyzed:").bold(),
        style(format_number(results.total_files)).green()
    )?;
    writeln!(
        writer,
        "  {} {}",
        style("File Categories:").bold(),
        results.category_distribution.len()
    )?;
    writeln!(
        writer,
        "  {} {}",
        style("Unique Extensions:").bold(),
        results.extension_distribution.len()
    )?;

    // Category distribution
    writeln!(
        writer,
        "\n{}",
        style("CATEGORY DISTRIBUTION").yellow().bold()
    )?;
    let mut categories: Vec<_> = results.category_distribution.iter().collect();
    categories.sort_by(|a, b| b.1.count.cmp(&a.1.count));

    for (category, stats) in categories.iter().take(10) {
        writeln!(
            writer,
            "  {:15} {:6} ({:5.1}%)",
            style(category).cyan(),
            style(format_number(stats.count)).green(),
            stats.percentage
        )?;

        if options.detailed_breakdown && !stats.common_extensions.is_empty() {
            write!(writer, "    {} ", style("Extensions:").dim())?;
            for (i, (ext, count)) in stats.common_extensions.iter().enumerate() {
                if i > 0 {
                    write!(writer, ", ")?;
                }
                write!(writer, "{} ({})", ext, count)?;
            }
            writeln!(writer)?;
        }
    }

    // File extensions
    writeln!(writer, "\n{}", style("TOP FILE EXTENSIONS").yellow().bold())?;
    let mut extensions: Vec<_> = results.extension_distribution.iter().collect();
    extensions.sort_by(|a, b| b.1.cmp(a.1));

    for (ext, count) in extensions.iter().take(15) {
        let percentage = (**count as f64 / results.total_files as f64) * 100.0;
        writeln!(
            writer,
            "  {:10} {:6} ({:5.1}%)",
            style(format!(".{}", ext)).cyan(),
            style(format_number(**count)).green(),
            percentage
        )?;
    }

    // Path analysis
    writeln!(
        writer,
        "\n{}",
        style("PATH STRUCTURE ANALYSIS").yellow().bold()
    )?;
    writeln!(
        writer,
        "  {} {:.1} characters",
        style("Average Path Length:").bold(),
        results.path_analysis.avg_path_length
    )?;
    writeln!(
        writer,
        "  {} {} characters",
        style("Maximum Path Length:").bold(),
        results.path_analysis.max_path_length
    )?;

    writeln!(writer, "\n  {}:", style("Common Path Prefixes").cyan())?;
    for (prefix, count) in results.path_analysis.common_prefixes.iter().take(8) {
        let percentage = (*count as f64 / results.total_files as f64) * 100.0;
        writeln!(
            writer,
            "    {:20} {:6} ({:4.1}%)",
            style(prefix).dim(),
            count,
            percentage
        )?;
    }

    // FileDataID range analysis
    writeln!(
        writer,
        "\n{}",
        style("FILEDATAID DISTRIBUTION").yellow().bold()
    )?;
    writeln!(
        writer,
        "  {} {}",
        style("ID Range:").bold(),
        style(format!(
            "{} - {}",
            results.id_range_analysis.min_id, results.id_range_analysis.max_id
        ))
        .green()
    )?;
    writeln!(
        writer,
        "  {} {:.3}%",
        style("ID Density:").bold(),
        results.id_range_analysis.id_density
    )?;

    if options.detailed_breakdown {
        writeln!(writer, "\n  {}:", style("Range Distribution").cyan())?;
        for (start, end, count) in &results.id_range_analysis.range_distribution {
            if *count > 0 {
                writeln!(
                    writer,
                    "    {}-{}: {} files",
                    style(start).dim(),
                    style(end).dim(),
                    style(count).green()
                )?;
            }
        }
    }

    // Content metadata
    writeln!(writer, "\n{}", style("CONTENT METADATA").yellow().bold())?;
    let encryption_percentage =
        (results.content_metadata.encrypted_files as f64 / results.total_files as f64) * 100.0;
    writeln!(
        writer,
        "  {} {} ({:.1}%)",
        style("Encrypted Files:").bold(),
        style(results.content_metadata.encrypted_files).red(),
        encryption_percentage
    )?;

    if !results.content_metadata.compression_distribution.is_empty() {
        writeln!(writer, "\n  {}:", style("Compression Levels").cyan())?;
        let mut compression: Vec<_> = results
            .content_metadata
            .compression_distribution
            .iter()
            .collect();
        compression.sort_by_key(|&(level, _)| level);

        for (level, count) in compression {
            let percentage = (*count as f64 / results.total_files as f64) * 100.0;
            writeln!(
                writer,
                "    Level {}: {} files ({:.1}%)",
                level, count, percentage
            )?;
        }
    }

    let size_mb = results.content_metadata.estimated_total_size as f64 / (1024.0 * 1024.0);
    writeln!(
        writer,
        "  {} {:.1} MB",
        style("Estimated Size:").bold(),
        style(size_mb).green()
    )?;

    writeln!(writer, "\n{}", style("─".repeat(60)).dim())?;
    writeln!(writer, "{} Analysis completed", style("✓").green().bold())?;

    Ok(())
}

/// Format large numbers with thousand separators
fn format_number(num: usize) -> String {
    let num_str = num.to_string();
    let mut result = String::new();
    let chars: Vec<char> = num_str.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }

    result
}
