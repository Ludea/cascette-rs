//! FileDataID search command implementation
//!
//! Provides advanced pattern-based search functionality with support for regex patterns,
//! case-insensitive matching, wildcard patterns, and category-specific searches.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use comfy_table::{Table, presets::UTF8_FULL};
use console::style;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;

use super::{FileCategory, FileInfo, OutputFormat};

/// Search options for pattern matching
#[derive(Default, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub regex_mode: bool,
    pub whole_word: bool,
    pub category_filter: Option<FileCategory>,
    pub limit: Option<usize>,
    pub show_context: bool,
}

/// Execute the FileDataID search command
pub async fn execute_search(
    orchestrator: &mut MetadataOrchestrator,
    pattern: &str,
    options: SearchOptions,
    format: OutputFormat,
    show_metadata: bool,
) -> Result<()> {
    // Load mappings to enable searching
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    println!(
        "{} Searching for pattern: {}",
        style("→").dim(),
        style(pattern).yellow()
    );

    // Store show_context before moving options
    let show_context = options.show_context;

    // Perform the search
    let search_results = perform_search(orchestrator, pattern, &options)?;

    if search_results.is_empty() {
        println!(
            "{} No files found matching pattern: {}",
            style("⚠").yellow(),
            style(pattern).yellow()
        );
        println!(
            "{} Try using different search options:",
            style("Tip:").cyan()
        );
        println!("  • --case-insensitive for case-insensitive matching");
        println!("  • --regex for regular expression patterns");
        println!("  • --category <category> to filter by file type");
        return Ok(());
    }

    // Display results
    display_search_results(
        &search_results,
        pattern,
        format,
        show_metadata,
        show_context,
    )?;

    // Show summary with search statistics
    show_search_summary(&search_results, pattern);

    Ok(())
}

/// Perform the search operation with the given pattern and options
fn perform_search(
    orchestrator: &MetadataOrchestrator,
    pattern: &str,
    options: &SearchOptions,
) -> Result<Vec<SearchResult>> {
    const SEARCH_RANGE: u32 = 3_000_000;
    const CHUNK_SIZE: u32 = 50000;

    let mut results = Vec::new();
    let limit = options.limit.unwrap_or(500);

    // Prepare search pattern
    let search_pattern = prepare_search_pattern(pattern, options)?;

    // Since we don't have direct iteration over all mappings,
    // we'll search through a reasonable range of FileDataIDs

    let mut processed_count = 0;
    let mut found_count = 0;

    for chunk_start in (1..=SEARCH_RANGE).step_by(CHUNK_SIZE as usize) {
        if found_count >= limit {
            break;
        }

        let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE - 1, SEARCH_RANGE);

        // Show progress for large searches
        if processed_count % 100_000 == 0 && processed_count > 0 {
            println!(
                "  {} Searched {} IDs, found {} matches...",
                style("→").dim(),
                processed_count,
                found_count
            );
        }

        for file_data_id in chunk_start..=chunk_end {
            processed_count += 1;

            if let Ok(Some(file_path)) = orchestrator.resolve_file_path(file_data_id) {
                // Apply category filter first for efficiency
                if let Some(ref target_category) = options.category_filter {
                    let file_category = FileCategory::from(file_path.as_str());
                    if &file_category != target_category {
                        continue;
                    }
                }

                // Check if path matches the search pattern
                if let Some(search_match) =
                    check_pattern_match(&file_path, &search_pattern, options)
                {
                    let mut file_info = FileInfo::new(file_data_id, file_path.clone());

                    // Add metadata if available
                    if let Ok(Some(content_info)) = orchestrator.get_content_info(file_data_id) {
                        file_info.requires_encryption = content_info.requires_encryption;
                        file_info.compression_level = content_info.compression_level;
                    }

                    results.push(SearchResult {
                        file_info,
                        match_info: search_match,
                    });

                    found_count += 1;
                    if found_count >= limit {
                        break;
                    }
                }
            }
        }
    }

    // Sort results by relevance (exact matches first, then by FileDataID)
    results.sort_by(|a, b| {
        // Prioritize exact matches
        match (a.match_info.is_exact_match, b.match_info.is_exact_match) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                // Then by number of matches (more matches = higher relevance)
                match b.match_info.match_count.cmp(&a.match_info.match_count) {
                    std::cmp::Ordering::Equal => {
                        a.file_info.file_data_id.cmp(&b.file_info.file_data_id)
                    }
                    other => other,
                }
            }
        }
    });

    Ok(results)
}

/// Prepare the search pattern based on options
fn prepare_search_pattern(pattern: &str, options: &SearchOptions) -> Result<SearchPattern> {
    if options.regex_mode {
        let regex_flags = if options.case_insensitive { "(?i)" } else { "" };
        let full_pattern = format!("{}{}", regex_flags, pattern);
        Ok(SearchPattern::Regex(Regex::new(&full_pattern)?))
    } else if pattern.contains('*') || pattern.contains('?') {
        // Convert wildcard pattern to regex
        let escaped = regex::escape(pattern)
            .replace(r"\*", ".*")
            .replace(r"\?", ".");
        let regex_flags = if options.case_insensitive { "(?i)" } else { "" };
        let full_pattern = format!("{}{}", regex_flags, escaped);
        Ok(SearchPattern::Regex(Regex::new(&full_pattern)?))
    } else {
        let search_term = if options.case_insensitive {
            pattern.to_lowercase()
        } else {
            pattern.to_string()
        };
        Ok(SearchPattern::Literal(search_term))
    }
}

/// Check if a file path matches the search pattern
fn check_pattern_match(
    file_path: &str,
    pattern: &SearchPattern,
    options: &SearchOptions,
) -> Option<MatchInfo> {
    let search_text = if options.case_insensitive {
        file_path.to_lowercase()
    } else {
        file_path.to_string()
    };

    match pattern {
        SearchPattern::Regex(regex) => {
            let matches: Vec<_> = regex.find_iter(&search_text).collect();
            if matches.is_empty() {
                None
            } else {
                Some(MatchInfo {
                    match_count: matches.len(),
                    is_exact_match: matches.len() == 1
                        && matches[0].start() == 0
                        && matches[0].end() == search_text.len(),
                    match_positions: matches.into_iter().map(|m| (m.start(), m.end())).collect(),
                })
            }
        }
        SearchPattern::Literal(literal) => {
            if options.whole_word {
                // Check for whole word matches
                let word_matches: Vec<_> = search_text
                    .match_indices(literal)
                    .filter(|&(start, _)| {
                        let is_word_start = start == 0
                            || !search_text
                                .chars()
                                .nth(start.saturating_sub(1))
                                .map_or(false, |c| c.is_alphanumeric() || c == '_');
                        let is_word_end = start + literal.len() >= search_text.len()
                            || !search_text
                                .chars()
                                .nth(start + literal.len())
                                .map_or(false, |c| c.is_alphanumeric() || c == '_');
                        is_word_start && is_word_end
                    })
                    .collect();

                if word_matches.is_empty() {
                    None
                } else {
                    Some(MatchInfo {
                        match_count: word_matches.len(),
                        is_exact_match: word_matches.len() == 1
                            && word_matches[0].0 == 0
                            && word_matches[0].0 + literal.len() == search_text.len(),
                        match_positions: word_matches
                            .into_iter()
                            .map(|(start, _)| (start, start + literal.len()))
                            .collect(),
                    })
                }
            } else {
                let matches: Vec<_> = search_text.match_indices(literal).collect();
                if matches.is_empty() {
                    None
                } else {
                    Some(MatchInfo {
                        match_count: matches.len(),
                        is_exact_match: matches.len() == 1
                            && matches[0].0 == 0
                            && matches[0].0 + literal.len() == search_text.len(),
                        match_positions: matches
                            .into_iter()
                            .map(|(start, _)| (start, start + literal.len()))
                            .collect(),
                    })
                }
            }
        }
    }
}

/// Display search results
fn display_search_results(
    results: &[SearchResult],
    pattern: &str,
    format: OutputFormat,
    show_metadata: bool,
    show_context: bool,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let json_results: Vec<_> = results
                .iter()
                .map(|result| {
                    let mut obj = json!({
                        "file_data_id": result.file_info.file_data_id,
                        "path": result.file_info.path,
                        "category": result.file_info.category.to_string(),
                        "match_count": result.match_info.match_count,
                        "is_exact_match": result.match_info.is_exact_match
                    });

                    if show_context {
                        obj["match_positions"] = json!(result.match_info.match_positions);
                        obj["context"] = json!(extract_match_context(
                            &result.file_info.path,
                            &result.match_info.match_positions
                        ));
                    }

                    if show_metadata {
                        obj["requires_encryption"] = json!(result.file_info.requires_encryption);
                        obj["compression_level"] = json!(result.file_info.compression_level);
                        obj["match_positions"] = json!(result.match_info.match_positions);
                    }
                    obj
                })
                .collect();

            println!("{}", serde_json::to_string_pretty(&json_results)?);
        }
        OutputFormat::Csv => {
            if show_metadata {
                println!(
                    "FileDataID,Path,Category,MatchCount,ExactMatch,RequiresEncryption,CompressionLevel"
                );
            } else {
                println!("FileDataID,Path,Category,MatchCount,ExactMatch");
            }

            for result in results {
                if show_metadata {
                    println!(
                        "{},\"{}\",{},{},{},{},{}",
                        result.file_info.file_data_id,
                        result.file_info.path,
                        result.file_info.category,
                        result.match_info.match_count,
                        result.match_info.is_exact_match,
                        result.file_info.requires_encryption,
                        result.file_info.compression_level
                    );
                } else {
                    println!(
                        "{},\"{}\",{},{},{}",
                        result.file_info.file_data_id,
                        result.file_info.path,
                        result.file_info.category,
                        result.match_info.match_count,
                        result.match_info.is_exact_match
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
                    "Matches",
                    "Encrypted",
                    "Compression",
                ]);
            } else {
                table.set_header(vec!["FileDataID", "Path", "Category", "Matches"]);
            }

            for result in results {
                let path_display = if result.match_info.is_exact_match {
                    style(&result.file_info.path).green().to_string()
                } else if show_context {
                    format_path_with_context(
                        &result.file_info.path,
                        &result.match_info.match_positions,
                        pattern,
                    )
                } else {
                    highlight_matches(&result.file_info.path, &result.match_info.match_positions)
                };

                if show_metadata {
                    table.add_row(vec![
                        result.file_info.file_data_id.to_string(),
                        path_display,
                        result.file_info.category.to_string(),
                        format_match_count(&result.match_info),
                        result.file_info.requires_encryption.to_string(),
                        result.file_info.compression_level.to_string(),
                    ]);
                } else {
                    table.add_row(vec![
                        result.file_info.file_data_id.to_string(),
                        path_display,
                        result.file_info.category.to_string(),
                        format_match_count(&result.match_info),
                    ]);
                }
            }

            println!("{}", table);
        }
    }

    Ok(())
}

/// Show search summary with statistics
fn show_search_summary(results: &[SearchResult], pattern: &str) {
    let total_results = results.len();
    let exact_matches = results
        .iter()
        .filter(|r| r.match_info.is_exact_match)
        .count();

    println!(
        "\n{} Found {} matches for pattern: {}",
        style("✓").green().bold(),
        style(total_results).yellow(),
        style(pattern).cyan()
    );

    if exact_matches > 0 {
        println!("  {} {} exact matches", style("•").green(), exact_matches);
    }

    // Show category breakdown
    let mut category_counts: HashMap<String, usize> = HashMap::new();
    for result in results {
        *category_counts
            .entry(result.file_info.category.to_string())
            .or_insert(0) += 1;
    }

    if category_counts.len() > 1 {
        println!("\n{}", style("Match Distribution:").cyan());
        let mut categories: Vec<_> = category_counts.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1));

        for (category, count) in categories.iter().take(5) {
            println!(
                "  {} {} matches",
                style(format!("{:12}", category)).dim(),
                style(count).yellow()
            );
        }
    }
}

/// Format match count with indicators
fn format_match_count(match_info: &MatchInfo) -> String {
    if match_info.is_exact_match {
        style("EXACT").green().bold().to_string()
    } else if match_info.match_count == 1 {
        "1".to_string()
    } else {
        format!("{}", match_info.match_count)
    }
}

/// Highlight matches in the path string
fn highlight_matches(path: &str, positions: &[(usize, usize)]) -> String {
    if positions.is_empty() {
        return path.to_string();
    }

    let mut result = String::new();
    let mut last_end = 0;

    for &(start, end) in positions {
        // Add text before match
        result.push_str(&path[last_end..start]);
        // Add highlighted match
        result.push_str(&style(&path[start..end]).yellow().bold().to_string());
        last_end = end;
    }

    // Add remaining text
    result.push_str(&path[last_end..]);
    result
}

/// Format path with surrounding context for better match visibility
fn format_path_with_context(path: &str, positions: &[(usize, usize)], _pattern: &str) -> String {
    const CONTEXT_SIZE: usize = 20;

    if positions.is_empty() {
        return path.to_string();
    }

    let mut result = String::new();

    // Only show context for first match to keep output manageable
    if let Some(&(start, end)) = positions.first() {
        // Calculate context window
        let context_start = start.saturating_sub(CONTEXT_SIZE);
        let context_end = std::cmp::min(end + CONTEXT_SIZE, path.len());

        // Add ellipsis if we're not at the beginning
        if context_start > 0 {
            result.push_str("...");
        }

        // Add context before match
        if context_start < start {
            result.push_str(&style(&path[context_start..start]).dim().to_string());
        }

        // Add highlighted match
        result.push_str(&style(&path[start..end]).yellow().bold().to_string());

        // Add context after match
        if end < context_end {
            result.push_str(&style(&path[end..context_end]).dim().to_string());
        }

        // Add ellipsis if there's more content
        if context_end < path.len() {
            result.push_str("...");
        }
    }

    result
}

/// Extract context information for JSON output
fn extract_match_context(path: &str, positions: &[(usize, usize)]) -> Vec<serde_json::Value> {
    const CONTEXT_SIZE: usize = 15;

    positions
        .iter()
        .map(|&(start, end)| {
            let context_start = start.saturating_sub(CONTEXT_SIZE);
            let context_end = std::cmp::min(end + CONTEXT_SIZE, path.len());

            json!({
                "before": &path[context_start..start],
                "match": &path[start..end],
                "after": &path[end..context_end],
                "full_context": &path[context_start..context_end]
            })
        })
        .collect()
}

/// Search pattern types
enum SearchPattern {
    Literal(String),
    Regex(Regex),
}

/// Information about a search match
#[derive(Clone, Debug)]
pub struct MatchInfo {
    pub match_count: usize,
    pub is_exact_match: bool,
    pub match_positions: Vec<(usize, usize)>,
}

/// Search result combining file info with match details
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub file_info: FileInfo,
    pub match_info: MatchInfo,
}
