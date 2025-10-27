//! Installation module - now uses cascette-installation library
//!
//! This module re-exports functionality from the cascette-installation library
//! for backward compatibility with existing CLI commands.

// Re-export modules used by CLI commands
pub use cascette_installation::builds;

// Re-export commonly used types for convenience
pub use cascette_installation::executor::{InstallationMode, PlanExecutor};
// Note: CacheConfig and RetryConfig are used by main.rs but linter doesn't detect re-export usage
#[allow(unused_imports)]
pub use cascette_installation::models::{
    CacheConfig, InstallationPlan, InstallationRequest, RetryConfig,
};
pub use cascette_installation::plan::{DryRunAnalyzer, PlanInspector};
pub use cascette_installation::plan_ngdp::NgdpPlanBuilder;
