//! TACT key management commands for the CLI
//!
//! Provides CRUD operations for TACT encryption keys through
//! the command-line interface.

use super::manager::TactKeyManager;
use anyhow::Result;
use clap::Subcommand;
use console::{Term, style};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum TactCommands {
    /// List all TACT keys
    #[command(
        long_about = "List all TACT encryption keys stored in the system keyring. \
                      Keys can be filtered by source, product, or verification status. \
                      The keyring provides secure OS-level storage for sensitive key data."
    )]
    List {
        /// Filter by source (e.g., github, wago, manual)
        #[arg(short, long, value_name = "SOURCE")]
        source: Option<String>,

        /// Filter by product (e.g., wow, wow_classic)
        #[arg(short, long, value_name = "PRODUCT")]
        product: Option<String>,

        /// Show only verified keys
        #[arg(short = 'v', long)]
        verified: bool,

        /// Show detailed information for each key
        #[arg(short = 'd', long)]
        detailed: bool,
    },

    /// Add a new TACT key
    #[command(long_about = "Add a new TACT encryption key to the system keyring. \
                      The key is stored securely using OS-level encryption. \
                      Keys are identified by their lookup hash (64-bit) and \
                      consist of 128-bit (32 hex character) encryption keys.")]
    Add {
        /// Key ID (lookup hash) in hexadecimal
        #[arg(value_name = "KEY_ID")]
        key_id: String,

        /// Encryption key (32 hex characters)
        #[arg(value_name = "KEY")]
        key: String,

        /// Source of the key
        #[arg(short, long, default_value = "manual", value_name = "SOURCE")]
        source: String,

        /// Description or comment
        #[arg(short = 'd', long, value_name = "DESC")]
        description: Option<String>,

        /// Associated product
        #[arg(short, long, value_name = "PRODUCT")]
        product: Option<String>,

        /// Associated build number
        #[arg(short, long, value_name = "BUILD")]
        build: Option<u32>,
    },

    /// Get a specific TACT key
    #[command(
        long_about = "Retrieve a specific TACT encryption key from the keyring. \
                      The key is displayed with its metadata including source, \
                      description, and verification status."
    )]
    Get {
        /// Key ID (lookup hash) in hexadecimal
        #[arg(value_name = "KEY_ID")]
        key_id: String,
    },

    /// Remove a TACT key
    #[command(long_about = "Remove a TACT encryption key from the system keyring. \
                      This permanently deletes the key and its metadata. \
                      Use with caution as this operation cannot be undone.")]
    Remove {
        /// Key ID (lookup hash) in hexadecimal
        #[arg(value_name = "KEY_ID")]
        key_id: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show TACT key statistics
    #[command(long_about = "Display statistics about the TACT key store including \
                      total key count, breakdown by source and product, \
                      and verification status.")]
    Stats,

    /// Verify a TACT key works
    #[command(
        long_about = "Mark a TACT key as verified after confirming it successfully \
                      decrypts content. This helps track which keys are known to work."
    )]
    Verify {
        /// Key ID (lookup hash) in hexadecimal
        #[arg(value_name = "KEY_ID")]
        key_id: String,
    },
}

impl TactCommands {
    /// Execute the TACT key command
    pub fn execute(self, data_dir: &PathBuf) -> Result<()> {
        let mut manager = TactKeyManager::new(data_dir)?;

        match self {
            Self::List {
                source,
                product,
                verified,
                detailed,
            } => {
                let keys = manager.list_keys(source.as_deref(), product.as_deref(), verified);

                if keys.is_empty() {
                    println!("{}", style("No TACT keys found matching criteria").yellow());
                    return Ok(());
                }

                println!(
                    "{}",
                    style(format!("Found {} TACT keys:", keys.len()))
                        .green()
                        .bold()
                );
                println!();

                if detailed {
                    for (key, metadata) in keys {
                        println!("{} {:016X}", style("Key ID:").cyan().bold(), key.id);
                        println!("  {}: {}", style("Key").dim(), hex::encode(key.key));
                        println!("  {}: {}", style("Source").dim(), metadata.source);
                        if let Some(desc) = &metadata.description {
                            println!("  {}: {}", style("Description").dim(), desc);
                        }
                        if let Some(product) = &metadata.product {
                            println!("  {}: {}", style("Product").dim(), product);
                        }
                        if let Some(build) = metadata.build {
                            println!("  {}: {}", style("Build").dim(), build);
                        }
                        println!(
                            "  {}: {}",
                            style("Added").dim(),
                            metadata.added_at.format("%Y-%m-%d %H:%M:%S")
                        );
                        if let Some(verified) = metadata.last_verified {
                            println!(
                                "  {}: {} ✓",
                                style("Verified").dim(),
                                verified.format("%Y-%m-%d %H:%M:%S")
                            );
                        }
                        println!();
                    }
                } else {
                    // Simple table format
                    println!(
                        "{:<18} {:<34} {:<10} {:<12} {}",
                        style("KEY ID").bold(),
                        style("ENCRYPTION KEY").bold(),
                        style("SOURCE").bold(),
                        style("PRODUCT").bold(),
                        style("VERIFIED").bold()
                    );
                    println!("{}", style("-".repeat(90)).dim());

                    for (key, metadata) in keys {
                        let verified = if metadata.last_verified.is_some() {
                            "✓"
                        } else {
                            ""
                        };
                        let product = metadata.product.as_deref().unwrap_or("-");

                        println!(
                            "{:016X}  {}  {:<10} {:<12} {}",
                            key.id,
                            hex::encode(key.key).to_uppercase(),
                            metadata.source,
                            product,
                            style(verified).green()
                        );
                    }
                }
            }

            Self::Add {
                key_id,
                key,
                source,
                description,
                product,
                build,
            } => {
                let key_id = u64::from_str_radix(key_id.trim_start_matches("0x"), 16)?;

                // Use batch method which handles persistence internally
                manager.add_key_batch(
                    key_id,
                    &key,
                    &source,
                    description,
                    product.clone(),
                    build,
                )?;

                println!(
                    "{} Added TACT key {:016X}",
                    style("✓").green().bold(),
                    key_id
                );
                println!("  Source: {}", source);
                if let Some(p) = product {
                    println!("  Product: {}", p);
                }
            }

            Self::Get { key_id } => {
                let key_id = u64::from_str_radix(key_id.trim_start_matches("0x"), 16)?;

                match manager.get_key(key_id)? {
                    Some((key, Some(metadata))) => {
                        println!("{} {:016X}", style("Key ID:").cyan().bold(), key.id);
                        println!("{} {}", style("Key:").cyan().bold(), hex::encode(key.key));

                        println!();
                        println!("{}", style("Metadata:").cyan().bold());
                        println!("  Source: {}", metadata.source);
                        if let Some(desc) = &metadata.description {
                            println!("  Description: {}", desc);
                        }
                        if let Some(product) = &metadata.product {
                            println!("  Product: {}", product);
                        }
                        if let Some(build) = metadata.build {
                            println!("  Build: {}", build);
                        }
                        println!("  Added: {}", metadata.added_at.format("%Y-%m-%d %H:%M:%S"));
                        if let Some(verified) = metadata.last_verified {
                            println!("  Verified: {} ✓", verified.format("%Y-%m-%d %H:%M:%S"));
                        }
                    }
                    Some((key, None)) => {
                        // Key exists but no metadata (shouldn't happen with file-based storage)
                        println!("{} {:016X}", style("Key ID:").cyan().bold(), key.id);
                        println!("{} {}", style("Key:").cyan().bold(), hex::encode(key.key));
                        println!();
                        println!("{}", style("No metadata available").yellow());
                    }
                    None => {
                        println!("{} Key {:016X} not found", style("✗").red().bold(), key_id);
                    }
                }
            }

            Self::Remove { key_id, yes } => {
                let key_id = u64::from_str_radix(key_id.trim_start_matches("0x"), 16)?;

                if !yes {
                    let term = Term::stdout();
                    println!("Remove TACT key {:016X}? This cannot be undone.", key_id);
                    print!("Continue? [y/N] ");
                    let _ = term.flush();

                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;

                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Cancelled");
                        return Ok(());
                    }
                }

                if manager.remove_key(key_id)? {
                    manager.save_store()?;
                    println!(
                        "{} Removed TACT key {:016X}",
                        style("✓").green().bold(),
                        key_id
                    );
                } else {
                    println!("{} Key {:016X} not found", style("✗").red().bold(), key_id);
                }
            }

            Self::Stats => {
                let stats = manager.get_stats();

                println!("{}", style("TACT Key Statistics").cyan().bold());
                println!();
                println!("  {}: {}", style("Total keys").dim(), stats.total_keys);
                println!(
                    "  {}: {}",
                    style("Verified keys").dim(),
                    stats.verified_keys
                );

                if !stats.by_source.is_empty() {
                    println!();
                    println!("  {}:", style("By source").dim());
                    for (source, count) in &stats.by_source {
                        println!("    {}: {}", source, count);
                    }
                }

                if !stats.by_product.is_empty() {
                    println!();
                    println!("  {}:", style("By product").dim());
                    for (product, count) in &stats.by_product {
                        println!("    {}: {}", product, count);
                    }
                }

                if let Some(last_update) = stats.last_update {
                    println!();
                    println!(
                        "  {}: {}",
                        style("Last update").dim(),
                        last_update.format("%Y-%m-%d %H:%M:%S")
                    );
                }
            }

            Self::Verify { key_id } => {
                let key_id = u64::from_str_radix(key_id.trim_start_matches("0x"), 16)?;

                manager.mark_verified(key_id)?;
                manager.save_store()?;
                println!(
                    "{} Marked key {:016X} as verified",
                    style("✓").green().bold(),
                    key_id
                );
            }
        }

        Ok(())
    }
}
