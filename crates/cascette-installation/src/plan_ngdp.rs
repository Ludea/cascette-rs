//! Installation plan builder with real NGDP integration

use crate::{
    error::{InstallationError, Result},
    metadata::MetadataResolver,
    models::{InstallationPlan, InstallationRequest, InstallationTarget, Platform},
};
use chrono::Utc;
use uuid::Uuid;

/// Builder for creating installation plans with real NGDP data
pub struct NgdpPlanBuilder {
    request: InstallationRequest,
    data_dir: Option<std::path::PathBuf>,
}

impl NgdpPlanBuilder {
    /// Create a new plan builder
    #[must_use]
    pub fn new(request: InstallationRequest) -> Self {
        Self {
            request,
            data_dir: None,
        }
    }

    /// Set the data directory for accessing imported builds
    #[must_use]
    pub fn with_data_dir(mut self, data_dir: std::path::PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }

    /// Build the installation plan using real NGDP data
    pub async fn build(self) -> Result<InstallationPlan> {
        // Validate request
        self.request
            .validate()
            .map_err(InstallationError::InvalidConfiguration)?;

        // Create metadata resolver
        let mut resolver =
            MetadataResolver::new(self.request.product_code.clone(), self.request.build_id)?;

        // Configure build manager if data directory is provided
        if let Some(data_dir) = self.data_dir {
            resolver = resolver.with_build_manager(&data_dir);
        }

        // Resolve product information
        let product = resolver.resolve_product().await?;

        // Resolve build
        let build = resolver.resolve_build().await?;

        // Resolve configurations from NGDP
        let configs = resolver.resolve_configs().await?;

        // Resolve manifests (now includes real config parsing)
        let manifests = resolver.resolve_manifests(&configs).await?;

        // Get archives from CDN config (parsed in resolve_manifests)
        let archives = resolver.resolve_archives(&configs).await?;

        // Create target
        let target = InstallationTarget {
            directory: self.request.output_dir.clone(),
            platform: get_current_platform(),
            tags: crate::metadata::MetadataResolver::detect_platform_tags(),
        };

        // Create the plan (summary is no longer stored, calculated on demand)
        let plan = InstallationPlan {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            product,
            build,
            configs,
            manifests,
            archives,
            target,
        };

        // Save if plan-only mode
        if self.request.plan_only {
            let plan_dir = self.request.output_dir.join(".cascette");
            std::fs::create_dir_all(&plan_dir).map_err(InstallationError::IoError)?;
            let plan_path = plan_dir.join("installation-plan.json");
            plan.save(&plan_path)
                .map_err(|e| InstallationError::Other(e.to_string()))?;
        }

        Ok(plan)
    }

    /// Validate build ID
    #[allow(dead_code)] // Future validation
    pub fn validate_build_id(&self) -> Result<()> {
        if let Some(build_id) = self.request.build_id {
            if build_id == 99_999_999 {
                return Err(InstallationError::BuildNotFound(build_id));
            }
        }
        Ok(())
    }
}

/// Get current platform
fn get_current_platform() -> Platform {
    #[cfg(target_os = "windows")]
    return Platform::Windows;

    #[cfg(target_os = "macos")]
    return Platform::MacOS;

    #[cfg(target_os = "linux")]
    return Platform::Linux;

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return Platform::Linux;
}
