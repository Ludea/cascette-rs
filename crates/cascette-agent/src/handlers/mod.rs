//! Api

pub mod error_codes;

pub mod agent;
pub mod agent_download;
pub mod backfill;
pub mod content;
pub mod download;
pub mod extract;
pub mod game;
pub mod gamesession;
pub mod hardware;
pub mod health;
pub mod install;
pub mod option;
pub mod override_config;
pub mod priorities;
pub mod progress;
pub mod register;
pub mod repair;
pub mod size_estimate;
pub mod spawned;
pub mod uninstall;
pub mod update;
pub mod version;

pub use crate::handlers::extract::ExtractRequest;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU16;

use crate::AgentConfig;
use crate::Metrics;
use crate::session::SessionTracker;
use crate::state::Database;
use crate::state::OperationQueue;
use crate::state::ProductRegistry;
use crate::state::SizeEstimateCache;

/// Global download speed/pause state shared across handlers and executors.
#[derive(Default)]
pub struct DownloadState {
    /// Maximum download speed in bytes per second (0 = unlimited).
    pub max_speed_bps: u64,
    /// Whether downloads are paused.
    pub paused: bool,
    /// Current aggregate download speed in bytes per second.
    pub current_speed_bps: u64,
}

/// Per-product download configuration (keyed by product UID).
#[derive(Debug, Clone)]
pub struct ProductDownloadConfig {
    /// Enable background downloads for this product.
    pub background_download: bool,
    /// Download priority (default 700).
    pub priority: u32,
    /// Speed limit in bytes per second (0 = unlimited).
    pub download_limit: u64,
    /// Whether downloads for this product are paused.
    pub paused: bool,
}

impl Default for ProductDownloadConfig {
    fn default() -> Self {
        Self {
            background_download: false,
            priority: 700,
            download_limit: 0,
            paused: false,
        }
    }
}

/// Shared application state accessible by all handlers.
pub struct AppState {
    /// Product registry.
    pub registry: ProductRegistry,
    /// Operation queue.
    pub queue: OperationQueue,
    /// Prometheus metrics.
    pub metrics: Metrics,
    /// Agent version string.
    pub agent_version: String,
    /// Server start time.
    pub started_at: std::time::SystemTime,
    /// Ribbit/TACT protocol client for version queries.
    pub ribbit_client: Arc<cascette_protocol::RibbitTactClient>,
    /// CDN content download client (implements CdnSource).
    pub cdn_client: Arc<cascette_protocol::CdnClient>,
    /// Agent configuration from CLI/env.
    pub config: Arc<AgentConfig>,
    /// In-memory game session tracker.
    pub session_tracker: SessionTracker,
    /// In-memory size estimation cache.
    pub size_cache: SizeEstimateCache,
    /// Global download speed/pause configuration.
    pub download_state: Arc<tokio::sync::RwLock<DownloadState>>,
    /// Per-product download configuration (keyed by product UID).
    pub product_download_config: Arc<tokio::sync::RwLock<HashMap<String, ProductDownloadConfig>>>,
    /// Actual port the HTTP server is bound to.
    /// Set after successful TCP bind; read by `/agent` handler.
    pub bound_port: AtomicU16,
    /// wago.tools build database for historical version lookup.
    pub wago: Arc<tokio::sync::RwLock<cascette_import::WagoProvider>>,
    /// Wakeup signal for the operation runner.
    ///
    /// Handlers call `notify_one()` after inserting into the queue so the
    /// runner picks up work immediately instead of waiting for its poll tick.
    pub queue_notify: tokio::sync::Notify,
    /// CDN / version-service / config override state.
    /// GET /agent/override reads; POST /agent/override writes.
    pub override_config: Arc<tokio::sync::RwLock<crate::handlers::override_config::OverrideConfig>>,
}

impl AppState {
    /// Create new application state from a database and protocol clients.
    ///
    /// Loads persisted per-product download configurations from the database
    /// into the in-memory cache so they survive agent restarts.
    ///
    /// # Errors
    ///
    /// Returns an error if metrics initialization fails.
    pub async fn new(
        db: Database,
        ribbit_client: Arc<cascette_protocol::RibbitTactClient>,
        cdn_client: Arc<cascette_protocol::CdnClient>,
        config: Arc<AgentConfig>,
        wago: cascette_import::WagoProvider,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let metrics = Metrics::new()?;
        let default_port = config.port();
        let registry = ProductRegistry::new(db.clone());

        // Hydrate per-product download configs from database.
        let configs = registry.list_download_configs().await.unwrap_or_default();
        if !configs.is_empty() {
            tracing::info!(
                count = configs.len(),
                "loaded per-product download configs from database"
            );
        }

        Ok(Self {
            registry,
            queue: OperationQueue::new(db),
            metrics,
            agent_version: format!("cascette-agent/{}", env!("CARGO_PKG_VERSION")),
            started_at: std::time::SystemTime::now(),
            ribbit_client,
            cdn_client,
            config,
            session_tracker: SessionTracker::new(),
            size_cache: SizeEstimateCache::new(),
            download_state: Arc::new(tokio::sync::RwLock::new(DownloadState::default())),
            product_download_config: Arc::new(tokio::sync::RwLock::new(configs)),
            bound_port: AtomicU16::new(default_port),
            wago: Arc::new(tokio::sync::RwLock::new(wago)),
            queue_notify: tokio::sync::Notify::new(),
            override_config: Arc::new(tokio::sync::RwLock::new(
                crate::handlers::override_config::OverrideConfig::default(),
            )),
        })
    }
}
