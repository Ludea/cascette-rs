//! Helper functions for info command enhancements

use crate::installation::builds::{BuildManager, parse_version_build};
use anyhow::Result;
use cascette_formats::bpsv::BpsvDocument;
use comfy_table::{
    Attribute, Cell, Color, ContentArrangement, Table, modifiers::UTF8_ROUND_CORNERS,
    presets::UTF8_FULL,
};
use console::style;
use indicatif::HumanCount;

/// Handle loading and displaying cached build information
pub fn handle_cached_build_info(
    build_manager: &BuildManager,
    product: &str,
    build_number: u32,
) -> Result<()> {
    println!(
        "\n{} {} {} {}",
        style("Loading cached build:").cyan().bold(),
        style(product).green().bold(),
        style("build").dim(),
        style(build_number).yellow().bold()
    );

    match build_manager.load_build(product, build_number) {
        Ok(metadata) => {
            // Display cached build information
            display_cached_build_info(&metadata)?;

            // Show when this was captured
            println!("\n{}", style("Metadata Information").cyan().bold());
            println!("{}", style("─".repeat(80)).dim());
            println!(
                "{} {}",
                style("Captured at:").bold(),
                metadata.meta.captured_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!(
                "{} {}",
                style("Last updated:").bold(),
                metadata.meta.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
            );

            match &metadata.meta.source {
                crate::installation::builds::DataSource::Live {
                    region,
                    endpoint,
                    query_time,
                } => {
                    println!(
                        "{} Live NGDP query from {} ({})",
                        style("Source:").bold(),
                        endpoint,
                        region
                    );
                    println!(
                        "{} {}",
                        style("Queried at:").bold(),
                        query_time.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
                crate::installation::builds::DataSource::Wago {
                    import_time,
                    wago_version_id,
                } => {
                    println!("{} Imported from wago.tools", style("Source:").bold());
                    if let Some(version_id) = wago_version_id {
                        println!("{} {}", style("Wago Version ID:").bold(), version_id);
                    }
                    println!(
                        "{} {}",
                        style("Imported at:").bold(),
                        import_time.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
                crate::installation::builds::DataSource::Manual { created_by, reason } => {
                    println!("{} Manual entry by {}", style("Source:").bold(), created_by);
                    println!("{} {}", style("Reason:").bold(), reason);
                }
            }

            // Show available cached builds
            display_cached_builds(build_manager, product)?;
        }
        Err(_) => {
            println!(
                "{} Build {} not found in local cache for {}",
                style("✗").red().bold(),
                style(build_number).yellow(),
                style(product).green()
            );
            println!(
                "{} Use '{}' to query current build from NGDP",
                style("Suggestion:").cyan(),
                style(format!("cascette info {}", product)).bold()
            );

            // Show available cached builds if any exist
            display_cached_builds(build_manager, product)?;

            return Err(anyhow::anyhow!("Build not cached locally"));
        }
    }

    Ok(())
}

/// Display cached build information
pub fn display_cached_build_info(
    metadata: &crate::installation::builds::BuildMetadata,
) -> Result<()> {
    let description = super::get_product_description(&metadata.build.product_code);

    println!(
        "\n{} {} {}",
        style("Product:").cyan().bold(),
        style(&metadata.build.product_code).green().bold(),
        style(format!("({})", description)).dim()
    );

    // Build information table
    let mut build_table = Table::new();
    build_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Build Information")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Value")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    // Add build information rows
    build_table.add_row(vec![
        Cell::new("Version").fg(Color::Yellow),
        Cell::new(&metadata.build.version).fg(Color::Green),
    ]);
    build_table.add_row(vec![
        Cell::new("Build Number").fg(Color::Yellow),
        Cell::new(metadata.build.build_number.to_string()).fg(Color::Green),
    ]);
    build_table.add_row(vec![
        Cell::new("Full Version").fg(Color::Yellow),
        Cell::new(&metadata.build.version_build).fg(Color::Green),
    ]);

    if let Some(ref build_name) = metadata.build.build_name {
        build_table.add_row(vec![
            Cell::new("Build Name").fg(Color::Yellow),
            Cell::new(build_name).fg(Color::Green),
        ]);
    }

    if let Some(ref branch) = metadata.build.branch {
        build_table.add_row(vec![
            Cell::new("Branch").fg(Color::Yellow),
            Cell::new(branch).fg(Color::Green),
        ]);
    }

    println!("{}", build_table);

    // Configuration hashes
    println!("\n{}", style("Configuration Hashes").cyan().bold());
    println!("{}", style("─".repeat(80)).dim());
    println!(
        "{} {}",
        style("Build Config:").bold(),
        style(&metadata.configs.build_config).green()
    );
    println!(
        "{} {}",
        style("CDN Config:").bold(),
        style(&metadata.configs.cdn_config).yellow()
    );

    if let Some(ref product_config) = metadata.configs.product_config {
        println!(
            "{} {}",
            style("Product Config:").bold(),
            style(product_config).magenta()
        );
    }

    // CDN Information
    if !metadata.cdn.hosts.is_empty() {
        println!("\n{}", style("CDN Configuration").cyan().bold());
        println!("{}", style("─".repeat(80)).dim());
        println!(
            "{} {}",
            style("CDN Path:").bold(),
            style(&metadata.cdn.path).cyan()
        );
        println!(
            "{} {} hosts",
            style("CDN Hosts:").bold(),
            metadata.cdn.hosts.len()
        );
        for host in &metadata.cdn.hosts {
            println!("  - {}", style(host).dim());
        }

        if let Some(archive_count) = metadata.cdn.archive_count {
            println!(
                "{} {}",
                style("Archive Count:").bold(),
                style(HumanCount(archive_count as u64)).green()
            );
        }
    }

    Ok(())
}

/// Extract current build number from versions document
pub fn extract_current_build_number(versions: &BpsvDocument, region: &str) -> Option<u32> {
    // Find the latest version for the specified region
    let headers = versions.schema().field_names();
    let region_idx = headers.iter().position(|h| *h == "Region")?;
    let version_idx = headers.iter().position(|h| h.contains("Version"))?;

    // Find the row for this region
    let version_row = versions.rows().iter().find(|row| {
        row.get_raw(region_idx)
            .is_some_and(|r| r.to_lowercase() == region.to_lowercase())
    })?;

    // Extract version string and parse build number
    let version_string = version_row.get_raw(version_idx)?;
    if let Ok((_, build_number)) = parse_version_build(version_string) {
        Some(build_number)
    } else {
        None
    }
}

/// Display list of cached builds for a product
pub fn display_cached_builds(build_manager: &BuildManager, product: &str) -> Result<()> {
    let cached_builds = build_manager.list_builds(product)?;

    if !cached_builds.is_empty() {
        println!("\n{}", style("Cached Builds").cyan().bold());
        println!("{}", style("─".repeat(80)).dim());

        for build in cached_builds.iter().take(5) {
            let age_str = if let Ok(duration) = chrono::Utc::now()
                .signed_duration_since(build.meta.captured_at)
                .to_std()
            {
                let days = duration.as_secs() / 86400;
                if days > 0 {
                    format!("{} days ago", days)
                } else {
                    let hours = duration.as_secs() / 3600;
                    if hours > 0 {
                        format!("{} hours ago", hours)
                    } else {
                        "recently".to_string()
                    }
                }
            } else {
                "unknown".to_string()
            };

            println!(
                "  • Build {} (v{}) - cached {}",
                style(build.build.build_number).yellow().bold(),
                style(&build.build.version).green(),
                style(&age_str).dim()
            );
        }

        if cached_builds.len() > 5 {
            println!(
                "  {} and {} more builds",
                style("...").dim(),
                style(cached_builds.len() - 5).dim()
            );
        }

        println!(
            "\n{} Use '{}' to view a specific cached build",
            style("Tip:").dim(),
            style(format!("cascette info {} --build <number>", product)).bold()
        );
    }

    Ok(())
}
