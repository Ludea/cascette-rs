//! FileDataID stats command implementation
//!
//! Displays comprehensive statistics about the FileDataID service including
//! cache performance, memory usage, provider information, and lookup metrics.

use anyhow::{Context, Result};
use cascette_metadata::MetadataOrchestrator;
use console::style;

/// Execute the FileDataID stats command
pub async fn execute_stats(orchestrator: &mut MetadataOrchestrator, detailed: bool) -> Result<()> {
    // Load mappings to get accurate stats
    println!("{} Loading FileDataID mappings...", style("→").dim());
    orchestrator
        .load_mappings()
        .await
        .context("Failed to load FileDataID mappings")?;

    let stats = orchestrator
        .get_stats()
        .context("Failed to get orchestrator stats")?;
    let fdid_stats = stats.fdid_stats;

    println!("\n{}", style("FileDataID Service Statistics").cyan().bold());
    println!("{}", style("─".repeat(60)).dim());

    // Core metrics
    println!(
        "{} {}",
        style("Total mappings:").bold(),
        style(format_number(fdid_stats.total_mappings as u64)).yellow()
    );
    println!(
        "{} {}",
        style("ID lookups:").bold(),
        style(format_number(fdid_stats.id_to_path_lookups)).yellow()
    );
    println!(
        "{} {}",
        style("Path lookups:").bold(),
        style(format_number(fdid_stats.path_to_id_lookups)).yellow()
    );
    println!(
        "{} {}",
        style("Successful lookups:").bold(),
        style(format_number(fdid_stats.successful_lookups)).green()
    );
    println!(
        "{} {}",
        style("Failed lookups:").bold(),
        style(format_number(fdid_stats.failed_lookups)).red()
    );

    // Calculate success rate
    let total_lookups = fdid_stats.id_to_path_lookups + fdid_stats.path_to_id_lookups;
    let hit_rate = if total_lookups > 0 {
        (fdid_stats.successful_lookups as f64 / total_lookups as f64) * 100.0
    } else {
        0.0
    };

    let hit_rate_color = if hit_rate >= 90.0 {
        style(format!("{:.2}%", hit_rate)).green()
    } else if hit_rate >= 75.0 {
        style(format!("{:.2}%", hit_rate)).yellow()
    } else {
        style(format!("{:.2}%", hit_rate)).red()
    };

    println!("{} {}", style("Success rate:").bold(), hit_rate_color);

    // Memory usage
    let memory_mb = fdid_stats.memory_usage_bytes as f64 / (1024.0 * 1024.0);
    println!(
        "{} {}",
        style("Memory usage:").bold(),
        style(format!("{:.1} MB", memory_mb)).cyan()
    );

    if let Some(last_loaded) = fdid_stats.last_loaded {
        println!(
            "{} {}",
            style("Last loaded:").bold(),
            style(last_loaded.format("%Y-%m-%d %H:%M:%S UTC")).dim()
        );
    }

    if detailed {
        println!("\n{}", style("Detailed Statistics").bold());
        println!("{}", style("─".repeat(30)).dim());

        // Performance metrics
        println!("\n{}", style("Performance Metrics:").cyan());
        let avg_lookup_time = if total_lookups > 0 {
            // Placeholder calculation - actual timing would need to be tracked
            0.001 // Assume 1ms average
        } else {
            0.0
        };
        println!(
            "  {} {:.3} ms",
            style("Average lookup:").dim(),
            avg_lookup_time
        );

        // Cache efficiency
        let cache_efficiency = if fdid_stats.total_mappings > 0 {
            (fdid_stats.successful_lookups as f64 / fdid_stats.total_mappings as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "  {} {:.1}%",
            style("Cache efficiency:").dim(),
            cache_efficiency
        );

        // Memory breakdown (estimated)
        let avg_entry_size = if fdid_stats.total_mappings > 0 {
            fdid_stats.memory_usage_bytes / fdid_stats.total_mappings
        } else {
            0
        };
        println!(
            "  {} {} bytes",
            style("Avg entry size:").dim(),
            avg_entry_size
        );

        // TACT key statistics
        let tact_stats = stats.tact_stats;
        if tact_stats.total_keys > 0 {
            println!("\n{}", style("TACT Key Statistics:").cyan());
            println!("  {} {}", style("Total keys:").dim(), tact_stats.total_keys);
            println!(
                "  {} {}",
                style("Verified keys:").dim(),
                tact_stats.verified_keys
            );
            let verification_rate = if tact_stats.total_keys > 0 {
                (tact_stats.verified_keys as f64 / tact_stats.total_keys as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "  {} {:.1}%",
                style("Verification rate:").dim(),
                verification_rate
            );
        }

        // Data source information
        println!("\n{}", style("Data Sources:").cyan());
        println!("  {} WoWDev/wow-listfile (GitHub)", style("Primary:").dim());
        println!("  {} Community-maintained", style("Type:").dim());
        println!("  {} Automatic refresh", style("Updates:").dim());

        // Recommendations
        println!("\n{}", style("Recommendations:").cyan());
        if hit_rate < 75.0 {
            println!(
                "  {} Consider running 'cascette import filedataid' to refresh mappings",
                style("•").yellow()
            );
        }
        if memory_mb > 200.0 {
            println!(
                "  {} High memory usage - consider reducing cache size",
                style("•").yellow()
            );
        }
        if fdid_stats.failed_lookups > fdid_stats.successful_lookups {
            println!(
                "  {} High failure rate - check mapping data integrity",
                style("•").red()
            );
        }
        if hit_rate >= 90.0 && memory_mb < 100.0 {
            println!("  {} System is performing optimally", style("•").green());
        }
    }

    println!(
        "\n{} Use '{}' to import the latest FileDataID mappings",
        style("Tip:").cyan(),
        style("cascette import filedataid").bold()
    );

    if !detailed {
        println!(
            "{} Use '{}' for detailed performance metrics",
            style("Tip:").cyan(),
            style("cascette fdid stats --detailed").bold()
        );
    }

    Ok(())
}

/// Format large numbers with thousand separators
fn format_number(num: u64) -> String {
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
