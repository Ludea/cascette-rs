//! # cascette-agent
//!
//! Background HTTP service for managing product installations, updates, and verification.
//!
//! The agent provides a Battle.net Agent-compatible REST API for:
//! - Installing products with progress tracking
//! - Updating installed products (delta downloads)
//! - Repairing corrupted installations
//! - Verifying installation integrity
//! - Uninstalling products
//!
//! ## Architecture
//!
//! - HTTP server (axum) on port 1120 (fallback 6881-6883)
//! - SQLite state persistence for products, operations, and history
//! - Operation queue with concurrent execution
//! - Resume capability for interrupted operations
//! - Comprehensive observability (logging, tracing, metrics)
//!
//! ## Configuration
//!
//! Configuration is loaded from:
//! - Linux: `~/.config/cascette/agent.toml`
//! - macOS: `~/Library/Application Support/Cascette/agent.toml`
//! - Windows: `%APPDATA%\\Cascette\\agent.toml`

mod config;
mod error;
mod executor;
mod models;
mod observability;
mod server;
mod state;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::signal;
use tokio_util::sync::CancellationToken;

use config::AgentConfig;
use executor::runner::ExecutionRunner;
use observability::ObservabilityConfig;
use state::{AppState, Database, OperationQueue, ProductRegistry};

#[tokio::main]
async fn main() -> Result<()> {
    // Print version on startup
    println!("cascette-agent v{}", env!("CARGO_PKG_VERSION"));
    println!("Starting Battle.net Agent-compatible service...\n");

    // Load configuration (T080)
    let config = AgentConfig::load(None::<&str>).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load configuration: {}", e);
        eprintln!("Using default configuration");
        AgentConfig::default()
    });

    // Initialize observability (T078)
    let obs_config = ObservabilityConfig {
        log_level: config.logging.level.clone(),
        tracing_endpoint: None, // TODO: Add to config
    };

    let observability = observability::init(obs_config)
        .await
        .context("Failed to initialize observability")?;

    tracing::info!(
        bind_address = %config.network.bind_address,
        primary_port = config.network.port,
        fallback_ports = ?config.network.fallback_ports,
        database_path = ?config.database.path,
        log_level = %config.logging.level,
        max_concurrent_operations = config.operations.max_concurrent,
        retention_days = config.operations.retention_days,
        "Configuration loaded"
    );

    // Initialize database (T078)
    let db_path = shellexpand::tilde(&config.database.path.to_string_lossy()).to_string();
    tracing::info!(path = %db_path, "Initializing SQLite database");

    let database = Database::open(&db_path).context("Failed to open database")?;
    let db = Arc::new(Mutex::new(database));

    // Create application state (T078)
    let queue = Arc::new(OperationQueue::new(db.clone()));
    let registry = Arc::new(ProductRegistry::new(db.clone()));
    let metrics = observability.metrics.clone();

    let app_state = Arc::new(AppState::new(queue, registry, metrics));

    // Resume interrupted operations (T076, T077)
    match app_state.queue.find_interrupted_operations() {
        Ok(interrupted_ops) => {
            if !interrupted_ops.is_empty() {
                tracing::info!(
                    count = interrupted_ops.len(),
                    "Found interrupted operations from previous session"
                );

                for op in &interrupted_ops {
                    tracing::info!(
                        operation_id = %op.operation_id,
                        product_code = %op.product_code,
                        operation_type = ?op.operation_type,
                        state = ?op.state,
                        "Interrupted operation will resume when executor is implemented"
                    );
                }

                // TODO: Once executor is implemented (future task), spawn tasks to resume these operations
                // For now, operations remain in the queue and can be monitored via API
            } else {
                tracing::info!("No interrupted operations found");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to check for interrupted operations");
        }
    }

    // Create shutdown token for coordinated shutdown
    let shutdown_token = CancellationToken::new();

    // Start execution runner for background operation processing
    tracing::info!("Starting execution runner");
    let execution_runner = ExecutionRunner::new(
        config.clone(),
        app_state.queue.clone(),
        app_state.queue.clone(), // OperationQueue implements ProgressReporter
        app_state.registry.clone(),
        (*app_state.metrics).clone(),
        shutdown_token.clone(),
    );

    let runner_handle = tokio::spawn(async move {
        if let Err(e) = execution_runner.run().await {
            tracing::error!(error = %e, "Execution runner failed");
        }
    });

    // Start periodic version checking task (T086)
    tracing::info!("Starting periodic version checking task (every 5 minutes)");
    let version_check_registry = app_state.registry.clone();
    let version_check_shutdown = shutdown_token.clone();

    let version_check_handle = tokio::spawn(async move {
        use tokio::time::{Duration, interval};

        let mut check_interval = interval(Duration::from_secs(300)); // 5 minutes
        let ribbit_url = "us.version.battle.net:1119";

        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    tracing::debug!("Running periodic version check");

                    // Get all products from the registry
                    match version_check_registry.list() {
                        Ok(products) => {
                            let installed_products: Vec<_> = products
                                .into_iter()
                                .filter(|p| p.status == crate::models::ProductStatus::Installed)
                                .collect();

                            if installed_products.is_empty() {
                                tracing::debug!("No installed products to check for updates");
                                continue;
                            }

                            tracing::info!(
                                count = installed_products.len(),
                                "Checking {} installed products for updates",
                                installed_products.len()
                            );

                            for product in installed_products {
                                match version_check_registry
                                    .check_for_updates(&product.product_code, ribbit_url)
                                    .await
                                {
                                    Ok(update_available) => {
                                        if update_available {
                                            tracing::info!(
                                                product_code = %product.product_code,
                                                "Update available for product"
                                            );
                                        } else {
                                            tracing::debug!(
                                                product_code = %product.product_code,
                                                "Product is up to date"
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            product_code = %product.product_code,
                                            error = %e,
                                            "Failed to check for updates"
                                        );
                                    }
                                }
                            }

                            tracing::debug!("Periodic version check completed");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to list products for version check");
                        }
                    }
                }
                _ = version_check_shutdown.cancelled() => {
                    tracing::info!("Version check task shutting down");
                    break;
                }
            }
        }
    });

    // Create router (T078)
    let app = server::create_router(app_state.clone());

    // Try binding to ports with fallback sequence (T081)
    let ports_to_try =
        std::iter::once(config.network.port).chain(config.network.fallback_ports.iter().copied());

    let mut listener = None;
    let mut bound_port = None;

    for port in ports_to_try {
        let addr = format!("{}:{}", config.network.bind_address, port);
        match TcpListener::bind(&addr).await {
            Ok(l) => {
                tracing::info!(address = %addr, "Successfully bound to port");
                listener = Some(l);
                bound_port = Some(port);
                break;
            }
            Err(e) => {
                tracing::warn!(port = port, error = %e, "Failed to bind to port, trying next");
            }
        }
    }

    let listener = listener.context("Failed to bind to any port (tried 1120, 6881, 6882, 6883)")?;
    let bound_port = bound_port.expect("Port should be set if listener exists");

    // Log successful startup (T082)
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        bind_address = %config.network.bind_address,
        port = bound_port,
        database = %db_path,
        "cascette-agent started successfully"
    );

    println!(
        "✓ Service running on http://{}:{}",
        config.network.bind_address, bound_port
    );
    println!("  GET  /health - Health check");
    println!("  GET  /metrics - Prometheus metrics");
    println!("  POST /products/{{code}}/install - Install product");
    println!("  GET  /operations/{{id}} - Get operation status");
    println!("\nPress Ctrl+C to shutdown gracefully\n");

    // Set up graceful shutdown (T079)
    let shutdown_token_for_signal = shutdown_token.clone();
    let shutdown_signal = async move {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        tracing::info!("Shutdown signal received, starting graceful shutdown");
        shutdown_token_for_signal.cancel();
    };

    // Start HTTP server with graceful shutdown (T078, T079)
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .context("HTTP server error")?;

    // Wait for background tasks to stop
    tracing::info!("HTTP server stopped, waiting for background tasks to finish");
    let _ = tokio::join!(runner_handle, version_check_handle);
    tracing::info!("All background tasks stopped");

    // Shutdown observability (T079)
    tracing::info!("Shutting down observability");
    observability.shutdown().await;

    println!("\ncascette-agent shutdown complete");
    Ok(())
}
