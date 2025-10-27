//! Product management functionality
//!
//! This module handles all operations related to Blizzard products,
//! including catalog queries, installation, updates, and verification.
#![allow(clippy::format_push_string)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::redundant_closure_for_method_calls)]

use crate::installation::builds::BuildManager;
use crate::paths;
use anyhow::{Context, Result};
use cascette_formats::{CascFormat, bpsv::BpsvDocument};
use cascette_protocol::RibbitTactClient;
use comfy_table::{
    Attribute, Cell, Color, ContentArrangement, Table, modifiers::UTF8_ROUND_CORNERS,
    presets::UTF8_FULL,
};
use console::style;
use indicatif::HumanCount;
use std::collections::BTreeMap;
use std::net::ToSocketAddrs;

mod helper_functions;

#[cfg(test)]
mod tests;

/// Query the product catalog from NGDP servers
pub async fn query_catalog(
    config: &crate::config::CascetteConfig,
    filter: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let total_start = std::time::Instant::now();

    // Check cache first for the product summary
    let cache_key = "ribbit:global:v1/summary";

    let cache_check_start = std::time::Instant::now();
    let cached_response = if let Some(cache) = crate::cache::get_cache() {
        let result = cache.get(cache_key)?;
        if verbose {
            let cache_check_time = cache_check_start.elapsed();
            if result.is_some() {
                println!("[DEBUG] Cache hit in {:?}", cache_check_time);
            } else {
                println!("[DEBUG] Cache miss in {:?}", cache_check_time);
            }
        }
        result
    } else {
        if verbose {
            println!("[DEBUG] Cache not available");
        }
        None
    };

    let summary = if let Some(cached_data) = cached_response {
        // Use cached data
        if let Some(filter_str) = filter {
            println!(
                "Loading your cached product catalog (filter: '{}')...\n",
                filter_str
            );
        } else {
            println!("Loading your cached product catalog...\n");
        }

        // Parse the cached BPSV data directly from bytes
        let parse_start = std::time::Instant::now();
        let doc = BpsvDocument::parse(&cached_data)
            .map_err(|e| anyhow::anyhow!("Failed to parse cached BPSV document: {}", e))?;

        if verbose {
            let parse_time = parse_start.elapsed();
            println!(
                "[DEBUG] Parsed cached BPSV ({} bytes, {} rows) in {:?}",
                cached_data.len(),
                doc.rows().len(),
                parse_time
            );
        }

        doc
    } else {
        // Convert CLI config to protocol config (summary is always global)
        let protocol_config = config.to_protocol_config(None);

        // Create the unified NGDP client
        let client_start = std::time::Instant::now();
        let client =
            RibbitTactClient::new(protocol_config).context("Failed to create NGDP client")?;

        if verbose {
            let client_time = client_start.elapsed();
            println!("[DEBUG] Created NGDP client in {:?}", client_time);
        }

        // Query the summary endpoint (TCP Ribbit only) - this is the global catalog
        if let Some(filter_str) = filter {
            println!(
                "Fetching your product catalog (filter: '{}')...\n",
                filter_str
            );
        } else {
            println!("Fetching your product catalog...\n");
        }

        let query_start = std::time::Instant::now();
        let summary = client
            .query("v1/summary")
            .await
            .context("Failed to query product summary")?;

        if verbose {
            let query_time = query_start.elapsed();
            println!(
                "[DEBUG] Network query completed in {:?} ({} rows)",
                query_time,
                summary.rows().len()
            );
        }

        // Cache the response using configured API TTL
        if let Some(cache) = crate::cache::get_cache() {
            let cache_store_start = std::time::Instant::now();
            // The BPSV document needs to be serialized back to string for caching
            let summary_str = summary.to_string();
            cache.store_with_ttl(
                cache_key,
                summary_str.as_bytes(),
                config.cache.api_ttl_seconds,
            )?;

            if verbose {
                let cache_store_time = cache_store_start.elapsed();
                println!(
                    "[DEBUG] Cached response ({} bytes) in {:?}",
                    summary_str.len(),
                    cache_store_time
                );
            }
        }

        summary
    };

    // Parse and display the catalog
    let display_start = std::time::Instant::now();
    display_product_catalog(&summary, filter)?;

    if verbose {
        let display_time = display_start.elapsed();
        let total_time = total_start.elapsed();
        println!("\n[DEBUG] Display formatting took {:?}", display_time);
        println!("[DEBUG] Total operation time: {:?}", total_time);
    }

    Ok(())
}

/// Display product catalog in a formatted table
fn display_product_catalog(summary: &BpsvDocument, filter: Option<&str>) -> Result<()> {
    // Prepare filter for case-insensitive matching
    let filter_lower = filter.map(str::to_lowercase);

    // Group products by product code
    let mut products: BTreeMap<String, Vec<ProductInfo>> = BTreeMap::new();

    for row in summary.rows() {
        // The summary endpoint has columns: Product, Seqn, Flags
        let product_code = row
            .get_raw(0)
            .context("Failed to get product code")?
            .to_string();

        let seqn = row.get_raw(1).context("Failed to get sequence number")?;

        let flags = row.get_raw(2).unwrap_or_default().to_string();

        let info = ProductInfo {
            product_code: product_code.clone(),
            sequence: seqn.parse().unwrap_or(0),
            flags,
        };

        products.entry(product_code).or_default().push(info);
    }

    if products.is_empty() {
        println!("{}", style("No products found in the catalog.").yellow());
        return Ok(());
    }

    // Create and configure the table
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Product Code")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Description")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Sequence Numbers")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    // Count and filter products
    let mut displayed_products = Vec::new();
    let mut filtered_count = 0;

    for (product, mut variants) in products {
        // Determine product description
        let description = get_product_description(&product);

        // Apply filter if provided
        if let Some(filter_str) = &filter_lower {
            let product_lower = product.to_lowercase();
            let description_lower = description.to_lowercase();

            if !product_lower.contains(filter_str) && !description_lower.contains(filter_str) {
                filtered_count += 1;
                continue;
            }
        }

        // Sort variants by flags for consistent display
        variants.sort_by(|a, b| {
            // Order: empty flag (product) first, then bgdl, then cdn
            match (a.flags.as_str(), b.flags.as_str()) {
                ("", _) => std::cmp::Ordering::Less,
                (_, "") => std::cmp::Ordering::Greater,
                ("bgdl", "cdn") => std::cmp::Ordering::Less,
                ("cdn", "bgdl") => std::cmp::Ordering::Greater,
                (a, b) => a.cmp(b),
            }
        });

        displayed_products.push((product.clone(), description, variants));
    }

    // Check if any products to display
    if displayed_products.is_empty() {
        if let Some(filter_str) = filter {
            println!(
                "{}",
                style(format!(
                    "No products found matching filter '{}'.",
                    filter_str
                ))
                .yellow()
            );
        } else {
            println!("{}", style("No products found in the catalog.").yellow());
        }
        return Ok(());
    }

    // Add rows to table
    for (product, description, variants) in displayed_products.iter() {
        // Build sequence number display with flags
        let mut seqn_lines = Vec::new();
        for variant in variants {
            let flag_label = match variant.flags.as_str() {
                "" => "product",
                "bgdl" => "bgdl",
                "cdn" => "cdn",
                other => other,
            };

            let formatted_seqn = HumanCount(variant.sequence).to_string();
            seqn_lines.push(format!("{}: {}", flag_label, formatted_seqn));
        }

        let seqn_display = seqn_lines.join("\n");

        table.add_row(vec![
            Cell::new(product).fg(Color::Green),
            Cell::new(description),
            Cell::new(seqn_display).fg(Color::Yellow),
        ]);
    }

    println!("\n{}", style("Your Product Catalog").cyan().bold());
    println!("{}", table);

    // Add informative footer about manifest types and filter results
    println!("\n{}", style("─".repeat(80)).dim());

    // Show filter results if applicable
    if filter.is_some() {
        println!(
            "{} Showing {} products{}",
            style("Filter Results:").dim(),
            style(displayed_products.len()).yellow(),
            if filtered_count > 0 {
                format!(" ({} filtered out)", style(filtered_count).dim())
            } else {
                String::new()
            }
        );
        println!();
    }

    println!(
        "{} {} = Main game data | {} = Background downloader for pre-patching | {} = CDN server configuration",
        style("Manifest Types:").dim(),
        style("product").yellow(),
        style("bgdl").yellow(),
        style("cdn").yellow()
    );
    println!(
        "\n{} Use '{}' to see detailed product information.",
        style("Tip:").cyan(),
        style("cascette info <product>").bold()
    );

    Ok(())
}

/// Product information from summary endpoint
#[derive(Debug)]
struct ProductInfo {
    #[allow(dead_code)]
    product_code: String,
    sequence: u64,
    flags: String,
}

/// Get a human-readable description for a product code
pub fn get_product_description(product: &str) -> &'static str {
    match product {
        // World of Warcraft products
        "wow" => "World of Warcraft (Retail)",
        "wow_beta" => "World of Warcraft (Beta)",
        "wow_classic" => "World of Warcraft Classic",
        "wow_classic_beta" => "World of Warcraft Classic (Beta)",
        "wow_classic_era" => "World of Warcraft Classic Era",
        "wow_classic_era_beta" => "World of Warcraft Classic Era (Beta)",
        "wow_classic_era_ptr" => "World of Warcraft Classic Era (PTR)",
        "wow_classic_ptr" => "World of Warcraft Classic (PTR)",
        "wowt" => "World of Warcraft (PTR)",
        "wowxptr" => "World of Warcraft (PTR 2)",
        "wowz" => "World of Warcraft (Submission)",
        "wowv" => "World of Warcraft (Vendor)",
        "wowv2" => "World of Warcraft (Vendor 2)",
        "wowv3" => "World of Warcraft (Vendor 3)",
        "wowv4" => "World of Warcraft (Vendor 4)",
        "wowv5" => "World of Warcraft (Vendor 5)",
        "wowv6" => "World of Warcraft (Vendor 6)",
        "wowv7" => "World of Warcraft (Vendor 7)",
        "wowv8" => "World of Warcraft (Vendor 8)",
        "wowv9" => "World of Warcraft (Vendor 9)",
        "wowv10" => "World of Warcraft (Vendor 10)",
        "wowlivetest" => "World of Warcraft (Live Test)",
        "wowlivetest2" => "World of Warcraft (Live Test 2)",
        "wowdev" => "World of Warcraft (Internal)",
        "wowdev2" => "World of Warcraft (Internal 2)",
        "wowdev3" => "World of Warcraft (Internal 3)",
        "wowdev4" => "World of Warcraft (Internal 4)",
        "wowdev5" => "World of Warcraft (Internal 5)",
        "wowdev6" => "World of Warcraft (Internal 6)",
        "wowe1" => "World of Warcraft (Event)",
        "wowe2" => "World of Warcraft (Event 2)",
        "wowe3" => "World of Warcraft (Event 3)",
        "wowdemo" => "World of Warcraft (Demo)",

        // Battle.net infrastructure
        "agent" => "Battle.net Agent",
        "agent_beta" => "Battle.net Agent (Beta)",
        "agent_redist" => "Battle.net Agent (Redistributable)",
        "bna" => "Battle.net Desktop App",
        "bts" => "Battle.net Update Agent",
        "catalogs" => "Battle.net Game Catalog",

        // Diablo series
        "d3" => "Diablo III",
        "d3t" => "Diablo III (PTR)",
        "d3b" => "Diablo III (Beta)",
        "d3cn" => "Diablo III (China)",
        "d3vcn" => "Diablo III (Vendor China)",
        "d4" => "Diablo IV",
        "fenris" => "Diablo IV",
        "fenrisb" => "Diablo IV (Beta)",
        "fenristest" => "Diablo IV (PTR)",
        "fenrishf" => "Diablo IV (Hotfix)",
        "fenrisvendor" => "Diablo IV (Vendor)",
        "fenrisvendor1" => "Diablo IV (Vendor 1)",
        "fenrisvendor2" => "Diablo IV (Vendor 2)",
        "fenrisvendor3" => "Diablo IV (Vendor 3)",
        "fenrisvendor4" => "Diablo IV (Vendor 4)",
        "fenrisvendor5" => "Diablo IV (Vendor 5)",
        "fenrisvendor6" => "Diablo IV (Vendor 6)",
        "fenrisvendor7" => "Diablo IV (Vendor 7)",
        "fenrisvendor8" => "Diablo IV (Vendor 8)",
        "fenrisvendor9" => "Diablo IV (Vendor 9)",
        "fenrisvendor10" => "Diablo IV (Vendor 10)",
        "fenrisdev" => "Diablo IV (Development)",
        "fenrisdev2" => "Diablo IV (Development 2)",
        "fenrise" => "Diablo IV (E-Sports)",

        // Diablo Immortal
        "anbs" => "Diablo Immortal",
        "anbs-event" => "Diablo Immortal (Event)",
        "anbscn" => "Diablo Immortal (China)",
        "anbsdev" => "Diablo Immortal (Development)",
        "anbsdev2" => "Diablo Immortal (Vendor 2)",

        // Diablo II: Resurrected (Osiris)
        "osi" => "Diablo II: Resurrected",
        "osia" => "Diablo II: Resurrected (Alpha)",
        "osib" => "Diablo II: Resurrected (Beta)",
        "osic" => "Diablo II: Resurrected (Campaign)",
        "osidev" => "Diablo II: Resurrected (Development)",
        "osit" => "Diablo II: Resurrected (PTR)",
        "osiv1" => "Diablo II: Resurrected (Vendor 1)",
        "osiv2" => "Diablo II: Resurrected (Vendor 2)",
        "osiv3" => "Diablo II: Resurrected (Vendor 3)",
        "osiv4" => "Diablo II: Resurrected (Vendor 4)",
        "osiv5" => "Diablo II: Resurrected (Vendor 5)",
        "osiv6" => "Diablo II: Resurrected (Vendor 6)",
        "osiv7" => "Diablo II: Resurrected (Vendor 7)",

        // Heroes of the Storm
        "hero" => "Heroes of the Storm",
        "heroc" => "Heroes of the Storm (China)",
        "herot" => "Heroes of the Storm (PTR)",
        "storm" => "Heroes of the Storm (Legacy)",

        // Hearthstone
        "hs" => "Hearthstone",
        "hst" => "Hearthstone (PTR)",
        "hsb" => "Hearthstone",
        "hsc" => "Hearthstone (China)",
        "hsdev" => "Hearthstone (Development)",
        "hse" => "Hearthstone (Event)",
        "hse1" => "Hearthstone (Event 1)",
        "hsrc" => "Hearthstone (RC)",

        // Call of Duty series
        "lazr" => "Call of Duty: MW2 Campaign Remastered",
        "lazrv1" => "Call of Duty: MW2 Campaign Remastered (Vendor 1)",
        "lazrv2" => "Call of Duty: MW2 Campaign Remastered (Vendor 2)",
        "odin" => "Call of Duty: Modern Warfare",
        "odina" => "Call of Duty: Modern Warfare (Alpha)",
        "odinb" => "Call of Duty: Modern Warfare (Beta)",
        "odindev" => "Call of Duty: Modern Warfare (Development)",
        "odine" => "Call of Duty: Modern Warfare (Event)",
        "odinv1" => "Call of Duty: Modern Warfare (Vendor 1)",
        "odinv2" => "Call of Duty: Modern Warfare (Vendor 2)",
        "odinv3" => "Call of Duty: Modern Warfare (Vendor 3)",
        "odinv4" => "Call of Duty: Modern Warfare (Vendor 4)",
        "odinv5" => "Call of Duty: Modern Warfare (Vendor 5)",
        "odinv6" => "Call of Duty: Modern Warfare (Vendor 6)",
        "odinv7" => "Call of Duty: Modern Warfare (Vendor 7)",
        "odinv8" => "Call of Duty: Modern Warfare (Vendor 8)",
        "odinv9" => "Call of Duty: Modern Warfare (Vendor 9)",
        "odinv10" => "Call of Duty: Modern Warfare (Vendor 10)",
        "odinv11" => "Call of Duty: Modern Warfare (Vendor 11)",
        "odinv12" => "Call of Duty: Modern Warfare (Vendor 12)",
        "odinv13" => "Call of Duty: Modern Warfare (Vendor 13)",
        "odinv14" => "Call of Duty: Modern Warfare (Vendor 14)",
        "odinv15" => "Call of Duty: Modern Warfare (Vendor 15)",
        "odinv16" => "Call of Duty: Modern Warfare (Vendor 16)",
        "zeus" => "Call of Duty: Black Ops Cold War",
        "zeusa" => "Call of Duty: Black Ops Cold War (Alpha)",
        "zeusb" => "Call of Duty: Black Ops Cold War (Beta)",
        "zeusc" => "Call of Duty: Black Ops Cold War (Campaign)",
        "zeuscdlevent" => "Call of Duty: Black Ops Cold War (CDL Event)",
        "zeuscdlstaff" => "Call of Duty: Black Ops Cold War (CDL Staff)",
        "zeusdev" => "Call of Duty: Black Ops Cold War (Development)",
        "zeusevent" => "Call of Duty: Black Ops Cold War (Event)",
        "zeusr" => "Call of Duty: Black Ops Cold War (Release)",
        "zeusv1" => "Call of Duty: Black Ops Cold War (Vendor 1)",
        "zeusv2" => "Call of Duty: Black Ops Cold War (Vendor 2)",
        "zeusv3" => "Call of Duty: Black Ops Cold War (Vendor 3)",
        "zeusv4" => "Call of Duty: Black Ops Cold War (Vendor 4)",
        "zeusv5" => "Call of Duty: Black Ops Cold War (Vendor 5)",
        "zeusv6" => "Call of Duty: Black Ops Cold War (Vendor 6)",
        "zeusv7" => "Call of Duty: Black Ops Cold War (Vendor 7)",
        "zeusv8" => "Call of Duty: Black Ops Cold War (Vendor 8)",
        "zeusv9" => "Call of Duty: Black Ops Cold War (Vendor 9)",
        "zeusv10" => "Call of Duty: Black Ops Cold War (Vendor 10)",
        "zeusv11" => "Call of Duty: Black Ops Cold War (Vendor 11)",
        "zeusv12" => "Call of Duty: Black Ops Cold War (Vendor 12)",
        "zeusv13" => "Call of Duty: Black Ops Cold War (Vendor 13)",
        "zeusv14" => "Call of Duty: Black Ops Cold War (Vendor 14)",
        "zeusv15" => "Call of Duty: Black Ops Cold War (Vendor 15)",
        "zeusv16" => "Call of Duty: Black Ops Cold War (Vendor 16)",
        "viper" => "Call of Duty: Black Ops 4",
        "viperdev" => "Call of Duty: Black Ops 4 (Development)",
        "viperv1" => "Call of Duty: Black Ops 4 (Vendor 1)",
        "fore" => "Call of Duty: Vanguard",
        "forea" => "Call of Duty: Vanguard (Alpha)",
        "foreb" => "Call of Duty: Vanguard (Beta)",
        "forec" => "Call of Duty: Vanguard (Campaign)",
        "forecdlstaff" => "Call of Duty: Vanguard (CDL Staff)",
        "foredev" => "Call of Duty: Vanguard (Development)",
        "forev1" => "Call of Duty: Vanguard (Vendor 1)",
        "forev2" => "Call of Duty: Vanguard (Vendor 2)",
        "forev3" => "Call of Duty: Vanguard (Vendor 3)",
        "forev4" => "Call of Duty: Vanguard (Vendor 4)",
        "forev5" => "Call of Duty: Vanguard (Vendor 5)",
        "forev6" => "Call of Duty: Vanguard (Vendor 6)",
        "forev7" => "Call of Duty: Vanguard (Vendor 7)",
        "forev8" => "Call of Duty: Vanguard (Vendor 8)",
        "forev9" => "Call of Duty: Vanguard (Vendor 9)",
        "forev10" => "Call of Duty: Vanguard (Vendor 10)",
        "forev11" => "Call of Duty: Vanguard (Vendor 11)",
        "forev12" => "Call of Duty: Vanguard (Vendor 12)",
        "forev13" => "Call of Duty: Vanguard (Vendor 13)",
        "forev14" => "Call of Duty: Vanguard (Vendor 14)",
        "forev15" => "Call of Duty: Vanguard (Vendor 15)",
        "forev16" => "Call of Duty: Vanguard (Vendor 16)",
        "forev17" => "Call of Duty: Vanguard (Vendor 17)",
        "forev18" => "Call of Duty: Vanguard (Vendor 18)",
        "forev19" => "Call of Duty: Vanguard (Vendor 19)",
        "forev20" => "Call of Duty: Vanguard (Vendor 20)",

        // Call of Duty: Vanguard Grid Testing
        "geirdrifulfore" => "Call of Duty: Vanguard (Grid Test)",
        "geirdrifulforecdl" => "Call of Duty: Vanguard (Grid Test CDL)",
        "geirdrifulforecdlqa" => "Call of Duty: Vanguard (Grid Test CDL QA)",
        "geirdrifulforecdls" => "Call of Duty: Vanguard (Grid Test CDL S)",
        "geirdrifulforecdlv" => "Call of Duty: Vanguard (Grid Test CDL V)",
        "geirdrifulforeqa" => "Call of Duty: Vanguard (Grid Test QA)",
        "geirdrifulforev" => "Call of Duty: Vanguard (Grid Test Vendor)",

        "spot" => "Call of Duty: Modern Warfare III",
        "auks" => "Call of Duty",
        "auksv1" => "Call of Duty (Vendor 1)",
        "auksv2" => "Call of Duty (Vendor 2)",
        "auksv3" => "Call of Duty (Vendor 3)",
        "auksv4" => "Call of Duty (Vendor 4)",
        "auksv5" => "Call of Duty (Vendor 5)",
        "auksv6" => "Call of Duty (Vendor 6)",
        "auksv7" => "Call of Duty (Vendor 7)",
        "auksv8" => "Call of Duty (Vendor 8)",
        "auksv9" => "Call of Duty (Vendor 9)",
        "auksv10" => "Call of Duty (Vendor 10)",
        "auksv11" => "Call of Duty (Vendor 11)",
        "auksv12" => "Call of Duty (Vendor 12)",
        "auksv13" => "Call of Duty (Vendor 13)",
        "auksv14" => "Call of Duty (Vendor 14)",
        "auksv15" => "Call of Duty (Vendor 15)",
        "auksv16" => "Call of Duty (Vendor 16)",
        "auksv17" => "Call of Duty (Vendor 17)",
        "auksv18" => "Call of Duty (Vendor 18)",
        "auksv19" => "Call of Duty (Vendor 19)",
        "auksv20" => "Call of Duty (Vendor 20)",
        "auksv21" => "Call of Duty (Vendor 21)",
        "auksv22" => "Call of Duty (Vendor 22)",
        "auksv23" => "Call of Duty (Vendor 23)",
        "auksa" => "Call of Duty (Alpha)",
        "auksb" => "Call of Duty (Beta)",
        "aukst" => "Call of Duty (Test)",
        "auksdev" => "Call of Duty (Development)",
        "aukse" => "Call of Duty (Event)",
        "aukse2" => "Call of Duty (Event 2)",
        "auksese" => "Call of Duty (E-Sports E)",
        "auksesp" => "Call of Duty (E-Sports P)",
        "auksess" => "Call of Duty (E-Sports S)",
        "auksrc" => "Call of Duty (RC)",
        "auksrc2" => "Call of Duty (RC 2)",
        "auks123" => "Call of Duty (Internal 123)",

        // Call of Duty Grid Testing
        "randgridauks" => "Call of Duty (Grid Test)",
        "randgridauks2" => "Call of Duty (Grid Test 2)",
        "randgridaukscdl" => "Call of Duty (Grid Test CDL)",
        "randgridaukscdlqa" => "Call of Duty (Grid Test CDL QA)",
        "randgridaukscdls" => "Call of Duty (Grid Test CDL S)",
        "randgridaukscdlv" => "Call of Duty (Grid Test CDL V)",
        "randgridaukse" => "Call of Duty (Grid Test Event)",
        "randgridaukslivedev" => "Call of Duty (Grid Test Live Dev)",
        "randgridauksqa" => "Call of Duty (Grid Test QA)",
        "randgridauksqa2" => "Call of Duty (Grid Test QA 2)",
        "randgridauksrc" => "Call of Duty (Grid Test RC)",
        "randgridauksrc2" => "Call of Duty (Grid Test RC 2)",
        "randgridauksv" => "Call of Duty (Grid Test Vendor)",
        "randgridauksv2" => "Call of Duty (Grid Test Vendor 2)",

        // Overwatch / Overwatch 2 (Prometheus)
        "pro" => "Overwatch 2",
        "prot" => "Overwatch 2 (PTR)",
        "prob" => "Overwatch 2 (Beta)",
        "prodev" => "Overwatch 2 (Development)",
        "prodev6" => "Overwatch 2 (Dev 6)",
        "prodev7" => "Overwatch 2 (Dev 7)",
        "prodev8" => "Overwatch 2 (Dev 8)",
        "prodev9" => "Overwatch 2 (Dev 9)",
        "prodev10" => "Overwatch 2 (Dev 10)",
        "prodev11" => "Overwatch 2 (Dev 11)",
        "prodev12" => "Overwatch 2 (Dev 12)",
        "prodev13" => "Overwatch 2 (Dev 13)",
        "prodev14" => "Overwatch 2 (Dev 14)",
        "prodevops" => "Overwatch 2 (DevOps)",
        "prodevops2" => "Overwatch 2 (DevOps 2)",
        "proindev" => "Overwatch 2 (Internal Dev)",
        "prodemo" => "Overwatch 2 (Demo)",
        "prodemo2" => "Overwatch 2 (Demo 2)",
        "prodemo3" => "Overwatch 2 (Demo 3)",
        "prodemo4" => "Overwatch 2 (Demo 4)",
        "prodemo5" => "Overwatch 2 (Demo 5)",
        "proev" => "Overwatch 2 (Event)",
        "prolocv1" => "Overwatch 2 (Localization 1)",
        "prolocv2" => "Overwatch 2 (Localization 2)",
        "prolocv3" => "Overwatch 2 (Localization 3)",
        "prolocv4" => "Overwatch 2 (Localization 4)",
        "prov" => "Overwatch 2 (Vendor)",
        "provbv" => "Overwatch 2 (Vendor BVID)",
        "provac" => "Overwatch 2 (Vendor AC)",
        "provcomp" => "Overwatch 2 (Vendor Comp)",
        "probv1" => "Overwatch 2 (Build Verification 1)",
        "probv2" => "Overwatch 2 (Build Verification 2)",
        "probv3" => "Overwatch 2 (Build Verification 3)",
        "proutr" => "Overwatch 2 (Ultra)",

        // Overwatch Tournament/Esports
        "proc" => "Overwatch Tournament (US)",
        "proc_cn" => "Overwatch Tournament (China)",
        "proc_eu" => "Overwatch Tournament (Europe)",
        "proc_kr" => "Overwatch Tournament (Korea)",
        "proc2" => "Overwatch Professional 2",
        "proc2_cn" => "Overwatch Professional 2 (China)",
        "proc2_eu" => "Overwatch Professional 2 (Europe)",
        "proc2_kr" => "Overwatch Professional 2 (Korea)",
        "proc3" => "Overwatch Tournament (Dev)",
        "proc4" => "Overwatch Tournament 4",
        "proc5" => "Overwatch Tournament 5",
        "procr" => "Overwatch League Stage 3",
        "procr2" => "Overwatch League Stage 2",
        "proms" => "Overwatch World Cup Viewer",

        // StarCraft series
        "s1" => "StarCraft: Remastered",
        "s1t" => "StarCraft: Remastered (PTR)",
        "s2" => "StarCraft II",
        "s2t" => "StarCraft II (PTR)",
        "s2b" => "StarCraft II (Beta)",
        "s2c" => "StarCraft II (China)",
        "s2v" => "StarCraft II (Vendor)",

        // Warcraft RTS Series
        "war1" => "Warcraft: Orcs & Humans",
        "w1r" => "Warcraft: Remastered",
        "w2bn" => "Warcraft II: Battle.net Edition",
        "w2r" => "Warcraft II: Remastered",
        "w2rd" => "Warcraft II: Remastered (Development)",

        // Warcraft III: Reforged
        "w3" => "Warcraft III: Reforged",
        "w3t" => "Warcraft III: Reforged (PTR)",
        "w3b" => "Warcraft III: Reforged (Beta)",
        "w3d" => "Warcraft III (Development)",

        // Warcraft Rumble
        "gryphon" => "Warcraft Rumble",
        "gryphonb" => "Warcraft Rumble (Beta)",
        "gryphondev" => "Warcraft Rumble (Development)",
        "gryphondev3" => "Warcraft Rumble (Development 3)",
        "gryphonv" => "Warcraft Rumble (Vendor)",

        // Microsoft/Bethesda titles on Battle.net
        "scor" => "Sea of Thieves",
        "scor-beta" => "Sea of Thieves (Insiders)",
        "scor-beta-dev-1" => "Sea of Thieves (Beta Dev 1)",
        "scor-beta-rc" => "Sea of Thieves (Beta RC)",
        "scor-retail-dev-1" => "Sea of Thieves (Dev 1)",
        "scor-retail-rc" => "Sea of Thieves (RC)",

        "aris" => "Doom: The Dark Ages",
        "aris-artbook" => "Doom: The Dark Ages (Artbook)",
        "aris-artbook-dev-1" => "Doom: The Dark Ages (Artbook Dev)",
        "aris-artbook-rc" => "Doom: The Dark Ages (Artbook RC)",
        "aris-retail-dev-1" => "Doom: The Dark Ages (Dev 1)",
        "aris-retail-rc" => "Doom: The Dark Ages (RC)",

        "ark-artbook-dev-1" => "The Outer Worlds 2 (Dev 2)",
        "ark-retail-dev-1" => "The Outer Worlds 2 (Dev 1)",

        // Call of Duty cross-game testing
        "brynhildr_odin" => "COD Warzone Integration Test",
        "brynhildr_odin2" => "COD Warzone Integration Test 2",
        "brynhildr_odin_qa" => "COD Warzone Integration QA",
        "brynhildr_odin_qa2" => "COD Warzone Integration QA 2",
        "brynhildr_odin_vendor" => "COD Warzone Integration Vendor",
        "brynhildr_odin_vendor2" => "COD Warzone Integration Vendor 2",

        // Crash Bandicoot 4: It's About Time
        "wlby" => "Crash Bandicoot 4: It's About Time",
        "wlbya" => "Crash Bandicoot 4 (Alpha)",
        "wlbydev" => "Crash Bandicoot 4 (Development)",
        "wlbyt" => "Crash Bandicoot 4 (Test)",
        "wlbyv1" => "Crash Bandicoot 4 (Vendor 1)",
        "wlbyv2" => "Crash Bandicoot 4 (Vendor 2)",
        "wlbyv3" => "Crash Bandicoot 4 (Vendor 3)",
        "wlbyv4" => "Crash Bandicoot 4 (Vendor 4)",
        "wlbyv5" => "Crash Bandicoot 4 (Vendor 5)",
        "wlbyv6" => "Crash Bandicoot 4 (Vendor 6)",

        // Other Blizzard products
        "rtro" => "Blizzard Arcade Collection",
        "rtrob" => "Blizzard Arcade Collection (Beta)",
        "rtrodev" => "Blizzard Arcade Collection (Development)",
        "rtrot" => "Blizzard Arcade Collection (Test)",

        // Microsoft/Bethesda titles (speculative)
        "gdt" => "Microsoft Game (GDT)",
        "gdt-retail-dev-1" => "Microsoft Game (GDT Dev 1)",
        "gdt-retail-rc" => "Microsoft Game (GDT RC)",

        "lbra" => "Microsoft Game (LBRA)",
        "lbra-demo" => "Microsoft Game (LBRA Demo)",
        "lbra-demo-dev-1" => "Microsoft Game (LBRA Demo Dev 1)",
        "lbra-demo-rc" => "Microsoft Game (LBRA Demo RC)",
        "lbra-retail-dev-1" => "Microsoft Game (LBRA Dev 1)",
        "lbra-retail-rc" => "Microsoft Game (LBRA RC)",

        // Xbox/Microsoft integration (speculative)
        "aqua" => "Xbox Game Pass Integration",
        "aqua-artbook" => "Xbox Game Pass Integration (Artbook)",
        "aqua-artbook-dev-1" => "Xbox Game Pass Integration (Artbook Dev)",
        "aqua-artbook-rc" => "Xbox Game Pass Integration (Artbook RC)",
        "aqua-retail-dev-1" => "Xbox Game Pass Integration (Dev 1)",
        "aqua-retail-rc" => "Xbox Game Pass Integration (RC)",

        _ => "Unknown Product",
    }
}

/// Extract hostname from a URL or host:port string
pub fn extract_hostname(url: &str) -> String {
    // Remove protocol if present
    let without_protocol = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("tcp://"))
        .unwrap_or(url);

    // Remove path if present
    let host_part = without_protocol
        .split('/')
        .next()
        .unwrap_or(without_protocol);

    // Remove port if present
    if let Some(colon_pos) = host_part.rfind(':') {
        // Check if it's actually a port (all digits after colon)
        if host_part[colon_pos + 1..]
            .chars()
            .all(|c| c.is_ascii_digit())
        {
            return host_part[..colon_pos].to_string();
        }
    }

    host_part.to_string()
}

/// Validate that a CDN host can be resolved via DNS
pub fn validate_cdn_host(host: &str) -> Result<()> {
    // Remove any port specification for DNS lookup
    let hostname = if let Some(colon_pos) = host.find(':') {
        &host[..colon_pos]
    } else {
        host
    };

    // Try to resolve the hostname
    // We append port 443 for the lookup but only care about DNS resolution
    let lookup_addr = format!("{}:443", hostname);
    match lookup_addr.to_socket_addrs() {
        Ok(mut addrs) => {
            if addrs.next().is_some() {
                Ok(())
            } else {
                anyhow::bail!(
                    "CDN host '{}' could not be resolved: no addresses found",
                    host
                )
            }
        }
        Err(e) => {
            anyhow::bail!("CDN host '{}' could not be resolved: {}", host, e)
        }
    }
}

/// Validate an endpoint URL (for Ribbit/TACT hosts)
pub fn validate_endpoint_url(url: &str) -> Result<()> {
    // Skip validation for template URLs
    if url.contains("{region}") {
        // This is a template URL, can't validate until region is substituted
        return Ok(());
    }

    let hostname = extract_hostname(url);
    validate_cdn_host(&hostname)
}

/// Validate all CDN hosts in a list
pub fn validate_cdn_hosts(hosts: &[String]) -> Result<()> {
    let mut failed_hosts = Vec::new();

    for host in hosts {
        if let Err(e) = validate_cdn_host(host) {
            failed_hosts.push((host.clone(), e.to_string()));
        }
    }

    if !failed_hosts.is_empty() {
        let mut error_msg = String::from("Failed to resolve CDN hosts:\n");
        for (host, error) in failed_hosts {
            error_msg.push_str(&format!("  • {}: {}\n", host, error));
        }
        anyhow::bail!(error_msg);
    }

    Ok(())
}

/// Query detailed information for a specific product
pub async fn query_product_info(
    config: &crate::config::CascetteConfig,
    product: &str,
    region: Option<&str>,
    build_number: Option<u32>,
) -> Result<()> {
    // Use provided region or fall back to config default
    let target_region = region.unwrap_or(&config.region);

    // Validate region
    let valid_regions = ["us", "eu", "kr", "tw", "cn", "sg"];
    if !valid_regions.contains(&target_region) {
        anyhow::bail!(
            "Invalid region '{}'. Valid regions are: {}",
            target_region,
            valid_regions.join(", ")
        );
    }

    // Initialize build manager
    let data_dir = paths::data_dir().context("Failed to determine data directory")?;
    let build_manager = BuildManager::new(&data_dir);

    // Handle build number parameter - if specified, load cached build metadata
    if let Some(build_num) = build_number {
        return helper_functions::handle_cached_build_info(&build_manager, product, build_num);
    }

    // Convert CLI config to protocol config with the specified region
    // This will expand the URL templates with the target region
    let protocol_config = config.to_protocol_config(Some(target_region));

    // Create the unified NGDP client
    let client = RibbitTactClient::new(protocol_config).context("Failed to create NGDP client")?;

    // Display product header
    let description = get_product_description(product);
    println!(
        "\n{} {} {}",
        style("Product:").cyan().bold(),
        style(product).green().bold(),
        style(format!("({})", description)).dim()
    );
    println!(
        "{} {}\n",
        style("Region:").cyan().bold(),
        style(target_region).yellow()
    );

    // Query versions endpoint with caching
    println!("{}", style("Version Information").cyan().bold());
    println!("{}", style("─".repeat(80)).dim());

    let versions_endpoint = format!("v1/products/{}/versions", product);
    let versions_cache_key = format!("tact:{}:{}", target_region, versions_endpoint);

    // Try cache first for versions
    let cached_versions = if let Some(cache) = crate::cache::get_cache() {
        cache.get(&versions_cache_key)?
    } else {
        None
    };

    let versions_result = if let Some(cached_data) = cached_versions {
        // Use cached versions data
        BpsvDocument::parse(&cached_data)
            .map_err(|e| anyhow::anyhow!("Failed to parse cached versions: {}", e))
    } else {
        // Query from network and cache the result
        match client.query(&versions_endpoint).await {
            Ok(versions_doc) => {
                // Cache the response using configured API TTL
                if let Some(cache) = crate::cache::get_cache() {
                    let versions_str = versions_doc.to_string();
                    cache.store_with_ttl(
                        &versions_cache_key,
                        versions_str.as_bytes(),
                        config.cache.api_ttl_seconds,
                    )?;
                }
                Ok(versions_doc)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to query versions: {}", e)),
        }
    };

    match &versions_result {
        Ok(versions_doc) => {
            display_versions_info(versions_doc, Some(target_region))?;
        }
        Err(e) => {
            println!("{} {}", style("✗").red(), e);
        }
    }

    // Query CDNs endpoint
    println!("\n{}", style("CDN Configuration").cyan().bold());
    println!("{}", style("─".repeat(80)).dim());

    // Check for CDN host overrides
    let cdn_overrides = config.get_cdn_hosts(product);

    // Validate CDN hosts if overrides are configured
    if let Some(ref hosts) = cdn_overrides {
        if !hosts.is_empty() {
            println!("{} Validating CDN host overrides...", style("→").dim());
            if let Err(e) = validate_cdn_hosts(hosts) {
                println!("{} {}", style("✗").red().bold(), e);
                std::process::exit(1);
            }
            println!("{} All CDN hosts resolved successfully", style("✓").green());
        }
    }

    let cdns_endpoint = format!("v1/products/{}/cdns", product);
    let cdns_cache_key = format!("tact:{}:{}", target_region, cdns_endpoint);

    // Try cache first for CDNs
    let cached_cdns = if let Some(cache) = crate::cache::get_cache() {
        cache.get(&cdns_cache_key)?
    } else {
        None
    };

    let cdns_result = if let Some(cached_data) = cached_cdns {
        // Use cached CDN data
        BpsvDocument::parse(&cached_data)
            .map_err(|e| anyhow::anyhow!("Failed to parse cached CDNs: {}", e))
    } else {
        // Query from network and cache the result
        match client.query(&cdns_endpoint).await {
            Ok(cdns_doc) => {
                // Cache the response using configured API TTL
                if let Some(cache) = crate::cache::get_cache() {
                    let cdns_str = cdns_doc.to_string();
                    cache.store_with_ttl(
                        &cdns_cache_key,
                        cdns_str.as_bytes(),
                        config.cache.api_ttl_seconds,
                    )?;
                }
                Ok(cdns_doc)
            }
            Err(e) => Err(anyhow::anyhow!("Failed to query CDNs: {}", e)),
        }
    };

    match &cdns_result {
        Ok(cdns_doc) => {
            display_cdns_info(cdns_doc, cdn_overrides.as_deref(), Some(target_region))?;
        }
        Err(e) => {
            println!("{} Failed to query CDNs: {}", style("✗").red(), e);
            // If we have overrides, still show them even if CDN query fails
            if let Some(hosts) = cdn_overrides {
                println!(
                    "\n{} Using configured CDN host overrides:",
                    style("Note:").dim()
                );
                for host in hosts {
                    println!("  - {}", style(host).yellow());
                }
            }
        }
    }

    // Query versions to extract current build number and potentially save metadata
    if let Ok(versions_doc) = &versions_result {
        if let Ok(cdns_doc) = &cdns_result {
            // Extract and save build metadata if not already cached
            if let Some(current_build_num) =
                helper_functions::extract_current_build_number(versions_doc, target_region)
            {
                // Check if we already have this build cached
                if !build_manager.build_exists(product, current_build_num) {
                    // Capture and save complete build metadata
                    match capture_and_save_build_metadata(
                        &build_manager,
                        product,
                        target_region,
                        versions_doc,
                        cdns_doc,
                    ) {
                        Ok(_) => {
                            println!(
                                "\n{} Cached build metadata for {} build {}",
                                style("✓").green().bold(),
                                style(product).green(),
                                style(current_build_num).yellow().bold()
                            );
                        }
                        Err(e) => {
                            println!(
                                "\n{} Failed to cache build metadata: {}",
                                style("Warning:").yellow(),
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    // Show list of cached builds
    helper_functions::display_cached_builds(&build_manager, product)?;

    // Add helpful footer
    println!("\n{}", style("─".repeat(80)).dim());
    println!(
        "{} Use '{}' to install this product.",
        style("Tip:").cyan(),
        style(format!("cascette install {} -o <path>", product)).bold()
    );
    println!(
        "{} Use '{}' to view a specific cached build.",
        style("Tip:").cyan(),
        style(format!("cascette info {} --build <number>", product)).bold()
    );

    Ok(())
}

/// Capture and save build metadata from live NGDP query
fn capture_and_save_build_metadata(
    build_manager: &BuildManager,
    product: &str,
    region: &str,
    versions: &BpsvDocument,
    cdns: &BpsvDocument,
) -> Result<()> {
    use crate::installation::builds::{
        BuildInfo, BuildMetadata, CatalogInfo, CdnInfo, CdnProtocol, ConfigInfo, DataSource,
        MetadataInfo, ProductInfo, RegionInfo, parse_version_build,
    };
    use std::collections::HashMap;

    // Find the current version row for this region
    let version_headers = versions.schema().field_names();
    let region_idx = version_headers
        .iter()
        .position(|h| *h == "Region")
        .context("No Region column in versions")?;
    let version_idx = version_headers
        .iter()
        .position(|h| h.contains("Version"))
        .context("No Version column")?;
    let build_config_idx = version_headers
        .iter()
        .position(|h| h.contains("BuildConfig"))
        .context("No BuildConfig column")?;
    let cdn_config_idx = version_headers
        .iter()
        .position(|h| h.contains("CDNConfig"))
        .context("No CDNConfig column")?;
    let product_config_idx = version_headers
        .iter()
        .position(|h| h.contains("ProductConfig"));

    // Find the version row for this region
    let version_row = versions
        .rows()
        .iter()
        .find(|row| {
            row.get_raw(region_idx)
                .is_some_and(|r| r.to_lowercase() == region.to_lowercase())
        })
        .context("No version found for region")?;

    // Extract version and build information
    let version_build_str = version_row
        .get_raw(version_idx)
        .context("No version string")?;
    let (version, build_number) = parse_version_build(version_build_str)?;

    // Extract configuration hashes
    let build_config_hash = version_row
        .get_raw(build_config_idx)
        .context("No build config hash")?;
    let cdn_config_hash = version_row
        .get_raw(cdn_config_idx)
        .context("No CDN config hash")?;
    let product_config_hash = product_config_idx
        .and_then(|idx| version_row.get_raw(idx))
        .map(String::from);

    // Extract CDN information
    let cdn_headers = cdns.schema().field_names();
    let hosts_idx = cdn_headers
        .iter()
        .position(|h| h.contains("Hosts"))
        .context("No Hosts column in CDNs")?;
    let path_idx = cdn_headers
        .iter()
        .position(|h| *h == "Path")
        .context("No Path column in CDNs")?;
    let config_path_idx = cdn_headers.iter().position(|h| h.contains("ConfigPath"));

    // Get first CDN row (they're usually all the same)
    let cdn_row = cdns.rows().first().context("No CDN data available")?;

    let hosts_str = cdn_row.get_raw(hosts_idx).context("No CDN hosts")?;
    let cdn_path = cdn_row.get_raw(path_idx).context("No CDN path")?;
    let product_path = config_path_idx
        .and_then(|idx| cdn_row.get_raw(idx))
        .map(String::from);

    // Parse hosts
    let hosts: Vec<String> = hosts_str.split_whitespace().map(String::from).collect();

    // Detect protocols based on host ports
    let mut protocols = Vec::new();
    if hosts.iter().any(|h| h.contains(":1119")) {
        protocols.push(CdnProtocol {
            protocol: "http".to_string(),
            port: 1119,
        });
    }
    if hosts.iter().any(|h| !h.contains(":1119")) {
        protocols.push(CdnProtocol {
            protocol: "https".to_string(),
            port: 443,
        });
    }

    // Generate unique build ID
    let build_id = format!("{}:{}:{}", product, build_number, &build_config_hash[..8]);

    // Create the metadata structure
    let metadata = BuildMetadata {
        meta: MetadataInfo {
            captured_at: chrono::Utc::now(),
            source: DataSource::Live {
                region: region.to_string(),
                endpoint: "NGDP".to_string(),
                query_time: chrono::Utc::now(),
            },
            schema_version: 1,
            updated_at: chrono::Utc::now(),
            build_id: build_id.clone(),
        },
        build: BuildInfo {
            product_code: product.to_string(),
            version: version.clone(),
            build_number,
            version_build: version_build_str.to_string(),
            build_name: None, // Would need to parse build config to get this
            build_uid: None,
            build_product: None,
            branch: None,
        },
        configs: ConfigInfo {
            build_config: build_config_hash.to_string(),
            cdn_config: cdn_config_hash.to_string(),
            product_config: product_config_hash,
            patch_config: None,
            exe_configs: HashMap::new(),
        },
        cdn: CdnInfo {
            hosts: hosts.clone(),
            path: cdn_path.to_string(),
            product_path,
            protocols,
            archive_group: None,
            archive_count: None,
        },
        regions: {
            let mut regions = HashMap::new();
            regions.insert(
                region.to_string(),
                RegionInfo {
                    region: region.to_string(),
                    available: true,
                    cdn_hosts: None,
                    version_string: Some(version_build_str.to_string()),
                },
            );
            regions
        },
        product: ProductInfo {
            display_name: get_product_description(product).to_string(),
            family: get_product_family(product),
            product_type: get_product_type(product),
            platforms: vec!["windows".to_string(), "macos".to_string()],
            subscription: None,
        },
        patch: None,
        catalog: Some(CatalogInfo {
            sequence_number: None, // Would need catalog info for this
            flags: vec![],
            tags: vec![],
            release_date: None,
            end_of_support: None,
        }),
    };

    // Save the metadata
    build_manager.save_build(&metadata)?;

    Ok(())
}

/// Get product family based on product code
pub fn get_product_family(product: &str) -> String {
    // Reuse knowledge from get_product_description
    if product.starts_with("wow") {
        "wow".to_string()
    } else if product.starts_with("d2")
        || product.starts_with("d3")
        || product.starts_with("d4")
        || product.starts_with("di")
    {
        "diablo".to_string()
    } else if product.starts_with("s1") || product.starts_with("s2") || product.starts_with("sc") {
        "starcraft".to_string()
    } else if product.starts_with("hs") {
        "hearthstone".to_string()
    } else if product.starts_with("hero") || product.starts_with("storm") {
        "heroes".to_string()
    } else if product.starts_with("pro") || product == "prometheus" {
        "overwatch".to_string()
    } else if product.starts_with("rtro") {
        "arcade".to_string()
    } else if product.starts_with("viper")
        || product.starts_with("odin")
        || product.starts_with("fenris")
        || product.starts_with("auks")
    {
        "cod".to_string()
    } else if product == "agent"
        || product.starts_with("bna")
        || product == "bts"
        || product == "catalogs"
    {
        "battlenet".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Get product type based on product code
pub fn get_product_type(product: &str) -> String {
    match product {
        // Battle.net infrastructure tools
        "agent" | "agent_beta" | "agent_redist" | "bna" | "bts" | "catalogs" => "tool".to_string(),
        // Everything else is a game
        _ => "game".to_string(),
    }
}

/// Display version information from BPSV document
fn display_versions_info(versions: &BpsvDocument, region_filter: Option<&str>) -> Result<()> {
    // Filter rows by region if specified
    let filtered_rows: Vec<_> = if let Some(region) = region_filter {
        // Find the Region column index
        let headers = versions.schema().field_names();
        let region_idx = headers.iter().position(|h| *h == "Region");

        if let Some(idx) = region_idx {
            versions
                .rows()
                .iter()
                .filter(|row| {
                    row.get_raw(idx)
                        .is_some_and(|r| r.to_lowercase() == region.to_lowercase())
                })
                .collect()
        } else {
            // No Region column, show all rows
            versions.rows().iter().collect()
        }
    } else {
        versions.rows().iter().collect()
    };

    if filtered_rows.is_empty() {
        println!(
            "{}",
            style("No version information available for this region").yellow()
        );
        return Ok(());
    }

    // Create a table for versions
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Get column headers from BPSV document schema
    let headers = versions.schema().field_names();

    // Build header row with styling
    let header_cells: Vec<Cell> = headers
        .iter()
        .map(|h| Cell::new(*h).add_attribute(Attribute::Bold).fg(Color::Cyan))
        .collect();

    table.set_header(header_cells);

    // Add data rows (show latest versions first, limit to 10)
    let mut row_count = 0;
    for row in filtered_rows.iter().rev() {
        if row_count >= 10 {
            break;
        }

        let cells: Vec<Cell> = (0..headers.len())
            .map(|i| {
                let value = row.get_raw(i).unwrap_or_default();
                // Highlight version IDs and build configs
                if headers[i].contains("Version") || headers[i].contains("BuildConfig") {
                    Cell::new(value).fg(Color::Green)
                } else if headers[i].contains("CDNConfig") {
                    Cell::new(value).fg(Color::Yellow)
                } else {
                    Cell::new(value)
                }
            })
            .collect();

        table.add_row(cells);
        row_count += 1;
    }

    println!("{}", table);

    if filtered_rows.len() > 10 {
        println!(
            "\n{} Showing latest 10 of {} versions",
            style("Note:").dim(),
            style(filtered_rows.len()).yellow()
        );
    }

    Ok(())
}

/// Display CDN information from BPSV document
fn display_cdns_info(
    cdns: &BpsvDocument,
    override_hosts: Option<&[String]>,
    region_filter: Option<&str>,
) -> Result<()> {
    // Check if we have override hosts configured
    if let Some(hosts) = override_hosts {
        if !hosts.is_empty() {
            println!(
                "{} Using CDN host overrides from configuration",
                style("⚠").yellow().bold()
            );
            println!();
        }
    }

    // Filter rows by region if specified
    let filtered_rows: Vec<_> = if let Some(region) = region_filter {
        // Find the Name column index (CDN entries use Name for region)
        let headers = cdns.schema().field_names();
        let name_idx = headers.iter().position(|h| *h == "Name");

        if let Some(idx) = name_idx {
            cdns.rows()
                .iter()
                .filter(|row| {
                    row.get_raw(idx)
                        .is_some_and(|n| n.to_lowercase() == region.to_lowercase())
                })
                .collect()
        } else {
            // No Name column, show all rows
            cdns.rows().iter().collect()
        }
    } else {
        cdns.rows().iter().collect()
    };

    if filtered_rows.is_empty() && override_hosts.is_none() {
        println!(
            "{}",
            style("No CDN information available for this region").yellow()
        );
        return Ok(());
    }

    // Create a table for CDNs
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Get column headers from BPSV document schema
    let headers = cdns.schema().field_names();

    // Build header row with styling
    let header_cells: Vec<Cell> = headers
        .iter()
        .map(|h| Cell::new(*h).add_attribute(Attribute::Bold).fg(Color::Cyan))
        .collect();

    table.set_header(header_cells);

    // Add data rows
    for row in filtered_rows.iter() {
        let cells: Vec<Cell> = (0..headers.len())
            .map(|i| {
                let value = row.get_raw(i).unwrap_or_default();
                // Highlight hosts and paths differently
                if headers[i].contains("Name") {
                    Cell::new(value).fg(Color::Green)
                } else if headers[i] == "Path" {
                    Cell::new(value).fg(Color::Yellow)
                } else if headers[i].contains("Hosts") {
                    // Format hosts list for better readability
                    // Note: Table cells don't support ANSI escape sequences well,
                    // so we'll just format them nicely without hyperlinks in the table
                    let hosts = value.replace(' ', "\n");
                    Cell::new(hosts).fg(Color::Cyan)
                } else if headers[i].contains("ConfigPath") {
                    Cell::new(value).fg(Color::Magenta)
                } else {
                    Cell::new(value)
                }
            })
            .collect();

        table.add_row(cells);
    }

    println!("{}", table);

    // Add additional info about CDN structure with URLs for all hosts
    // Use override hosts if provided, otherwise use hosts from CDN response
    let hosts_to_display = if let Some(override_hosts) = override_hosts {
        // Use override hosts
        override_hosts
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>()
    } else if let Some(first_row) = filtered_rows.first() {
        // Use hosts from CDN response
        let hosts_str = headers
            .iter()
            .position(|h| h.contains("Hosts"))
            .and_then(|idx| first_row.get_raw(idx))
            .unwrap_or("");

        hosts_str.split_whitespace().collect()
    } else {
        Vec::new()
    };

    if !hosts_to_display.is_empty() {
        println!("\n{}", style("CDN Access Points:").dim());

        // Still need to get the first row for paths
        let first_row = filtered_rows.first();

        // Get paths from the CDN configuration (if we have a row)
        let (config_path, cdn_path) = if let Some(row) = first_row {
            let config = headers
                .iter()
                .position(|h| h.contains("ConfigPath"))
                .and_then(|idx| row.get_raw(idx))
                .unwrap_or("");

            let cdn = headers
                .iter()
                .position(|h| h == &"Path")
                .and_then(|idx| row.get_raw(idx))
                .unwrap_or("");

            (config, cdn)
        } else {
            // If using overrides without CDN data, use default paths
            ("", "/tpr/wow") // Default path structure
        };

        // Display URLs for each host
        for host in hosts_to_display {
            // Determine protocol based on port
            let protocol = if host.contains(":1119") {
                "http"
            } else {
                "https"
            };

            println!("\n  {} {}", style("Host:").dim(), style(host).green());

            // Show launcher base path
            if !config_path.is_empty() {
                println!(
                    "    {} {}",
                    style("Launcher Base:").dim(),
                    style(format!(
                        "{}://{}/{}",
                        protocol,
                        host,
                        config_path.trim_start_matches('/')
                    ))
                    .magenta()
                );
            }

            // Show CDN paths
            if !cdn_path.is_empty() {
                println!(
                    "    {} {}",
                    style("CDN Base:").dim(),
                    style(format!(
                        "{}://{}/{}",
                        protocol,
                        host,
                        cdn_path.trim_start_matches('/')
                    ))
                    .cyan()
                );
                println!(
                    "      Config: {}",
                    style(format!(
                        "{}://{}/{}/config/",
                        protocol,
                        host,
                        cdn_path.trim_start_matches('/')
                    ))
                    .dim()
                );
                println!(
                    "      Data:   {}",
                    style(format!(
                        "{}://{}/{}/data/",
                        protocol,
                        host,
                        cdn_path.trim_start_matches('/')
                    ))
                    .dim()
                );
                println!(
                    "      Patch:  {}",
                    style(format!(
                        "{}://{}/{}/patch/",
                        protocol,
                        host,
                        cdn_path.trim_start_matches('/')
                    ))
                    .dim()
                );
            }
        }
    }

    Ok(())
}
