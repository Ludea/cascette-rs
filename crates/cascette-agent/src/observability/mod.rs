//! Observability infrastructure for cascette-agent
//!
//! Provides comprehensive observability through:
//! - Structured logging (FR-033)
//! - Distributed tracing (FR-034)
//! - Prometheus metrics (FR-035)
//!
//! Future use: T078 (main.rs observability initialization), T041 (metrics endpoint)
#![allow(dead_code)]
//!
//! ## Quick Start
//!
//! ```no_run
//! use cascette_agent::observability::{self, ObservabilityConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Initialize observability
//!     let config = ObservabilityConfig {
//!         log_level: "info".to_string(),
//!         tracing_endpoint: None, // Optional OTLP endpoint
//!     };
//!
//!     let observability = observability::init(config).await.expect("Task should complete");
//!
//!     // Use metrics
//!     observability.metrics.record_operation_start("install");
//!
//!     // Application runs...
//!
//!     // Graceful shutdown
//!     observability.shutdown().await;
//! }
//! ```
//!
//! ## Components
//!
//! - [`logging`]: Structured JSON logging with tracing-subscriber
//! - [`tracing`]: OpenTelemetry distributed tracing
//! - [`metrics`]: Prometheus metrics collection
//! - [`middleware`]: HTTP middleware for request tracing and metrics

pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod tracing;

// Re-export commonly used types
pub use metrics::Metrics;

use anyhow::{Context, Result};
use std::sync::Arc;

/// Observability configuration
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,

    /// Optional OpenTelemetry OTLP endpoint (e.g., "<http://localhost:4317>")
    pub tracing_endpoint: Option<String>,
}

/// Observability handles for the agent service
///
/// Contains references to all observability components and provides
/// graceful shutdown capability.
pub struct Observability {
    /// Metrics registry
    pub metrics: Arc<metrics::Metrics>,

    /// Optional tracer for distributed tracing
    tracer: Option<opentelemetry_sdk::trace::Tracer>,
}

impl Observability {
    /// Graceful shutdown of observability infrastructure
    ///
    /// Flushes all pending traces and metrics before shutdown.
    pub async fn shutdown(self) {
        if let Some(_tracer) = self.tracer {
            ::tracing::info!("Shutting down OpenTelemetry tracer");
            // Tracer will be dropped and flush on drop
            // Future versions may want explicit flush control
        }

        ::tracing::info!("Observability shutdown complete");
    }
}

/// Initialize observability infrastructure
///
/// Sets up logging, tracing (if endpoint provided), and metrics.
///
/// # Configuration
///
/// - If `tracing_endpoint` is Some, initializes full distributed tracing
/// - If `tracing_endpoint` is None, only sets up structured logging
///
/// # Examples
///
/// ```no_run
/// use cascette_agent::observability::{self, ObservabilityConfig};
///
/// # async fn example() -> anyhow::Result<()> {
/// // With distributed tracing
/// let config = ObservabilityConfig {
///     log_level: "info".to_string(),
///     tracing_endpoint: Some("http://localhost:4317".to_string()),
/// };
/// let obs = observability::init(config).await?;
///
/// // Without distributed tracing
/// let config = ObservabilityConfig {
///     log_level: "debug".to_string(),
///     tracing_endpoint: None,
/// };
/// let obs = observability::init(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn init(config: ObservabilityConfig) -> Result<Observability> {
    let tracer = if let Some(endpoint) = &config.tracing_endpoint {
        // Initialize with distributed tracing
        let tracer =
            tracing::init_tracer(endpoint).context("Failed to initialize distributed tracing")?;

        logging::init_with_tracing(&config.log_level, tracer.clone())
            .context("Failed to initialize logging with tracing")?;

        Some(tracer)
    } else {
        // Initialize without distributed tracing
        logging::init(&config.log_level).context("Failed to initialize logging")?;
        None
    };

    // Initialize metrics
    let metrics = Arc::new(metrics::Metrics::new());

    ::tracing::info!(
        log_level = %config.log_level,
        tracing_enabled = config.tracing_endpoint.is_some(),
        "Observability initialized"
    );

    Ok(Observability { metrics, tracer })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_without_tracing() {
        let config = ObservabilityConfig {
            log_level: "info".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");
        assert_eq!(obs.metrics.active_operations_count(), 0);

        obs.shutdown().await;
    }

    #[tokio::test]
    async fn test_metrics_integration() {
        let config = ObservabilityConfig {
            log_level: "debug".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");

        // Test metrics
        obs.metrics.record_operation_start("install");
        assert_eq!(obs.metrics.active_operations_count(), 1);

        obs.metrics.record_operation_complete("install", 10.5);
        assert_eq!(obs.metrics.active_operations_count(), 0);

        obs.shutdown().await;
    }

    #[tokio::test]
    async fn test_logging_levels() {
        for level in &["trace", "debug", "info", "warn", "error"] {
            let config = ObservabilityConfig {
                log_level: level.to_string(),
                tracing_endpoint: None,
            };

            // Should not panic
            let obs = init(config).await.expect("Task should complete");
            obs.shutdown().await;
        }
    }

    #[tokio::test]
    async fn test_operation_lifecycle_tracing() {
        let config = ObservabilityConfig {
            log_level: "debug".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");

        // Simulate operation lifecycle
        obs.metrics.record_operation_start("update");
        assert_eq!(obs.metrics.active_operations_count(), 1);

        // Simulate progress
        obs.metrics.record_download_throughput(5_000_000.0);

        // Complete operation
        obs.metrics.record_operation_complete("update", 45.0);
        assert_eq!(obs.metrics.active_operations_count(), 0);

        obs.shutdown().await;
    }

    #[tokio::test]
    async fn test_operation_error_tracking() {
        let config = ObservabilityConfig {
            log_level: "warn".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");

        // Start operation
        obs.metrics.record_operation_start("repair");
        assert_eq!(obs.metrics.active_operations_count(), 1);

        // Error occurs
        obs.metrics
            .record_operation_error("repair", "network_timeout");
        assert_eq!(obs.metrics.active_operations_count(), 0);

        obs.shutdown().await;
    }

    #[tokio::test]
    async fn test_concurrent_operations_metrics() {
        let config = ObservabilityConfig {
            log_level: "info".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");

        // Start multiple operations
        obs.metrics.record_operation_start("install");
        obs.metrics.record_operation_start("update");
        obs.metrics.record_operation_start("verify");
        assert_eq!(obs.metrics.active_operations_count(), 3);

        // Complete them in different ways
        obs.metrics.record_operation_complete("install", 120.0);
        assert_eq!(obs.metrics.active_operations_count(), 2);

        obs.metrics.record_operation_error("update", "disk_full");
        assert_eq!(obs.metrics.active_operations_count(), 1);

        obs.metrics.record_operation_complete("verify", 30.0);
        assert_eq!(obs.metrics.active_operations_count(), 0);

        obs.shutdown().await;
    }

    #[tokio::test]
    async fn test_download_throughput_tracking() {
        let config = ObservabilityConfig {
            log_level: "trace".to_string(),
            tracing_endpoint: None,
        };

        let obs = init(config).await.expect("Task should complete");

        // Record various throughputs
        let throughputs = [
            100_000.0,    // 100 KB/s
            1_000_000.0,  // 1 MB/s
            10_000_000.0, // 10 MB/s
            50_000_000.0, // 50 MB/s
        ];

        for throughput in &throughputs {
            obs.metrics.record_download_throughput(*throughput);
        }

        obs.shutdown().await;
    }

    #[test]
    fn test_span_builders() {
        use crate::observability::tracing::SpanBuilder;
        use uuid::Uuid;

        // Test all span builder methods
        let _http = SpanBuilder::http_request("GET", "/agent/products");
        let _operation = SpanBuilder::operation("install", "wow_classic", Uuid::new_v4());
        let _network = SpanBuilder::network("download", "https://cdn.arctium.tools/data/...");
        let _database = SpanBuilder::database("INSERT", "operations");
        let _file = SpanBuilder::file_operation("verify", "Data/data.000");

        // Should not panic
    }
}
