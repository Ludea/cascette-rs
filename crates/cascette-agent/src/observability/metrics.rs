//! Metrics collection using Prometheus
//!
//! Implements FR-035: Metrics for active operations, throughput, errors, completion times
//!
//! ## Metrics Exposed
//!
//! - `operation_total` (Counter): Total operations started, labeled by type
//! - `operation_errors` (Counter): Total operation errors, labeled by type and `error_type`
//! - `active_operations` (Gauge): Currently active operations
//! - `operation_duration_seconds` (Histogram): Operation completion time distribution
//! - `download_bytes_per_second` (Histogram): Download throughput distribution
//! - `update_delta_efficiency_percent` (Histogram): Update delta efficiency percentage (T097)
//!
//! ## Usage
//!
//! ```no_run
//! use cascette_agent::observability::metrics::Metrics;
//!
//! let metrics = Metrics::new();
//!
//! // Record operation start
//! metrics.record_operation_start("install");
//!
//! // Record download throughput
//! metrics.record_download_throughput(1_000_000.0); // 1 MB/s
//!
//! // Record operation completion
//! metrics.record_operation_complete("install", 120.5);
//!
//! // Record operation error
//! metrics.record_operation_error("install", "network_timeout");
//!
//! // Record update delta efficiency (T097)
//! metrics.record_update_delta_efficiency("update", 85.5);
//! ```

use prometheus_client::{
    encoding::EncodeLabelSet,
    metrics::{counter::Counter, family::Family, gauge::Gauge, histogram::Histogram},
    registry::Registry,
};
use std::sync::Arc;

/// Operation type labels for metrics
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct OperationLabels {
    /// Operation type (install, update, repair, verify, uninstall)
    pub operation_type: String,
}

/// Error type labels for error metrics
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ErrorLabels {
    /// Operation type
    pub operation_type: String,
    /// Error category
    pub error_type: String,
}

/// Metrics registry for cascette-agent
///
/// Contains all Prometheus metrics exposed by the agent.
#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,

    // Counters
    operation_total: Family<OperationLabels, Counter>,
    operation_errors: Family<ErrorLabels, Counter>,

    // Gauges
    active_operations: Gauge,

    // Histograms
    operation_duration_seconds: Family<OperationLabels, Histogram>,
    download_bytes_per_second: Histogram,
    update_delta_efficiency_percent: Family<OperationLabels, Histogram>,
}

// Future use: T041 (metrics endpoint), T078 (main.rs metrics collection)
#[allow(dead_code)]
impl Metrics {
    /// Create a new metrics registry
    ///
    /// Initializes all metrics and registers them with the Prometheus registry.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Registry::default();

        // Initialize counters
        let operation_total = Family::<OperationLabels, Counter>::default();
        registry.register(
            "operation_total",
            "Total number of operations started",
            operation_total.clone(),
        );

        let operation_errors = Family::<ErrorLabels, Counter>::default();
        registry.register(
            "operation_errors",
            "Total number of operation errors",
            operation_errors.clone(),
        );

        // Initialize gauges
        let active_operations = Gauge::default();
        registry.register(
            "active_operations",
            "Number of currently active operations",
            active_operations.clone(),
        );

        // Initialize histograms
        let operation_duration_seconds =
            Family::<OperationLabels, Histogram>::new_with_constructor(|| {
                // Buckets: 1s, 5s, 10s, 30s, 1m, 5m, 10m, 30m, 1h, 2h, 4h
                Histogram::new(
                    [
                        1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0, 7200.0, 14400.0,
                    ]
                    .into_iter(),
                )
            });
        registry.register(
            "operation_duration_seconds",
            "Operation completion time in seconds",
            operation_duration_seconds.clone(),
        );

        let download_bytes_per_second = {
            // Buckets: 100KB/s, 500KB/s, 1MB/s, 5MB/s, 10MB/s, 50MB/s, 100MB/s, 500MB/s, 1GB/s
            Histogram::new(
                [
                    100_000.0,
                    500_000.0,
                    1_000_000.0,
                    5_000_000.0,
                    10_000_000.0,
                    50_000_000.0,
                    100_000_000.0,
                    500_000_000.0,
                    1_000_000_000.0,
                ]
                .into_iter(),
            )
        };
        registry.register(
            "download_bytes_per_second",
            "Download throughput in bytes per second",
            download_bytes_per_second.clone(),
        );

        let update_delta_efficiency_percent =
            Family::<OperationLabels, Histogram>::new_with_constructor(|| {
                // Buckets: 0%, 10%, 20%, ..., 90%, 95%, 99%, 100%
                // Measures how much bandwidth was saved using delta updates
                Histogram::new(
                    [
                        0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 99.0,
                        100.0,
                    ]
                    .into_iter(),
                )
            });
        registry.register(
            "update_delta_efficiency_percent",
            "Update delta efficiency as percentage of bandwidth saved (T097)",
            update_delta_efficiency_percent.clone(),
        );

        Self {
            registry: Arc::new(registry),
            operation_total,
            operation_errors,
            active_operations,
            operation_duration_seconds,
            download_bytes_per_second,
            update_delta_efficiency_percent,
        }
    }

    /// Get the Prometheus registry
    ///
    /// Used for encoding metrics in the /metrics endpoint.
    #[must_use]
    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    /// Record an operation start
    ///
    /// Increments:
    /// - `operation_total` counter
    /// - `active_operations` gauge
    pub fn record_operation_start(&self, operation_type: &str) {
        let labels = OperationLabels {
            operation_type: operation_type.to_string(),
        };

        self.operation_total.get_or_create(&labels).inc();
        self.active_operations.inc();

        tracing::debug!(
            operation_type = %operation_type,
            "Recorded operation start"
        );
    }

    /// Record an operation completion
    ///
    /// Records:
    /// - Duration in `operation_duration_seconds` histogram
    ///
    /// Decrements:
    /// - `active_operations` gauge
    pub fn record_operation_complete(&self, operation_type: &str, duration_seconds: f64) {
        let labels = OperationLabels {
            operation_type: operation_type.to_string(),
        };

        self.operation_duration_seconds
            .get_or_create(&labels)
            .observe(duration_seconds);
        self.active_operations.dec();

        tracing::debug!(
            operation_type = %operation_type,
            duration_seconds = %duration_seconds,
            "Recorded operation completion"
        );
    }

    /// Record an operation error
    ///
    /// Increments:
    /// - `operation_errors` counter
    ///
    /// Decrements:
    /// - `active_operations` gauge
    pub fn record_operation_error(&self, operation_type: &str, error_type: &str) {
        let labels = ErrorLabels {
            operation_type: operation_type.to_string(),
            error_type: error_type.to_string(),
        };

        self.operation_errors.get_or_create(&labels).inc();
        self.active_operations.dec();

        tracing::warn!(
            operation_type = %operation_type,
            error_type = %error_type,
            "Recorded operation error"
        );
    }

    /// Record download throughput
    ///
    /// Records bytes per second in `download_bytes_per_second` histogram.
    pub fn record_download_throughput(&self, bytes_per_second: f64) {
        self.download_bytes_per_second.observe(bytes_per_second);

        tracing::trace!(
            bytes_per_second = %bytes_per_second,
            "Recorded download throughput"
        );
    }

    /// Record update delta efficiency (T097)
    ///
    /// Records the efficiency percentage of delta updates, measuring how much
    /// bandwidth was saved compared to downloading the full installation.
    ///
    /// # Arguments
    ///
    /// * `operation_type` - Type of update operation
    /// * `efficiency_percent` - Percentage of bandwidth saved (0-100)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cascette_agent::observability::metrics::Metrics;
    /// let metrics = Metrics::new();
    ///
    /// // Record 85.5% efficiency (only downloaded 14.5% of full size)
    /// metrics.record_update_delta_efficiency("update", 85.5);
    /// ```
    pub fn record_update_delta_efficiency(&self, operation_type: &str, efficiency_percent: f64) {
        let labels = OperationLabels {
            operation_type: operation_type.to_string(),
        };

        self.update_delta_efficiency_percent
            .get_or_create(&labels)
            .observe(efficiency_percent);

        tracing::info!(
            operation_type = %operation_type,
            efficiency_percent = %efficiency_percent,
            "Recorded update delta efficiency"
        );
    }

    /// Get current active operations count
    #[must_use]
    pub fn active_operations_count(&self) -> i64 {
        self.active_operations.get()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        assert_eq!(metrics.active_operations_count(), 0);
    }

    #[test]
    fn test_operation_lifecycle() {
        let metrics = Metrics::new();

        // Start operation
        metrics.record_operation_start("install");
        assert_eq!(metrics.active_operations_count(), 1);

        // Complete operation
        metrics.record_operation_complete("install", 120.5);
        assert_eq!(metrics.active_operations_count(), 0);
    }

    #[test]
    fn test_operation_error() {
        let metrics = Metrics::new();

        // Start operation
        metrics.record_operation_start("update");
        assert_eq!(metrics.active_operations_count(), 1);

        // Error occurs
        metrics.record_operation_error("update", "network_timeout");
        assert_eq!(metrics.active_operations_count(), 0);
    }

    #[test]
    fn test_multiple_concurrent_operations() {
        let metrics = Metrics::new();

        // Start multiple operations
        metrics.record_operation_start("install");
        metrics.record_operation_start("update");
        metrics.record_operation_start("repair");
        assert_eq!(metrics.active_operations_count(), 3);

        // Complete one
        metrics.record_operation_complete("install", 100.0);
        assert_eq!(metrics.active_operations_count(), 2);

        // Error on another
        metrics.record_operation_error("update", "disk_full");
        assert_eq!(metrics.active_operations_count(), 1);

        // Complete last
        metrics.record_operation_complete("repair", 50.0);
        assert_eq!(metrics.active_operations_count(), 0);
    }

    #[test]
    fn test_download_throughput() {
        let metrics = Metrics::new();

        // Record various throughputs
        metrics.record_download_throughput(1_000_000.0); // 1 MB/s
        metrics.record_download_throughput(5_000_000.0); // 5 MB/s
        metrics.record_download_throughput(10_000_000.0); // 10 MB/s

        // No panics, metrics recorded
    }

    #[test]
    fn test_update_delta_efficiency() {
        let metrics = Metrics::new();

        // Record various efficiency percentages
        metrics.record_update_delta_efficiency("update", 85.5);
        metrics.record_update_delta_efficiency("update", 92.3);
        metrics.record_update_delta_efficiency("update", 78.0);

        // No panics, metrics recorded
    }

    #[test]
    fn test_registry_access() {
        let metrics = Metrics::new();
        let registry = metrics.registry();
        assert!(Arc::strong_count(&registry) >= 1);
    }
}
