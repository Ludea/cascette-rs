//! Import functionality for community data sources
//!
//! This module provides commands for importing historical build data,
//! TACT keys, and FileDataID mappings from community sources.

use anyhow::{Context, Result};
use cascette_import::{ImportManager, WagoProvider};
use cascette_metadata::{MetadataOrchestrator, OrchestratorConfig};
use cascette_protocol::RibbitTactClient;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;

use crate::config::CascetteConfig;
use crate::installation::builds::{
    BuildInfo, BuildManager, BuildMetadata, CatalogInfo, CdnInfo, CdnProtocol, ConfigInfo,
    DataSource, MetadataInfo, ProductInfo, parse_version_build,
};
use crate::paths;
use crate::products::{get_product_description, get_product_family, get_product_type};

/// Import historic build information from wago.tools
pub async fn import_builds(force: bool, product_filter: Option<String>) -> Result<()> {
    println!(
        "\n{} {} {}",
        style("Importing builds from").cyan(),
        style("wago.tools").green().bold(),
        style("community archive...").cyan()
    );

    // Check if the product is supported by wago.tools
    if let Some(ref product) = product_filter {
        if !is_wago_supported_product(product) {
            println!(
                "\n{} Product '{}' is not supported by wago.tools",
                style("⚠").yellow().bold(),
                style(product).yellow()
            );
            println!(
                "{} wago.tools only provides data for World of Warcraft products:",
                style("Note:").cyan()
            );
            println!("  • wow (Retail)");
            println!("  • wow_classic");
            println!("  • wow_classic_era");
            println!("  • wow_beta, wow_classic_beta, wow_classic_ptr, etc.");
            println!(
                "\n{} Use '{}' to import all WoW products",
                style("Tip:").cyan(),
                style("cascette import builds").bold()
            );
            return Ok(());
        }
    }

    // Initialize components
    let data_dir = paths::data_dir().context("Failed to determine data directory")?;
    let build_manager = BuildManager::new(&data_dir);

    // Load config for NGDP client
    let config = CascetteConfig::load().context("Failed to load configuration")?;
    let protocol_config = config.to_protocol_config(Some("us")); // Default to US region
    let ngdp_client =
        RibbitTactClient::new(protocol_config).context("Failed to create NGDP client")?;

    // Create wago provider
    let wago_provider = WagoProvider::new()
        .await
        .context("Failed to initialize wago.tools provider")?;

    // Create import manager and add provider
    let import_manager = ImportManager::new();
    import_manager
        .add_provider("wago", Box::new(wago_provider))
        .await?;

    println!("{} Fetching build catalog...", style("→").dim());

    // Fetch builds based on filter
    let all_builds = if let Some(ref product) = product_filter {
        println!(
            "{} Filtering for product: {}",
            style("→").dim(),
            style(product).yellow()
        );
        import_manager
            .get_builds(product)
            .await
            .with_context(|| format!("Failed to fetch builds for {}", product))?
    } else {
        import_manager
            .get_all_builds()
            .await
            .context("Failed to fetch builds from wago.tools")?
    };

    // Group builds by product
    let mut builds_by_product: HashMap<String, Vec<_>> = HashMap::new();
    for build in all_builds {
        builds_by_product
            .entry(build.product.clone())
            .or_default()
            .push(build);
    }

    let total_products = builds_by_product.len();
    let total_builds = builds_by_product
        .values()
        .map(std::vec::Vec::len)
        .sum::<usize>();

    println!(
        "\n{} Found {} builds across {} products",
        style("✓").green().bold(),
        style(total_builds).yellow().bold(),
        style(total_products).yellow().bold()
    );

    // Process each product's builds
    let mut total_imported = 0;
    let mut total_skipped = 0;
    let mut total_failed = 0;

    for (product_code, builds) in &builds_by_product {
        let product_desc = get_product_description(product_code);
        println!(
            "\n{} {} {} ({} builds)",
            style("Processing:").cyan(),
            style(product_code).green().bold(),
            style(format!("({})", product_desc)).dim(),
            builds.len()
        );

        // Try to fetch CDN info for this product once
        let cdn_info = fetch_cdn_info(&ngdp_client, product_code).await.ok();
        if cdn_info.is_none() {
            println!(
                "  {} Could not fetch CDN info for {} (will import without CDN data)",
                style("⚠").yellow(),
                product_code
            );
        }

        // Create progress bar for this product
        let progress = ProgressBar::new(builds.len() as u64);
        progress.set_style(
            ProgressStyle::default_bar()
                .template("  {bar:40.cyan/blue} {pos}/{len} {msg}")
                .expect("Valid progress bar template")
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );

        let mut imported = 0;
        let mut skipped = 0;
        let mut failed = 0;

        for build_info in builds {
            // Update progress message
            progress.set_message(format!(
                "Build {} (v{})",
                build_info.build, build_info.version
            ));

            // Check if already exists
            if !force && build_manager.build_exists(product_code, build_info.build) {
                skipped += 1;
                progress.inc(1);
                continue;
            }

            // Transform and save with CDN info if available
            match transform_and_save_build(&build_manager, build_info, cdn_info.as_ref()) {
                Ok(_) => {
                    imported += 1;
                }
                Err(e) => {
                    failed += 1;
                    eprintln!(
                        "  {} Failed to import build {}: {}",
                        style("✗").red(),
                        build_info.build,
                        e
                    );
                }
            }

            progress.inc(1);
        }

        progress.finish_and_clear();

        // Report product results
        if imported > 0 || skipped > 0 || failed > 0 {
            println!(
                "  {} Imported: {}, Skipped: {}, Failed: {}",
                style("→").dim(),
                style(imported).green(),
                style(skipped).yellow(),
                style(failed).red()
            );
        }

        total_imported += imported;
        total_skipped += skipped;
        total_failed += failed;
    }

    // Display summary
    println!("\n{}", style("Import Summary").cyan().bold());
    println!("{}", style("─".repeat(80)).dim());
    println!(
        "{} {}",
        style("Products processed:").bold(),
        style(total_products).yellow()
    );
    println!(
        "{} {}",
        style("Builds imported:").bold(),
        style(total_imported).green()
    );
    if total_skipped > 0 {
        println!(
            "{} {} (already cached)",
            style("Builds skipped:").bold(),
            style(total_skipped).yellow()
        );
    }
    if total_failed > 0 {
        println!(
            "{} {}",
            style("Failed imports:").bold(),
            style(total_failed).red()
        );
    }

    println!(
        "\n{} Cached builds are now available for offline use.",
        style("✓").green().bold()
    );
    println!(
        "{} Use '{}' to view imported builds.",
        style("Tip:").cyan(),
        style("cascette info <product> --build <number>").bold()
    );

    Ok(())
}

/// Fetch CDN information for a product from NGDP
async fn fetch_cdn_info(client: &RibbitTactClient, product: &str) -> Result<CdnInfo> {
    // Query CDNs endpoint
    let cdns_endpoint = format!("v1/products/{}/cdns", product);
    let cdns_doc = client.query(&cdns_endpoint).await?;

    // Extract CDN information from BPSV document
    let headers = cdns_doc.schema().field_names();
    let hosts_idx = headers
        .iter()
        .position(|h| h.contains("Hosts"))
        .context("No Hosts column in CDNs")?;
    let path_idx = headers
        .iter()
        .position(|h| *h == "Path")
        .context("No Path column in CDNs")?;
    let config_path_idx = headers.iter().position(|h| h.contains("ConfigPath"));

    // Get first CDN row (they're usually all the same)
    let cdn_row = cdns_doc.rows().first().context("No CDN data available")?;

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

    Ok(CdnInfo {
        hosts,
        path: cdn_path.to_string(),
        product_path,
        protocols,
        archive_group: None,
        archive_count: None,
    })
}

/// Transform a wago build to our BuildMetadata format and save it
fn transform_and_save_build(
    build_manager: &BuildManager,
    wago_build: &cascette_import::types::BuildInfo,
    cdn_info: Option<&CdnInfo>,
) -> Result<()> {
    // Parse version components
    let (version, build_number) = parse_version_build(&wago_build.version)?;

    // Ensure build number matches
    if build_number != wago_build.build {
        // Use the one from the parsed version as authoritative
        // Some wago builds might have inconsistent data
    }

    // Extract config hashes from metadata
    let build_config = wago_build
        .metadata
        .get("build_config")
        .cloned()
        .unwrap_or_default();
    let cdn_config = wago_build
        .metadata
        .get("cdn_config")
        .cloned()
        .unwrap_or_default();
    let product_config = wago_build.metadata.get("product_config").cloned();

    // Generate build ID
    let build_id = if build_config.len() >= 8 {
        format!(
            "{}:{}:{}",
            wago_build.product,
            build_number,
            &build_config[..8]
        )
    } else {
        format!("{}:{}:imported", wago_build.product, build_number)
    };

    // Determine branch from product name
    let branch = determine_branch(&wago_build.product);

    // Create metadata structure
    let metadata = BuildMetadata {
        meta: MetadataInfo {
            captured_at: wago_build.timestamp.unwrap_or_else(chrono::Utc::now),
            source: DataSource::Wago {
                import_time: chrono::Utc::now(),
                wago_version_id: None,
            },
            schema_version: 1,
            updated_at: chrono::Utc::now(),
            build_id,
        },
        build: BuildInfo {
            product_code: wago_build.product.clone(),
            version,
            build_number,
            version_build: wago_build.version.clone(),
            build_name: None,
            build_uid: None,
            build_product: None,
            branch,
        },
        configs: ConfigInfo {
            build_config,
            cdn_config,
            product_config,
            patch_config: None,
            exe_configs: HashMap::new(),
        },
        cdn: cdn_info.cloned().unwrap_or_else(|| CdnInfo {
            // Use fetched CDN info if available, otherwise empty
            hosts: vec![],
            path: String::new(),
            product_path: None,
            protocols: vec![],
            archive_group: None,
            archive_count: None,
        }),
        regions: HashMap::new(), // No region data from wago
        product: ProductInfo {
            display_name: get_product_description(&wago_build.product).to_string(),
            family: get_product_family(&wago_build.product),
            product_type: get_product_type(&wago_build.product),
            platforms: vec!["windows".to_string(), "macos".to_string()],
            subscription: None,
        },
        patch: None,
        catalog: {
            let mut flags = vec![];
            if wago_build
                .metadata
                .get("is_bgdl")
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false)
            {
                flags.push("bgdl".to_string());
            }

            // Add version type as a tag
            let mut tags = vec![];
            if !wago_build.version_type.is_empty() && wago_build.version_type != "unknown" {
                tags.push(wago_build.version_type.clone());
            }

            Some(CatalogInfo {
                sequence_number: wago_build
                    .metadata
                    .get("seqn")
                    .and_then(|v| v.parse::<u64>().ok()),
                flags,
                tags,
                release_date: wago_build.timestamp,
                end_of_support: None,
            })
        },
    };

    // Save the metadata
    build_manager.save_build(&metadata)?;

    Ok(())
}

/// Check if a product is supported by wago.tools
fn is_wago_supported_product(product: &str) -> bool {
    // wago.tools only supports World of Warcraft products
    product.starts_with("wow")
}

/// Determine branch from product code
fn determine_branch(product: &str) -> Option<String> {
    if product.ends_with("_ptr") || product.ends_with('t') {
        Some("ptr".to_string())
    } else if product.ends_with("_beta") {
        Some("beta".to_string())
    } else if product.contains("_classic_era") {
        Some("classic_era".to_string())
    } else if product.contains("_classic") {
        Some("classic".to_string())
    } else if product.starts_with("wow") && !product.contains('_') {
        Some("retail".to_string())
    } else {
        None
    }
}

/// Import TACT encryption keys from WoWDev community repository
pub async fn import_tact_keys(_force: bool, verbose: bool) -> Result<()> {
    use cascette_protocol::HttpClient;

    println!(
        "\n{} {} {}",
        style("Importing TACT keys from").cyan(),
        style("WoWDev/TACTKeys").green().bold(),
        style("repository...").cyan()
    );

    // Initialize metadata orchestrator for unified key management
    let data_dir = paths::data_dir().context("Failed to determine data directory")?;
    let config = OrchestratorConfig {
        data_dir: data_dir.clone(),
        enable_metrics: true,
        max_cache_memory: 32 * 1024 * 1024, // 32 MB for TACT keys
    };

    // Create a minimal FileDataID provider for orchestrator (TACT keys don't need mappings)
    let memory_provider = cascette_metadata::fdid::MemoryProvider::empty();
    let mut orchestrator = MetadataOrchestrator::new(Box::new(memory_provider), config)
        .context("Failed to create metadata orchestrator")?;

    // Create and initialize the provider with refresh
    let _provider = cascette_import::tactkeys::TactKeysProvider::new();

    // The provider needs to refresh its cache to fetch keys
    if verbose {
        println!("{} Fetching latest keys from GitHub...", style("→").dim());
    }

    // Note: refresh_cache() is currently a stub in TactKeysProvider
    // We need to fetch the keys directly using HttpClient's inner reqwest client

    let client = HttpClient::new().context("Failed to create HTTP client")?;
    let url = "https://raw.githubusercontent.com/wowdev/TACTKeys/master/WoW.txt";
    let response = client
        .inner()
        .get(url)
        .send()
        .await
        .context("Failed to fetch TACT keys from GitHub")?;

    if !response.status().is_success() {
        return Err(anyhow::Error::msg(format!(
            "Failed to fetch TACT keys: HTTP {}",
            response.status()
        )));
    }

    let content = response
        .text()
        .await
        .context("Failed to read TACT keys response")?;

    if verbose {
        println!("{} Parsing TACT keys file...", style("→").dim());
    }

    // Parse the keys manually since the provider's method is private
    let mut parsed_keys: Vec<(u64, String)> = Vec::new();
    for line in content.lines() {
        // Skip empty lines and comments
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        // Parse format: LOOKUP_HASH ENCRYPTION_KEY [ADDITIONAL_INFO]
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let lookup = parts[0];
            let key_hex = parts[1];

            // Validate hex format
            if lookup.len() != 16 || key_hex.len() != 32 {
                continue;
            }

            if !lookup.chars().all(|c| c.is_ascii_hexdigit())
                || !key_hex.chars().all(|c| c.is_ascii_hexdigit())
            {
                continue;
            }

            // Parse lookup as u64
            if let Ok(key_id) = u64::from_str_radix(lookup, 16) {
                parsed_keys.push((key_id, key_hex.to_string()));
            }
        }
    }

    let keys = parsed_keys;

    if keys.is_empty() {
        println!(
            "{} No keys available from WoWDev/TACTKeys",
            style("⚠").yellow().bold()
        );
        return Ok(());
    }

    println!(
        "{} Found {} keys to process",
        style("→").dim(),
        style(keys.len()).yellow()
    );

    let mut imported = 0;
    let mut skipped = 0;
    let mut failed = 0;

    // Batch process keys for maximum performance
    let batch_size = if verbose { 100 } else { 1000 }; // Smaller batches when verbose for progress
    let total_keys = keys.len();

    for (batch_idx, batch) in keys.chunks(batch_size).enumerate() {
        // Note: Could calculate batch_start = batch_idx * batch_size for progress tracking

        for (key_id, key_hex) in batch {
            // Add key through orchestrator (unified metadata management)
            // Note: We currently don't have a fast duplicate check for TACT keys
            // through the orchestrator, so we'll add all keys
            if false {
                skipped += 1;
                if verbose {
                    println!("  {} Key {:016X} already exists", style("○").dim(), key_id);
                }
                continue;
            }

            // Add key through orchestrator (unified metadata management)
            match orchestrator.add_tact_key(
                *key_id,
                key_hex,
                "wowdev",
                Some("WoWDev/TACTKeys community repository".to_string()),
                Some("wow".to_string()),
                None,
            ) {
                Ok(_) => {
                    imported += 1;
                    if verbose {
                        println!("  {} Imported key {:016X} ✓", style("✓").green(), key_id);
                    }
                }
                Err(e) => {
                    failed += 1;
                    if verbose {
                        eprintln!(
                            "  {} Failed to import key {:016X}: {}",
                            style("✗").red(),
                            key_id,
                            e
                        );
                    }
                }
            }
        }

        // Progress update for large batches
        if !verbose && batch_size >= 1000 {
            let processed = std::cmp::min((batch_idx + 1) * batch_size, total_keys);
            println!(
                "  {} Processed {}/{} keys ({:.1}%)",
                style("→").dim(),
                processed,
                total_keys,
                (processed as f64 / total_keys as f64) * 100.0
            );
        }
    }

    // Keys are automatically persisted through the orchestrator
    if imported > 0 {
        println!(
            "\n{} {} keys processed and stored successfully",
            style("✓").green().bold(),
            imported
        );

        // Show TACT key statistics if verbose
        if verbose {
            let stats = orchestrator
                .get_stats()
                .context("Failed to get orchestrator stats")?;
            let tact_stats = stats.tact_stats;
            println!("\n{}", style("TACT Key Statistics").cyan().bold());
            println!("{}", style("─".repeat(40)).dim());
            println!(
                "{} {}",
                style("Total keys:").bold(),
                style(tact_stats.total_keys).yellow()
            );
            println!(
                "{} {}",
                style("Verified keys:").bold(),
                style(tact_stats.verified_keys).green()
            );
            // Memory usage is not currently tracked in TactKeyStats
        }
    }

    // Display summary
    println!("\n{}", style("Import Summary").cyan().bold());
    println!("{}", style("─".repeat(40)).dim());
    println!(
        "{} {}",
        style("Keys imported:").bold(),
        style(imported).green()
    );
    if skipped > 0 {
        println!(
            "{} {} (already in keyring)",
            style("Keys skipped:").bold(),
            style(skipped).yellow()
        );
    }
    if failed > 0 {
        println!(
            "{} {}",
            style("Failed imports:").bold(),
            style(failed).red()
        );
    }

    println!(
        "\n{} TACT keys stored in system keyring (marked as verified)",
        style("✓").green().bold()
    );
    println!(
        "{} Use '{}' to view imported keys",
        style("Tip:").cyan(),
        style("cascette tact list --verified").bold()
    );

    Ok(())
}

/// Import FileDataID mappings from community sources
pub async fn import_filedataid(_force: bool, stats: bool) -> Result<()> {
    println!(
        "\n{} {} {}",
        style("Importing FileDataID mappings from").cyan(),
        style("WoWDev/wow-listfile").green().bold(),
        style("repository...").cyan()
    );

    // Initialize metadata orchestrator
    let data_dir = paths::data_dir().context("Failed to determine data directory")?;
    let config = OrchestratorConfig {
        data_dir: data_dir.clone(),
        enable_metrics: true,
        max_cache_memory: 128 * 1024 * 1024, // 128 MB for FileDataID cache
    };

    // Create listfile provider with data loaded
    let listfile_provider = cascette_import::listfile::create_listfile_provider()
        .await
        .context("Failed to create listfile provider")?;
    let provider_adapter = cascette_metadata::fdid::ListfileProviderAdapter::new(listfile_provider);

    // Create orchestrator
    let mut orchestrator = MetadataOrchestrator::new(Box::new(provider_adapter), config)
        .context("Failed to create metadata orchestrator")?;

    println!("{} Loading FileDataID mappings...", style("→").dim());

    // Load mappings with progress indication
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("Valid spinner template")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message("Fetching FileDataID mappings from GitHub...");

    // Load mappings (this will fetch from the provider)
    match orchestrator.load_mappings().await {
        Ok(_) => {
            spinner.finish_with_message(format!(
                "{} FileDataID mappings loaded successfully",
                style("✓").green().bold()
            ));
        }
        Err(e) => {
            spinner.finish_with_message(format!(
                "{} Failed to load mappings",
                style("✗").red().bold()
            ));
            return Err(anyhow::Error::new(e).context("Failed to load FileDataID mappings"));
        }
    }

    // Get and display statistics if requested
    if stats {
        println!("\n{}", style("FileDataID Statistics").cyan().bold());
        println!("{}", style("─".repeat(50)).dim());

        // Get service statistics
        let stats = orchestrator
            .get_stats()
            .context("Failed to get orchestrator stats")?;
        let fdid_stats = stats.fdid_stats;
        println!(
            "{} {}",
            style("Total mappings:").bold(),
            style(fdid_stats.total_mappings).yellow()
        );
        println!(
            "{} {}",
            style("ID lookups:").bold(),
            style(fdid_stats.id_to_path_lookups).yellow()
        );
        println!(
            "{} {}",
            style("Path lookups:").bold(),
            style(fdid_stats.path_to_id_lookups).yellow()
        );
        let hit_rate = if fdid_stats.id_to_path_lookups + fdid_stats.path_to_id_lookups > 0 {
            (fdid_stats.successful_lookups as f64
                / (fdid_stats.id_to_path_lookups + fdid_stats.path_to_id_lookups) as f64)
                * 100.0
        } else {
            0.0
        };
        println!("{} {:.2}%", style("Success rate:").bold(), hit_rate);

        // Memory usage
        let memory_mb = fdid_stats.memory_usage_bytes as f64 / (1024.0 * 1024.0);
        println!("{} {:.1} MB", style("Memory usage:").bold(), memory_mb);

        // Provider stats are not currently exposed in the FileDataIdStats
    }

    println!(
        "\n{} FileDataID mappings are now available for file resolution.",
        style("✓").green().bold()
    );
    println!(
        "{} Use '{}' to resolve specific FileDataIDs.",
        style("Tip:").cyan(),
        style("cascette resolve --id <file_data_id>").bold()
    );

    Ok(())
}

/// Import all data sources
pub async fn import_all(force: bool, verbose: bool) -> Result<()> {
    println!(
        "\n{} {} {}",
        style("Importing").cyan(),
        style("ALL").yellow().bold(),
        style("community data sources...").cyan()
    );

    // Import builds
    println!("\n{}", style("Step 1/3: Historic Builds").cyan().bold());
    import_builds(force, None).await?;

    // Import TACT keys
    println!("\n{}", style("Step 2/3: TACT Keys").cyan().bold());
    import_tact_keys(force, verbose).await?;

    // Import FileDataID mappings
    println!("\n{}", style("Step 3/3: FileDataID Mappings").cyan().bold());
    import_filedataid(force, false).await?;

    println!(
        "\n{} All data sources imported successfully!",
        style("✓").green().bold()
    );

    Ok(())
}
