//! FileDataID command definitions and handler
//!
//! Provides the command-line interface definitions and routing for all
//! FileDataID operations integrated with the main cascette CLI.

use anyhow::Result;
use cascette_metadata::fdid::FileDataIdStorage;
use clap::Subcommand;

use super::create_storage;

/// FileDataID subcommands
#[derive(Subcommand)]
pub enum FdidCommands {
    /// Resolve FileDataIDs to paths or vice versa
    #[command(
        long_about = "Resolve FileDataIDs to file paths or file paths to FileDataIDs. \
        Supports both single lookups and bulk operations."
    )]
    Resolve {
        /// FileDataID to resolve (can be comma-separated list)
        #[arg(short, long, value_name = "ID[,ID,...]")]
        id: Option<String>,

        /// File path to resolve (can be comma-separated list)
        #[arg(short, long, value_name = "PATH[,PATH,...]")]
        path: Option<String>,

        /// Output format
        #[arg(short = 'f', long, default_value = "table")]
        format: String,

        /// Show additional metadata
        #[arg(short = 'm', long)]
        show_metadata: bool,
    },

    /// Browse FileDataIDs with filtering
    #[command(
        long_about = "Browse FileDataID mappings with optional filters for extension, \
        category, ID range, or path patterns."
    )]
    Browse {
        /// Filter by file extension (e.g., 'blp', 'm2', 'wmo')
        #[arg(short = 'e', long)]
        extension: Option<String>,

        /// Filter by path pattern (supports wildcards)
        #[arg(short = 'p', long)]
        pattern: Option<String>,

        /// Filter by category (model, texture, audio, etc.)
        #[arg(short = 'c', long)]
        category: Option<String>,

        /// Minimum FileDataID
        #[arg(long)]
        min_id: Option<u32>,

        /// Maximum FileDataID
        #[arg(long)]
        max_id: Option<u32>,

        /// Maximum number of results to show
        #[arg(short = 'l', long, default_value = "100")]
        limit: usize,

        /// Output format
        #[arg(short = 'f', long, default_value = "table")]
        format: String,

        /// Show additional metadata
        #[arg(short = 'm', long)]
        show_metadata: bool,
    },

    /// Search for FileDataIDs by pattern
    #[command(
        long_about = "Search for FileDataIDs using text patterns or regular expressions. \
        Supports case-insensitive and regex modes."
    )]
    Search {
        /// Search pattern
        pattern: String,

        /// Case-insensitive search
        #[arg(short = 'i', long)]
        case_insensitive: bool,

        /// Use regular expression
        #[arg(short = 'r', long)]
        regex: bool,

        /// Search in IDs instead of paths
        #[arg(long)]
        search_ids: bool,

        /// Show context around matches
        #[arg(short = 'c', long)]
        show_context: bool,

        /// Maximum number of results
        #[arg(short = 'l', long, default_value = "100")]
        limit: usize,

        /// Output format
        #[arg(short = 'f', long, default_value = "table")]
        format: String,
    },

    /// Show FileDataID statistics
    #[command(
        long_about = "Display statistics about FileDataID mappings including counts, \
        categories, and coverage information."
    )]
    Stats {
        /// Show detailed statistics
        #[arg(short = 'd', long)]
        detailed: bool,
    },

    /// Analyze FileDataID content distribution
    #[command(
        long_about = "Analyze the distribution of FileDataIDs across different categories, \
        extensions, and directories."
    )]
    Analyze {
        /// Sample size for analysis (0 = all)
        #[arg(short = 's', long, default_value = "10000")]
        sample_size: usize,

        /// Show detailed breakdown
        #[arg(short = 'd', long)]
        detailed: bool,

        /// Include path analysis
        #[arg(short = 'p', long)]
        include_paths: bool,

        /// Output file
        #[arg(short = 'o', long)]
        output: Option<String>,

        /// Output format
        #[arg(short = 'f', long, default_value = "table")]
        format: String,
    },

    /// Batch operations on FileDataIDs
    #[command(
        long_about = "Perform batch operations on FileDataIDs from input files or stdin. \
        Supports resolution, validation, and filtering operations."
    )]
    Batch {
        /// Operation to perform
        #[arg(value_enum)]
        #[arg(
            value_name = "OPERATION",
            help = "Operation: resolve-ids, resolve-paths, validate-ids, validate-paths"
        )]
        operation: String,

        /// Input file (use '-' for stdin)
        #[arg(short = 'i', long)]
        input_file: Option<String>,

        /// Read from stdin
        #[arg(long, conflicts_with = "input_file")]
        stdin: bool,

        /// Output file (default: stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,

        /// Show progress
        #[arg(short = 'p', long)]
        progress: bool,

        /// Continue on errors
        #[arg(short = 'c', long)]
        continue_on_error: bool,

        /// Output format
        #[arg(short = 'f', long, default_value = "csv")]
        format: String,

        /// Include metadata in output
        #[arg(short = 'm', long)]
        show_metadata: bool,
    },

    /// Validate FileDataID mappings
    #[command(
        long_about = "Validate FileDataID mappings for consistency, checking for duplicates, \
        conflicts, and invalid entries."
    )]
    Validate {
        /// Check for duplicate IDs
        #[arg(long, default_value = "true")]
        check_duplicates: bool,

        /// Check for path conflicts
        #[arg(long, default_value = "true")]
        check_conflicts: bool,

        /// Check for invalid paths
        #[arg(long, default_value = "true")]
        check_invalid: bool,

        /// Fix issues if possible
        #[arg(short = 'f', long)]
        fix: bool,

        /// Output format
        #[arg(long, default_value = "table")]
        format: String,
    },

    /// Profile FileDataID operations
    #[command(
        long_about = "Profile FileDataID lookup performance and memory usage. \
        Useful for optimizing cache settings and identifying bottlenecks."
    )]
    Profile {
        /// Number of operations to perform
        #[arg(short = 'n', long, default_value = "10000")]
        operations: usize,

        /// Mix of operations (id:path ratio, e.g., '50:50')
        #[arg(short = 'm', long, default_value = "50:50")]
        mix: String,

        /// Number of warmup operations
        #[arg(short = 'w', long, default_value = "1000")]
        warmup: usize,

        /// Show detailed metrics
        #[arg(short = 'd', long)]
        detailed: bool,

        /// Output format
        #[arg(short = 'f', long, default_value = "table")]
        format: String,
    },
}

/// Handle all FileDataID command operations
pub async fn handle_fdid_command(subcommand: FdidCommands) -> Result<()> {
    let storage = create_storage().await?;

    match subcommand {
        FdidCommands::Resolve {
            id,
            path,
            format: _,
            show_metadata: _,
        } => {
            if let Some(id_str) = id {
                // Resolve IDs to paths
                for id_part in id_str.split(',') {
                    if let Ok(file_id) = id_part.trim().parse::<u32>() {
                        if let Some(file_path) = storage.get_path(file_id) {
                            println!("{} → {}", file_id, file_path);
                        } else {
                            println!("{} → [Not found]", file_id);
                        }
                    }
                }
            } else if let Some(path_str) = path {
                // Resolve paths to IDs
                for path_part in path_str.split(',') {
                    let trimmed_path = path_part.trim();
                    if let Some(file_id) = storage.get_id(trimmed_path) {
                        println!("{} → {}", trimmed_path, file_id);
                    } else {
                        println!("{} → [Not found]", trimmed_path);
                    }
                }
            } else {
                anyhow::bail!("Please specify either --id or --path");
            }

            Ok(())
        }

        FdidCommands::Stats { detailed } => {
            println!("\n📊 FileDataID Statistics");
            println!("  Total mappings: {}", storage.mapping_count());

            if detailed {
                // Count by category
                use super::FileCategory;
                use std::collections::HashMap;

                let mut categories: HashMap<String, usize> = HashMap::new();
                let mut extensions: HashMap<String, usize> = HashMap::new();

                for (_id, path) in storage.iter_mappings() {
                    let category = FileCategory::from(path);
                    *categories.entry(category.to_string()).or_insert(0) += 1;

                    if let Some(ext) = path.rsplit('.').next() {
                        if ext.len() <= 10 {
                            *extensions.entry(ext.to_lowercase()).or_insert(0) += 1;
                        }
                    }
                }

                println!("\n📁 Categories:");
                let mut cats: Vec<_> = categories.into_iter().collect();
                cats.sort_by(|a, b| b.1.cmp(&a.1));
                for (cat, count) in cats.iter().take(10) {
                    println!("  {}: {}", cat, count);
                }

                println!("\n📄 Top Extensions:");
                let mut exts: Vec<_> = extensions.into_iter().collect();
                exts.sort_by(|a, b| b.1.cmp(&a.1));
                for (ext, count) in exts.iter().take(10) {
                    println!("  .{}: {}", ext, count);
                }
            }

            Ok(())
        }

        FdidCommands::Analyze {
            sample_size,
            detailed: _,
            include_paths: _,
            output: _,
            format: _,
        } => {
            println!("\n🔍 FileDataID Content Analysis");
            println!("  Total mappings to analyze: {}", storage.mapping_count());

            if storage.mapping_count() > 0 {
                use std::collections::HashMap;

                let mut path_depth: HashMap<usize, usize> = HashMap::new();
                let mut id_ranges: HashMap<u32, usize> = HashMap::new();
                let sample_count = if sample_size == 0 {
                    usize::MAX
                } else {
                    sample_size
                };
                for (count, (id, path)) in storage.iter_mappings().enumerate() {
                    if count >= sample_count {
                        break;
                    }

                    // Analyze path depth
                    let depth = path.matches('/').count();
                    *path_depth.entry(depth).or_insert(0) += 1;

                    // Analyze ID ranges (buckets of 100k)
                    let bucket = id / 100_000;
                    *id_ranges.entry(bucket).or_insert(0) += 1;
                }

                println!("\n📊 Path Depth Distribution:");
                let mut depths: Vec<_> = path_depth.into_iter().collect();
                depths.sort_by(|a, b| a.0.cmp(&b.0));
                for (depth, count) in depths {
                    println!("  Depth {}: {} files", depth, count);
                }

                println!("\n🔢 ID Range Distribution:");
                let mut ranges: Vec<_> = id_ranges.into_iter().collect();
                ranges.sort_by(|a, b| a.0.cmp(&b.0));
                for (bucket, count) in ranges {
                    let start = bucket * 100_000;
                    let end = start + 99_999;
                    println!("  {}-{}: {} files", start, end, count);
                }
            }

            Ok(())
        }

        _ => {
            println!("Command not yet fully implemented with new storage system");
            println!("Storage has {} mappings loaded", storage.mapping_count());
            Ok(())
        }
    }
}
