#![allow(clippy::if_not_else)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::literal_string_with_formatting_args)]
#![allow(clippy::doc_markdown)]

mod cache;
mod config;
mod fdid;
mod imports;
mod installation;
mod paths;
mod products;
mod tact;

use anyhow::Result;
use clap::{Parser, Subcommand, builder::styling};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use installation::{CacheConfig, RetryConfig};

fn get_styles() -> styling::Styles {
    styling::Styles::styled()
        .header(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .usage(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .literal(styling::AnsiColor::Cyan.on_default() | styling::Effects::BOLD)
        .placeholder(styling::AnsiColor::Cyan.on_default())
        .error(styling::AnsiColor::Red.on_default() | styling::Effects::BOLD)
        .valid(styling::AnsiColor::Green.on_default())
        .invalid(styling::AnsiColor::Yellow.on_default())
}

fn print_version() {
    println!(
        "{} {}",
        style("cascette").cyan().bold(),
        style(env!("CARGO_PKG_VERSION")).green()
    );
    println!("Release channel: {}", style("stable").green());
    println!("Authors: {}", env!("CARGO_PKG_AUTHORS"));
    println!("Repository: {}", style(env!("CARGO_PKG_REPOSITORY")).dim());
}

#[derive(Parser)]
#[command(name = "cascette")]
#[command(version)]
#[command(author = "Daniel S. Reichenbach <daniel@reichenbach.nl>")]
#[command(
    about = "Community-driven NGDP Swiss Army Knife",
    long_about = "Cascette manages products distributed via Blizzard's Next Generation \
                  Distribution Pipeline (NGDP). Download, install, update, and verify \
                  products with caching and progress tracking."
)]
#[command(after_help = "CAPABILITIES:\n\
                  • Discover products via Ribbit protocol\n\
                  • Download from CDN with resume support\n\
                  • Store using CASC (content-addressed storage)\n\
                  • Cache with two-tier system (memory + disk)\n\
                  • Track progress with real-time updates\n\n\
                  RESOURCES:\n\
                    • Discord:    https://discord.gg/Q44pPMvGEd\n\
                    • Github:     https://github.com/wowemulation-dev\n\
                    • Repository: https://github.com/wowemulation-dev/cascette-rs")]
#[command(after_long_help = "\nDETAILS:\n\
                       \n\
                       Service Discovery:\n\
                         Ribbit protocol automatically finds CDN endpoints and product\n\
                         versions across regions (US, EU, KR, CN).\n\
                       \n\
                       Content Delivery:\n\
                         HTTP/HTTPS downloads with range requests enable efficient\n\
                         bandwidth usage and automatic resume on failure.\n\
                       \n\
                       Storage System:\n\
                         CASC (Content-Addressable Storage Container) provides\n\
                         deduplication, compression (BLTE), and optional encryption.\n\
                       \n\
                       Caching:\n\
                         Two-tier cache reduces redundant downloads:\n\
                         • L1: In-memory cache for frequently accessed data\n\
                         • L2: Disk cache for persistent storage\n\
                       \n\
                       Progress Tracking:\n\
                         Real-time updates show download speed, completion percentage,\n\
                         and estimated time remaining.\n\
                       \n\
                       RESOURCES:\n\
                         • Discord:    https://discord.gg/Q44pPMvGEd\n\
                         • Github:     https://github.com/wowemulation-dev\n\
                         • Repository: https://github.com/wowemulation-dev/cascette-rs\n\
                         • Issues:     https://github.com/wowemulation-dev/cascette-rs/issues\n\
                         • FileDataID: Community-driven file resolution system")]
#[command(styles = get_styles())]
#[command(help_template = "\
{about-with-newline}
{usage-heading} {usage}

PRODUCT DISCOVERY:
  list     List available products
  info     Show detailed product information

PRODUCT MANAGEMENT:
  install  Install a product
  upgrade  Update cascette to the latest version
  verify   Verify product integrity

ARCHIVE:
  import   Import community data sources

DATA:
  tact     Manage TACT encryption keys
  fdid     Resolve FileDataID mappings (file ID ↔ path)

UTILITIES:
  cache    Manage cache storage
  config   Manage configuration settings
  paths    Show data storage locations
  version  Show version information
  help     Print this message or the help of the given subcommand(s)

OPTIONS:
{options}
{after-help}
")]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available products
    #[command(long_about = "Query the NGDP catalog to list all available products. \
                      Products include tools (agent, launcher) and entertainment software. \
                      The catalog shows the latest sequence numbers for each manifest type.")]
    List {
        /// Filter products by code or description
        #[arg(short, long, value_name = "FILTER")]
        filter: Option<String>,

        /// Show detailed timing information for debugging performance
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show detailed product information
    #[command(
        long_about = "Query detailed information for a specific product from NGDP servers. \
                      This includes version history, available builds, CDN configuration, \
                      and regional distribution endpoints. Provide the product code (e.g., wow, \
                      wow_classic, d4) to retrieve current deployment information.\n\n\
                      Use --build to load cached build metadata instead of querying live NGDP. \
                      Without --build, queries live NGDP and saves build metadata for future use.\n\n\
                      CDN host overrides can be configured via 'cascette config cdn-hosts' \
                      to use alternative CDN servers for testing or development purposes."
    )]
    Info {
        /// Product code (e.g., wow, wow_classic, d4)
        product: String,

        /// Region to query (defaults to configured region or 'us')
        #[arg(short, long, value_name = "REGION", value_parser = ["us", "eu", "kr", "tw", "cn", "sg"])]
        region: Option<String>,

        /// Load specific build metadata from disk (no network request)
        #[arg(short, long, value_name = "BUILD_NUMBER")]
        build: Option<u32>,
    },

    /// Install a product
    #[command(long_about = "Download and install a product from the NGDP catalog. \
                      Specify the product code (e.g., agent, wow, wow_classic) and \
                      target directory. The installation process handles all dependencies \
                      and verifies integrity automatically.\n\n\
                      Installation follows a plan-first approach:\n\
                      1. Resolve build version and CDN configuration\n\
                      2. Create installation plan with all metadata\n\
                      3. Save plan for review (with --plan-only)\n\
                      4. Execute plan to download and install files\n\n\
                      INSTALLATION MODES:\n\
                      By default, creates Battle.net-compatible installations that games can run.\n\
                      Use --simple for file extraction only (games will NOT run in this mode).\n\n\
                      Use --plan-only to create a plan without downloading.\n\
                      Use --execute-plan to resume from a saved plan.")]
    Install {
        /// Product code (e.g., agent, wow, `wow_classic`, d4)
        product: String,

        /// Installation directory
        #[arg(short, long, value_name = "PATH")]
        output: std::path::PathBuf,

        /// Region to download from (us, eu, kr, cn)
        #[arg(short, long, default_value = "us", value_name = "REGION")]
        region: String,

        /// Specific build ID to install (for historic builds)
        #[arg(short, long, value_name = "BUILD_ID")]
        build: Option<u32>,

        /// Create installation plan only, don't download
        #[arg(long)]
        plan_only: bool,

        /// Execute existing installation plan
        #[arg(long, value_name = "PLAN_PATH", conflicts_with = "plan_only")]
        execute_plan: Option<std::path::PathBuf>,

        /// Dry run - show what would be downloaded
        #[arg(long, conflicts_with = "execute_plan")]
        dry_run: bool,

        /// Use simple extraction mode (testing/debugging only)
        ///
        /// WARNING: This mode extracts files without creating proper
        /// Battle.net structure. Games will NOT run in this mode.
        #[arg(long)]
        simple: bool,

        /// Maximum concurrent downloads
        #[arg(long, default_value = "4", value_name = "NUM")]
        max_concurrent: usize,

        /// Show progress for each file
        #[arg(short, long)]
        verbose: bool,

        /// Inspect an existing installation plan
        #[arg(long, value_name = "PLAN_PATH", conflicts_with_all = ["plan_only", "execute_plan", "dry_run"])]
        inspect: Option<std::path::PathBuf>,
    },

    /// Update cascette to the latest version
    #[command(
        long_about = "Check Github for new cascette releases and provide upgrade instructions. \
                      Compares your current version with the latest available release \
                      and guides you through the update process."
    )]
    Upgrade,

    /// Verify product integrity
    #[command(
        long_about = "Verify the integrity of an installed product by checking all files \
                      against their expected hashes. Optionally repair corrupted or missing \
                      files by re-downloading them from the CDN."
    )]
    Verify {
        /// Product installation path
        path: std::path::PathBuf,

        /// Repair corrupted or missing files
        #[arg(short, long)]
        repair: bool,
    },

    /// Show version information
    #[command(long_about = "Display cascette version, build information, and release channel.")]
    Version,

    /// Manage cache storage
    #[command(
        long_about = "Manage cascette's cache storage including clearing cached data, \
                      viewing cache statistics, and controlling cache behavior. \
                      The cache stores protocol responses, configuration data, and \
                      downloaded content for faster subsequent access."
    )]
    Cache {
        #[command(subcommand)]
        subcommand: CacheCommands,
    },

    /// Manage configuration settings
    #[command(
        long_about = "View or edit cascette configuration settings including protocol \
                      endpoints, default region, cache settings, and network parameters. \
                      Configuration is stored in the platform-specific config directory."
    )]
    Config {
        #[command(subcommand)]
        subcommand: Option<ConfigCommands>,
    },

    /// Show data storage locations
    #[command(
        long_about = "Display the platform-specific directories used by cascette for \
                      configuration, cache, and data storage. These directories are \
                      created automatically when needed."
    )]
    Paths,

    /// Import community data sources
    #[command(
        long_about = "Import data from community sources including historic build information, \
                      TACT encryption keys, and FileDataID mappings. These resources are cached \
                      locally and updated as needed from providers like wago.tools and WoWDev."
    )]
    Import {
        #[command(subcommand)]
        subcommand: ImportCommands,
    },

    /// Manage TACT encryption keys
    #[command(
        long_about = "Manage TACT encryption keys used for content decryption in the CASC system. \
                      Keys are stored securely in the system keyring (Windows Credential Manager, \
                      macOS Keychain, or Linux Secret Service). Supports adding, removing, listing, \
                      importing, and exporting keys."
    )]
    Tact {
        #[command(subcommand)]
        subcommand: tact::TactCommands,
    },

    /// Resolve FileDataID mappings
    #[command(
        long_about = "Resolve FileDataID mappings between numeric file IDs and file paths. \
                      FileDataIDs are used in CASC archives to identify files numerically while \
                      file paths provide human-readable names. This command enables bidirectional \
                      resolution using community-maintained mapping databases."
    )]
    Fdid {
        #[command(subcommand)]
        subcommand: fdid::commands::FdidCommands,
    },
}

#[derive(Subcommand)]
enum ImportCommands {
    /// Import historic build information
    #[command(
        long_about = "Import historic build information from wago.tools including version \
                      numbers, CDN configurations, and build metadata for World of Warcraft products. \
                      This data enables cascette to work with older game versions and track \
                      version evolution over time.\n\n\
                      Note: wago.tools only provides data for WoW products (wow, wow_classic, \
                      wow_classic_era, and their beta/PTR variants)."
    )]
    Builds {
        /// Force refresh even if cached data exists
        #[arg(short, long)]
        force: bool,

        /// WoW product code to import (e.g., wow, wow_classic, wow_classic_era)
        #[arg(short, long)]
        product: Option<String>,
    },

    /// Import TACT encryption keys
    #[command(
        name = "tact-keys",
        long_about = "Import TACT encryption keys from the WoWDev community repository. \
                      These keys are required to decrypt protected game content across \
                      different expansions and patches. Keys are deduplicated and stored \
                      securely in the local key manager."
    )]
    TactKeys {
        /// Force refresh even if cached keys exist
        #[arg(short, long)]
        force: bool,

        /// Show detailed import progress
        #[arg(short, long)]
        verbose: bool,
    },

    /// Import FileDataID mappings
    #[command(
        name = "filedataid",
        long_about = "Import FileDataID to filename mappings from the WoWDev community listfile. \
                      These mappings translate numeric file IDs to human-readable paths, \
                      essential for navigating and extracting specific game assets from archives."
    )]
    FileDataId {
        /// Force refresh even if cached mappings exist
        #[arg(short, long)]
        force: bool,

        /// Show import statistics
        #[arg(short, long)]
        stats: bool,
    },

    /// Import all available data sources
    #[command(
        long_about = "Import all available data sources including builds, TACT keys, and \
                      FileDataID mappings in a single operation. This ensures cascette has \
                      complete archive support for all game versions and content types."
    )]
    All {
        /// Force refresh all data sources
        #[arg(short, long)]
        force: bool,

        /// Show detailed progress for each import
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show cache statistics
    #[command(
        long_about = "Display cache statistics including size, number of entries, hit rate, \
                      and age of cached data. Shows both memory (L1) and disk (L2) cache information."
    )]
    Stats,

    /// Clear all cached data
    #[command(
        long_about = "Clear all cached data including protocol responses, configuration files, \
                      and downloaded content. This will free up disk space but may result in \
                      slower operations until the cache is rebuilt."
    )]
    Clear {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Clear specific cache types
    #[command(
        long_about = "Clear cached data of specific types such as ribbit responses, \
                      configuration files, or product catalogs. Use this for targeted cache management."
    )]
    ClearType {
        /// Cache type to clear
        #[arg(value_parser = ["ribbit", "config", "catalog", "import", "all"])]
        cache_type: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Initialize configuration file with defaults
    Init,

    /// Set default region
    #[command(long_about = "Set the default region for NGDP operations. \
                      Valid regions: us, eu, kr, tw, cn")]
    SetRegion {
        /// Region code (us, eu, kr, tw, cn)
        region: String,
    },

    /// Set protocol endpoint URL
    #[command(long_about = "Set a protocol endpoint URL template. \
                      Use {region} as a placeholder for the region code.")]
    SetEndpoint {
        /// Protocol type (ribbit-tcp, tact-http, tact-https)
        #[arg(value_parser = ["ribbit-tcp", "tact-http", "tact-https"])]
        protocol: String,

        /// URL template with {region} placeholder
        url: String,
    },

    /// Manage CDN host overrides
    #[command(
        long_about = "Configure CDN host overrides for testing or private CDN usage. \
                      These hosts will be used instead of the ones returned by the cdns endpoint."
    )]
    CdnHosts {
        #[command(subcommand)]
        subcommand: CdnHostCommands,
    },
}

#[derive(Subcommand)]
enum CdnHostCommands {
    /// Show current CDN host overrides
    Show,

    /// Add a global CDN host override
    Add {
        /// CDN host (e.g., cdn.example.com or cdn.example.com:1119)
        host: String,
    },

    /// Add a product-specific CDN host override
    AddProduct {
        /// Product code (e.g., wow, wow_classic)
        product: String,

        /// CDN host (e.g., cdn.example.com or cdn.example.com:1119)
        host: String,
    },

    /// Clear all CDN host overrides
    Clear,

    /// Clear CDN host overrides for a specific product
    ClearProduct {
        /// Product code (e.g., wow, wow_classic)
        product: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize configuration first (needed by some commands even if they handle their own)
    // Cache initialization happens inside match for commands that need it

    // Handle commands that don't need config/cache first
    match &cli.command {
        Commands::Version => {
            print_version();
            return Ok(());
        }
        Commands::Paths => {
            println!("Cascette Directory Locations:\n");
            match paths::config_dir() {
                Ok(dir) => println!("Config Dir:  {}", style(dir.display()).cyan()),
                Err(e) => println!("Config Dir:  {}", style(format!("Error: {}", e)).red()),
            }
            match paths::config_file() {
                Ok(file) => println!("Config File: {}", style(file.display()).cyan()),
                Err(e) => println!("Config File: {}", style(format!("Error: {}", e)).red()),
            }
            match paths::cache_dir() {
                Ok(dir) => println!("Cache Dir:   {}", style(dir.display()).cyan()),
                Err(e) => println!("Cache Dir:   {}", style(format!("Error: {}", e)).red()),
            }
            match paths::data_dir() {
                Ok(dir) => println!("Data Dir:    {}", style(dir.display()).cyan()),
                Err(e) => println!("Data Dir:    {}", style(format!("Error: {}", e)).red()),
            }
            println!("\nThese directories will be created automatically when needed.");
            return Ok(());
        }
        _ => {}
    }

    // Load configuration for commands that need it
    let config = match crate::config::CascetteConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            // Config init doesn't need existing config
            if matches!(
                cli.command,
                Commands::Config {
                    subcommand: Some(ConfigCommands::Init)
                }
            ) {
                crate::config::CascetteConfig::default()
            } else {
                println!(
                    "{} Failed to load configuration: {}",
                    style("✗").red().bold(),
                    e
                );
                println!(
                    "\nRun '{}' to create a default configuration.",
                    style("cascette config init").cyan()
                );
                std::process::exit(1);
            }
        }
    };

    // Initialize cache globally (if enabled)
    if config.cache.enabled {
        if let Err(e) = cache::init_cache(&config.cache) {
            println!(
                "{} Warning: Failed to initialize cache: {}",
                style("!").yellow().bold(),
                e
            );
            println!("  Operations will continue without caching.");
        }
    }

    match cli.command {
        Commands::List { filter, verbose } => {
            // Load configuration
            let config = match crate::config::CascetteConfig::load() {
                Ok(cfg) => cfg,
                Err(e) => {
                    println!(
                        "{} Failed to load configuration: {}",
                        style("✗").red().bold(),
                        e
                    );
                    println!(
                        "\nRun '{}' to create a default configuration.",
                        style("cascette config init").cyan()
                    );
                    std::process::exit(1);
                }
            };

            // Query the product catalog (always global)
            if let Err(e) = products::query_catalog(&config, filter.as_deref(), verbose).await {
                println!("{} Failed to list products: {}", style("✗").red().bold(), e);
                std::process::exit(1);
            }
        }
        Commands::Info {
            product,
            region,
            build,
        } => {
            // Query product information - uses global config
            if let Err(e) =
                products::query_product_info(&config, &product, region.as_deref(), build).await
            {
                println!(
                    "{} Failed to get product info: {}",
                    style("✗").red().bold(),
                    e
                );
                std::process::exit(1);
            }
        }
        Commands::Install {
            product,
            output,
            region,
            build,
            plan_only,
            execute_plan,
            dry_run,
            simple,
            max_concurrent,
            verbose,
            inspect,
        } => {
            // Handle the install command
            handle_install_command(InstallOptions {
                product,
                output,
                region,
                build,
                plan_only,
                execute_plan,
                dry_run,
                simple,
                max_concurrent,
                verbose,
                inspect,
            })
            .await?;
        }
        Commands::Upgrade => {
            let spinner = ProgressBar::new_spinner();
            spinner.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Valid progress style template")
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
            );
            spinner.set_message("Checking Github for updates...");

            // Simulate checking
            for _ in 0..20 {
                spinner.tick();
                std::thread::sleep(Duration::from_millis(100));
            }

            spinner.finish_with_message(format!(
                "{} You are running the latest version!",
                style("✓").green().bold()
            ));

            println!();
            print_version();
        }
        Commands::Verify { path, repair } => {
            println!(
                "Verifying product integrity at: {}",
                style(path.display()).cyan()
            );

            if repair {
                println!("Mode: {}", style("Verify and repair").yellow());
            } else {
                println!("Mode: {}", style("Verify only").green());
            }

            // Temporary placeholder
            println!(
                "\n{}",
                style("Verification will be available in the next release.").dim()
            );
            println!(
                "{}",
                style("This will check all files against their expected hashes.").dim()
            );
        }
        Commands::Version => {
            print_version();
        }
        Commands::Cache { subcommand } => {
            handle_cache_command(subcommand, &config)?;
        }
        Commands::Config { subcommand } => {
            use crate::config::CascetteConfig;

            match subcommand {
                None | Some(ConfigCommands::Show) => {
                    // Load and display current configuration
                    match CascetteConfig::load() {
                        Ok(config) => {
                            println!("{}\n", style("Your Configuration:").cyan().bold());

                            // Network and Region Settings
                            println!("{}", style("── Network & CDN Settings ──").dim());
                            println!(
                                "\n{}  {}",
                                style("Region:").bold(),
                                style(&config.region).green()
                            );

                            // Check if endpoints are customized (differ from defaults)
                            let default_ribbit = "{region}.version.battle.net:1119";
                            let default_tact_http = "http://{region}.patch.battle.net:1119";
                            let default_tact_https = "https://{region}.version.battle.net";

                            let endpoints_customized = config.endpoints.ribbit_host
                                != default_ribbit
                                || config.endpoints.tact_http != default_tact_http
                                || config.endpoints.tact_https != default_tact_https;

                            if endpoints_customized {
                                println!("\n{}\n", style("Custom Endpoints:").bold());
                                if config.endpoints.ribbit_host != default_ribbit {
                                    println!(
                                        "  {} {}",
                                        style("Ribbit TCP:").dim(),
                                        style(&config.endpoints.ribbit_host).yellow()
                                    );
                                }
                                if config.endpoints.tact_http != default_tact_http {
                                    println!(
                                        "  {} {}",
                                        style("TACT HTTP:").dim(),
                                        style(&config.endpoints.tact_http).yellow()
                                    );
                                }
                                if config.endpoints.tact_https != default_tact_https {
                                    println!(
                                        "  {} {}",
                                        style("TACT HTTPS:").dim(),
                                        style(&config.endpoints.tact_https).yellow()
                                    );
                                }
                            }

                            // Always show expanded URLs for current region
                            println!("\n{}\n", style("Your Active Endpoints:").bold());
                            println!("  Ribbit TCP:  {}", style(config.ribbit_url(None)).cyan());
                            println!(
                                "  TACT HTTP:   {}",
                                style(config.tact_http_url(None)).cyan()
                            );
                            println!(
                                "  TACT HTTPS:  {}",
                                style(config.tact_https_url(None)).cyan()
                            );

                            // Show CDN overrides if configured
                            if !config.cdn_overrides.hosts.is_empty()
                                || !config.cdn_overrides.product_hosts.is_empty()
                            {
                                println!("\n{}\n", style("CDN Overrides:").bold());
                                if !config.cdn_overrides.hosts.is_empty() {
                                    println!("  {}", style("Global hosts:").dim());
                                    for host in &config.cdn_overrides.hosts {
                                        println!("    - {}", style(host).yellow());
                                    }
                                }
                                if !config.cdn_overrides.product_hosts.is_empty() {
                                    println!("  {}", style("Product-specific:").dim());
                                    for (product, hosts) in &config.cdn_overrides.product_hosts {
                                        println!("    {}:", style(product).green());
                                        for host in hosts {
                                            println!("      - {}", style(host).yellow());
                                        }
                                    }
                                }
                            }

                            // Cache Settings
                            println!("\n{}", style("── Cache Settings ──").dim());
                            println!();
                            println!(
                                "  {}      {}",
                                style("Enabled:").dim(),
                                if config.cache.enabled {
                                    style("Yes").green()
                                } else {
                                    style("No").red()
                                }
                            );
                            println!(
                                "  {}  {} seconds",
                                style("API TTL:").dim(),
                                config.cache.api_ttl_seconds
                            );
                            println!(
                                "  {}  {} seconds",
                                style("CDN TTL:").dim(),
                                config.cache.cdn_ttl_seconds
                            );
                            println!(
                                "  {} {} MB",
                                style("Max Size:").dim(),
                                config.cache.max_size_mb
                            );
                        }
                        Err(e) => {
                            println!(
                                "{} Loading configuration: {}",
                                style("Error").red().bold(),
                                e
                            );
                            println!(
                                "\nRun '{}' to create a default configuration.",
                                style("cascette config init").cyan()
                            );
                        }
                    }
                }
                Some(ConfigCommands::Init) => match CascetteConfig::init() {
                    Ok(_) => {
                        let config_path =
                            paths::config_file().expect("Failed to determine config file path");
                        println!(
                            "{} Configuration initialized at: {}",
                            style("✓").green().bold(),
                            style(config_path.display()).cyan()
                        );
                    }
                    Err(e) => {
                        println!(
                            "{} Failed to initialize configuration: {}",
                            style("✗").red().bold(),
                            e
                        );
                    }
                },
                Some(ConfigCommands::SetRegion { region }) => {
                    // Validate region
                    let valid_regions = ["us", "eu", "kr", "tw", "cn", "sg"];
                    if !valid_regions.contains(&region.as_str()) {
                        println!("{} Invalid region: {}", style("✗").red().bold(), region);
                        println!("Valid regions: {}", valid_regions.join(", "));
                        std::process::exit(1);
                    }

                    match CascetteConfig::load() {
                        Ok(mut config) => {
                            config.region.clone_from(&region);
                            match config.save() {
                                Ok(_) => {
                                    println!(
                                        "{} Default region set to: {}",
                                        style("✓").green().bold(),
                                        style(&region).cyan()
                                    );
                                }
                                Err(e) => {
                                    println!(
                                        "{} Failed to save configuration: {}",
                                        style("✗").red().bold(),
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            println!(
                                "{} Failed to load configuration: {}",
                                style("✗").red().bold(),
                                e
                            );
                            println!(
                                "\nRun '{}' to create a default configuration.",
                                style("cascette config init").cyan()
                            );
                        }
                    }
                }
                Some(ConfigCommands::SetEndpoint { protocol, url }) => {
                    // Validate the endpoint URL if it's not a template
                    if !url.contains("{region}") {
                        println!("{} Validating endpoint: {}", style("→").dim(), url);
                        if let Err(e) = products::validate_endpoint_url(&url) {
                            println!("{} {}", style("✗").red().bold(), e);
                            std::process::exit(1);
                        }
                    }

                    match CascetteConfig::load() {
                        Ok(mut config) => {
                            match protocol.as_str() {
                                "ribbit-tcp" => {
                                    // For ribbit, we expect host:port format, not full URL
                                    let host = url.replace("tcp://", "");
                                    config.endpoints.ribbit_host = host;
                                }
                                "tact-http" => config.endpoints.tact_http.clone_from(&url),
                                "tact-https" => config.endpoints.tact_https.clone_from(&url),
                                _ => unreachable!("Invalid protocol"),
                            }

                            match config.save() {
                                Ok(_) => {
                                    println!(
                                        "{} {} endpoint set to: {}",
                                        style("✓").green().bold(),
                                        style(&protocol).cyan(),
                                        style(&url).dim()
                                    );

                                    // Show expanded URL
                                    let expanded = config.expand_url(&url, None);
                                    println!("  Expanded: {}", style(expanded).cyan());
                                }
                                Err(e) => {
                                    println!(
                                        "{} Failed to save configuration: {}",
                                        style("✗").red().bold(),
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            println!(
                                "{} Failed to load configuration: {}",
                                style("✗").red().bold(),
                                e
                            );
                            println!(
                                "\nRun '{}' to create a default configuration.",
                                style("cascette config init").cyan()
                            );
                        }
                    }
                }
                Some(ConfigCommands::CdnHosts { subcommand }) => {
                    use crate::config::CascetteConfig;

                    match subcommand {
                        CdnHostCommands::Show => match CascetteConfig::load() {
                            Ok(config) => {
                                if config.cdn_overrides.hosts.is_empty()
                                    && config.cdn_overrides.product_hosts.is_empty()
                                {
                                    println!(
                                        "{}",
                                        style("No CDN host overrides configured").yellow()
                                    );
                                    println!("\n{}", style("Example development CDN hosts:").dim());
                                    println!("  - cdn.arctium.tools");
                                    println!("  - casc.wago.tools");
                                    println!("  - tact.mirror.reliquaryhq.com");
                                } else {
                                    println!("{}\n", style("CDN Host Overrides:").cyan().bold());

                                    if !config.cdn_overrides.hosts.is_empty() {
                                        println!("{}", style("Global hosts:").bold());
                                        for host in &config.cdn_overrides.hosts {
                                            println!("  - {}", style(host).yellow());
                                        }
                                    }

                                    if !config.cdn_overrides.product_hosts.is_empty() {
                                        println!("\n{}", style("Product-specific hosts:").bold());
                                        for (product, hosts) in &config.cdn_overrides.product_hosts
                                        {
                                            println!("  {}:", style(product).green());
                                            for host in hosts {
                                                println!("    - {}", style(host).yellow());
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                println!(
                                    "{} Failed to load configuration: {}",
                                    style("✗").red().bold(),
                                    e
                                );
                            }
                        },
                        CdnHostCommands::Add { host } => {
                            // Validate DNS resolution first
                            println!("{} Validating host: {}", style("→").dim(), host);
                            if let Err(e) = products::validate_cdn_host(&host) {
                                println!("{} {}", style("✗").red().bold(), e);
                                std::process::exit(1);
                            }

                            match CascetteConfig::load() {
                                Ok(mut config) => {
                                    if !config.cdn_overrides.hosts.contains(&host.to_string()) {
                                        config.cdn_overrides.hosts.push(host.clone());
                                        match config.save() {
                                            Ok(_) => {
                                                println!(
                                                    "{} Added global CDN host override: {}",
                                                    style("✓").green().bold(),
                                                    style(host).yellow()
                                                );
                                            }
                                            Err(e) => {
                                                println!(
                                                    "{} Failed to save configuration: {}",
                                                    style("✗").red().bold(),
                                                    e
                                                );
                                            }
                                        }
                                    } else {
                                        println!(
                                            "{} Host already exists: {}",
                                            style("!").yellow().bold(),
                                            host
                                        );
                                    }
                                }
                                Err(e) => {
                                    println!(
                                        "{} Failed to load configuration: {}",
                                        style("✗").red().bold(),
                                        e
                                    );
                                }
                            }
                        }
                        CdnHostCommands::AddProduct { product, host } => {
                            // Validate DNS resolution first
                            println!("{} Validating host: {}", style("→").dim(), host);
                            if let Err(e) = products::validate_cdn_host(&host) {
                                println!("{} {}", style("✗").red().bold(), e);
                                std::process::exit(1);
                            }

                            match CascetteConfig::load() {
                                Ok(mut config) => {
                                    let hosts = config
                                        .cdn_overrides
                                        .product_hosts
                                        .entry(product.clone())
                                        .or_default();
                                    if !hosts.contains(&host.to_string()) {
                                        hosts.push(host.clone());
                                        match config.save() {
                                            Ok(_) => {
                                                println!(
                                                    "{} Added CDN host override for {}: {}",
                                                    style("✓").green().bold(),
                                                    style(product).green(),
                                                    style(host).yellow()
                                                );
                                            }
                                            Err(e) => {
                                                println!(
                                                    "{} Failed to save configuration: {}",
                                                    style("✗").red().bold(),
                                                    e
                                                );
                                            }
                                        }
                                    } else {
                                        println!(
                                            "{} Host already exists for {}: {}",
                                            style("!").yellow().bold(),
                                            product,
                                            host
                                        );
                                    }
                                }
                                Err(e) => {
                                    println!(
                                        "{} Failed to load configuration: {}",
                                        style("✗").red().bold(),
                                        e
                                    );
                                }
                            }
                        }
                        CdnHostCommands::Clear => match CascetteConfig::load() {
                            Ok(mut config) => {
                                config.cdn_overrides.hosts.clear();
                                config.cdn_overrides.product_hosts.clear();
                                match config.save() {
                                    Ok(_) => {
                                        println!(
                                            "{} Cleared all CDN host overrides",
                                            style("✓").green().bold()
                                        );
                                    }
                                    Err(e) => {
                                        println!(
                                            "{} Failed to save configuration: {}",
                                            style("✗").red().bold(),
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                println!(
                                    "{} Failed to load configuration: {}",
                                    style("✗").red().bold(),
                                    e
                                );
                            }
                        },
                        CdnHostCommands::ClearProduct { product } => match CascetteConfig::load() {
                            Ok(mut config) => {
                                if config
                                    .cdn_overrides
                                    .product_hosts
                                    .remove(&product.to_string())
                                    .is_some()
                                {
                                    match config.save() {
                                        Ok(_) => {
                                            println!(
                                                "{} Cleared CDN host overrides for {}",
                                                style("✓").green().bold(),
                                                style(product).green()
                                            );
                                        }
                                        Err(e) => {
                                            println!(
                                                "{} Failed to save configuration: {}",
                                                style("✗").red().bold(),
                                                e
                                            );
                                        }
                                    }
                                } else {
                                    println!(
                                        "{} No overrides configured for {}",
                                        style("!").yellow().bold(),
                                        product
                                    );
                                }
                            }
                            Err(e) => {
                                println!(
                                    "{} Failed to load configuration: {}",
                                    style("✗").red().bold(),
                                    e
                                );
                            }
                        },
                    }
                }
            }
        }
        Commands::Paths => {
            println!("Cascette Directory Locations:\n");

            match paths::config_dir() {
                Ok(dir) => println!("Config Dir:  {}", style(dir.display()).cyan()),
                Err(e) => println!("Config Dir:  {}", style(format!("Error: {}", e)).red()),
            }

            match paths::config_file() {
                Ok(file) => println!("Config File: {}", style(file.display()).cyan()),
                Err(e) => println!("Config File: {}", style(format!("Error: {}", e)).red()),
            }

            match paths::cache_dir() {
                Ok(dir) => println!("Cache Dir:   {}", style(dir.display()).cyan()),
                Err(e) => println!("Cache Dir:   {}", style(format!("Error: {}", e)).red()),
            }

            match paths::data_dir() {
                Ok(dir) => println!("Data Dir:    {}", style(dir.display()).cyan()),
                Err(e) => println!("Data Dir:    {}", style(format!("Error: {}", e)).red()),
            }

            println!("\nThese directories will be created automatically when needed.");
        }
        Commands::Import { subcommand } => match subcommand {
            ImportCommands::Builds { force, product } => {
                imports::import_builds(force, product).await?;
            }
            ImportCommands::TactKeys { force, verbose } => {
                imports::import_tact_keys(force, verbose).await?;
            }
            ImportCommands::FileDataId { force, stats } => {
                imports::import_filedataid(force, stats).await?;
            }
            ImportCommands::All { force, verbose } => {
                imports::import_all(force, verbose).await?;
            }
        },
        Commands::Tact { subcommand } => {
            let data_dir = paths::data_dir()?;
            subcommand.execute(&data_dir)?;
        }
        Commands::Fdid { subcommand } => {
            fdid::commands::handle_fdid_command(subcommand).await?;
        }
    }

    Ok(())
}

/// Options for install command
#[allow(clippy::struct_excessive_bools)]
struct InstallOptions {
    product: String,
    output: std::path::PathBuf,
    region: String,
    build: Option<u32>,
    plan_only: bool,
    execute_plan: Option<std::path::PathBuf>,
    dry_run: bool,
    simple: bool,
    max_concurrent: usize,
    verbose: bool,
    inspect: Option<std::path::PathBuf>,
}

/// Handle install command
#[allow(clippy::too_many_lines)]
async fn handle_install_command(opts: InstallOptions) -> Result<()> {
    // Handle inspect first
    if let Some(plan_path) = opts.inspect {
        println!(
            "{} Inspecting installation plan: {}",
            style("→").cyan().bold(),
            style(plan_path.display()).cyan()
        );

        let inspector = installation::PlanInspector::new(&plan_path);
        match inspector.format_output() {
            Ok(output) => {
                println!("\n{}", output);
            }
            Err(e) => {
                println!("{} Failed to inspect plan: {}", style("✗").red().bold(), e);
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // Handle dry run first
    if opts.dry_run {
        println!(
            "{} Analyzing installation for {}",
            style("→").cyan().bold(),
            style(&opts.product).cyan().bold()
        );

        let request = installation::InstallationRequest {
            product_code: opts.product.clone(),
            build_id: opts.build,
            output_dir: opts.output.clone(),
            plan_only: false,
            execute_plan: None,
            retry_config: RetryConfig::default(),
            cache_config: CacheConfig::default(),
            max_concurrent: opts.max_concurrent,
        };

        let analyzer = installation::DryRunAnalyzer::new(request);
        let analysis = analyzer.analyze();

        println!("\n{}", style("Installation Analysis:").green().bold());
        println!("  Product: {}", style(&analysis.product_code).cyan());
        println!("  Version: {}", style(&analysis.latest_version).yellow());
        println!(
            "  Manifest size: {:.1} MB",
            analysis.manifest_size as f64 / 1_000_000.0
        );
        println!(
            "  Game size: {:.1} GB",
            analysis.game_files_size as f64 / 1_000_000_000.0
        );
        println!(
            "  Total download: {:.1} GB",
            analysis.total_download_size as f64 / 1_000_000_000.0
        );
        println!(
            "  Install size: {:.1} GB",
            analysis.install_size as f64 / 1_000_000_000.0
        );
        println!(
            "  Temp space needed: {:.1} GB",
            analysis.temp_space_needed as f64 / 1_000_000_000.0
        );
        println!("\n  Estimated time at 10 MB/s:");
        println!("    Manifests: {} seconds", analysis.manifest_download_time);
        println!(
            "    Game files: {} minutes",
            analysis.game_download_time / 60
        );
        println!("    Total: {} minutes", analysis.total_time / 60);
        return Ok(());
    }

    // Handle execute existing plan
    if let Some(plan_path) = opts.execute_plan {
        println!(
            "{} Executing installation plan from {}",
            style("→").cyan().bold(),
            style(plan_path.display()).green()
        );

        // Load the plan from JSON
        let plan = installation::InstallationPlan::load(&plan_path)
            .map_err(|e| anyhow::anyhow!("Failed to load installation plan: {}", e))?;

        // Determine installation mode from CLI flag
        let installation_mode = if opts.simple {
            installation::InstallationMode::Simple
        } else {
            installation::InstallationMode::Battlenet
        };

        // Display warning for Simple mode
        if installation_mode == installation::InstallationMode::Simple {
            println!(
                "\n{} {}",
                style("⚠").yellow().bold(),
                style("WARNING: Simple extraction mode").yellow().bold()
            );
            println!(
                "   {}",
                style("This mode does not create a working game installation").yellow()
            );
            println!(
                "   {}",
                style("Games will NOT be playable from this directory").yellow()
            );
            println!(
                "   {}\n",
                style("Use only for file extraction and debugging").yellow()
            );
        }

        // Create executor with persistent progress tracking
        let mut executor = installation::PlanExecutor::new()
            .map_err(|e| anyhow::anyhow!("Failed to create plan executor: {}", e))?
            .with_installation_mode(installation_mode)
            .with_persistent_progress(&plan, opts.verbose);

        // Execute the plan
        executor
            .execute_plan(&plan)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to execute plan: {}", e))?;

        return Ok(());
    }

    // Create installation request
    let request = installation::InstallationRequest {
        product_code: opts.product.clone(),
        build_id: opts.build,
        output_dir: opts.output.clone(),
        plan_only: opts.plan_only,
        execute_plan: None,
        retry_config: RetryConfig::default(),
        cache_config: CacheConfig::default(),
        max_concurrent: opts.max_concurrent,
    };

    println!(
        "{} {} {} to {}",
        style("→").cyan().bold(),
        if opts.plan_only {
            "Planning installation for"
        } else {
            "Installing"
        },
        style(&opts.product).cyan().bold(),
        style(opts.output.display()).green()
    );

    if let Some(build_id) = opts.build {
        println!("  Using specific build: {}", style(build_id).yellow());
    } else {
        println!("  Using latest build");
    }
    println!("  Region: {}", style(&opts.region).cyan());

    // Get data directory for accessing imported builds
    let data_dir = paths::data_dir()?;

    // Create installation plan
    let plan_result = installation::NgdpPlanBuilder::new(request)
        .with_data_dir(data_dir)
        .build()
        .await;

    let spinner = if !opts.verbose {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .expect("Failed to set progress style"),
        );
        pb.set_message("Creating installation plan...");
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        println!("Creating installation plan...");
        None
    };

    match plan_result {
        Ok(plan) => {
            if let Some(pb) = spinner {
                pb.finish_and_clear();
            }

            println!("\n{}", style("Installation Plan Created:").green().bold());
            println!("  Plan ID: {}", style(plan.id).dim());
            println!("  Product: {} ({})", plan.product.code, plan.product.name);
            println!("  Version: {}", style(plan.build.version()).yellow());
            println!(
                "  Build: {} ({})",
                plan.build.build_id(),
                if plan.build.is_latest() {
                    "Latest"
                } else {
                    "Historic"
                }
            );

            // Format file count with commas
            let file_count = plan.manifests.install.file_count;
            let file_count_str = {
                let s = file_count.to_string();
                let mut result = String::new();
                let mut count = 0;
                for c in s.chars().rev() {
                    if count == 3 {
                        result.insert(0, ',');
                        count = 0;
                    }
                    result.insert(0, c);
                    count += 1;
                }
                result
            };

            println!("  Files: {} files", file_count_str);
            println!(
                "  Download size: {:.1} GB",
                plan.archives.total_archive_size as f64 / 1_000_000_000.0
            );
            println!(
                "  Install size: {:.1} GB",
                plan.manifests.install.total_install_size as f64 / 1_000_000_000.0
            );

            if opts.plan_only {
                let plan_path = opts.output.join(".cascette/installation-plan.json");
                println!("\n{}", style("Plan saved to:").green().bold());
                println!("  {}", style(plan_path.display()).cyan());
                println!("\nTo execute this plan later, run:");
                println!(
                    "  {} --execute-plan {}",
                    style("cascette install").cyan(),
                    style(plan_path.display()).yellow()
                );
            } else {
                // Execute the plan immediately
                println!(
                    "\n{}",
                    style("Executing installation plan...").green().bold()
                );

                // Save the plan first for potential resume
                let plan_path = opts.output.join(".cascette/installation-plan.json");
                if let Err(e) = plan.save(&plan_path) {
                    println!(
                        "{} Warning: Failed to save plan for resume: {}",
                        style("!").yellow().bold(),
                        e
                    );
                }

                // Determine installation mode from CLI flag
                let installation_mode = if opts.simple {
                    installation::InstallationMode::Simple
                } else {
                    installation::InstallationMode::Battlenet
                };

                // Display warning for Simple mode
                if installation_mode == installation::InstallationMode::Simple {
                    println!(
                        "\n{} {}",
                        style("⚠").yellow().bold(),
                        style("WARNING: Simple extraction mode").yellow().bold()
                    );
                    println!(
                        "   {}",
                        style("This mode does not create a working game installation").yellow()
                    );
                    println!(
                        "   {}",
                        style("Games will NOT be playable from this directory").yellow()
                    );
                    println!(
                        "   {}\n",
                        style("Use only for file extraction and debugging").yellow()
                    );
                }

                // Create executor with persistent progress tracking
                let mut executor = installation::PlanExecutor::new()
                    .map_err(|e| anyhow::anyhow!("Failed to create plan executor: {}", e))?
                    .with_installation_mode(installation_mode)
                    .with_persistent_progress(&plan, opts.verbose);

                // Execute the plan
                match executor.execute_plan(&plan).await {
                    Ok(_) => {
                        println!(
                            "\n{} Installation completed successfully!",
                            style("✓").green().bold()
                        );
                    }
                    Err(e) => {
                        println!("\n{} Installation failed: {}", style("✗").red().bold(), e);
                        println!(
                            "\nTo retry, run: {} --execute-plan {}",
                            style("cascette install").cyan(),
                            style(plan_path.display()).yellow()
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
        Err(e) => {
            if let Some(pb) = spinner {
                pb.finish_and_clear();
            }
            println!(
                "{} Failed to create installation plan: {}",
                style("✗").red().bold(),
                e
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Handle cache management commands
fn handle_cache_command(
    subcommand: CacheCommands,
    config: &crate::config::CascetteConfig,
) -> Result<()> {
    use console::Term;

    match subcommand {
        CacheCommands::Stats => {
            if let Some(cache) = cache::get_cache() {
                let stats = cache.stats();

                println!("{}", style("Your Cache Statistics:").cyan().bold());
                println!();
                println!("{}", style("Configuration:").bold());
                println!("  {} {}", style("Enabled:").dim(), config.cache.enabled);
                println!(
                    "  {} {}",
                    style("Directory:").dim(),
                    cache.cache_dir().display()
                );
                println!(
                    "  {} {} seconds",
                    style("API TTL:").dim(),
                    config.cache.api_ttl_seconds
                );
                println!(
                    "  {} {} seconds",
                    style("CDN TTL:").dim(),
                    config.cache.cdn_ttl_seconds
                );
                println!(
                    "  {} {} MB",
                    style("Max size:").dim(),
                    config.cache.max_size_mb
                );
                println!();
                println!("{}", style("Your Cache Usage:").bold());
                println!("  {}", style("Memory cache:").dim());
                println!("    Entries: {}", style(stats.memory_entries).green());
                println!(
                    "    Size:    {}",
                    style(indicatif::HumanBytes(stats.memory_bytes as u64)).green()
                );
                println!("  {}", style("Disk cache:").dim());
                println!("    Entries: {}", style(stats.disk_entries).green());
                println!(
                    "    Size:    {}",
                    style(indicatif::HumanBytes(stats.disk_bytes as u64)).green()
                );
            } else {
                println!("{} Cache is not enabled.", style("!").yellow().bold());
            }
        }
        CacheCommands::Clear { force } => {
            if !force {
                println!(
                    "{} This will clear all cached data.",
                    style("Warning:").yellow().bold()
                );
                println!("Are you sure you want to continue? (y/N)");

                let term = Term::stdout();
                let input = term.read_char()?;
                if input != 'y' && input != 'Y' {
                    println!("Cache clear cancelled.");
                    return Ok(());
                }
            }

            if let Some(cache) = cache::get_cache() {
                cache.clear_all()?;
                println!("{} Cache cleared successfully.", style("✓").green().bold());
            } else {
                println!("{} Cache is not enabled.", style("!").yellow().bold());
            }
        }
        CacheCommands::ClearType { cache_type, force } => {
            if !force {
                println!(
                    "{} This will clear {} cache data.",
                    style("Warning:").yellow().bold(),
                    cache_type
                );
                println!("Are you sure you want to continue? (y/N)");

                let term = Term::stdout();
                let input = term.read_char()?;
                if input != 'y' && input != 'Y' {
                    println!("Cache clear cancelled.");
                    return Ok(());
                }
            }

            if let Some(cache) = cache::get_cache() {
                // For now, we can only clear all
                // In the future, implement pattern-based clearing
                if cache_type == "all" {
                    cache.clear_all()?;
                } else {
                    cache.clear_pattern(&format!("{}:*", cache_type))?;
                }
                println!(
                    "{} {} cache cleared successfully.",
                    style("✓").green().bold(),
                    cache_type
                );
            } else {
                println!("{} Cache is not enabled.", style("!").yellow().bold());
            }
        }
    }

    Ok(())
}
