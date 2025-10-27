//! Installation plan inspection and analysis

use crate::{
    error::{InstallationError, Result},
    models::{InstallationPlan, InstallationRequest},
};

// NOTE: PlanBuilder has been replaced by NgdpPlanBuilder in plan_ngdp.rs
// This deprecated implementation has been removed.

// NOTE: PlanBuilder implementation removed - replaced by NgdpPlanBuilder
// NOTE: get_current_platform moved to models.rs where Platform is defined

/// Inspector for analyzing and displaying installation plan details
///
/// Provides methods to load and inspect saved installation plans without
/// executing them. Useful for verifying plan contents and debugging.
pub struct PlanInspector {
    /// Path to the installation plan file
    path: std::path::PathBuf,
}

impl PlanInspector {
    /// Create a new inspector for the given plan file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the installation plan JSON file
    #[must_use]
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Inspect the plan and extract detailed information
    ///
    /// Loads and parses the plan file, extracting all relevant metadata
    /// for display or analysis.
    ///
    /// # Errors
    ///
    /// Returns an error if the plan file cannot be read or parsed.
    pub fn inspect(&self) -> Result<PlanInspectionResult> {
        if !self.path.exists() {
            return Err(InstallationError::Other(format!(
                "Plan not found at {:?}",
                self.path
            )));
        }

        let json = std::fs::read_to_string(&self.path)?;
        let plan: InstallationPlan = serde_json::from_str(&json)
            .map_err(|e| InstallationError::Other(format!("Failed to parse plan: {e}")))?;

        Ok(PlanInspectionResult {
            id: plan.id.to_string(),
            created_at: plan.created_at.to_string(),
            product_code: plan.product.code,
            product_name: plan.product.name,
            build_id: plan.build.build_id(),
            version: plan.build.version().to_string(),
            build_type: if plan.build.is_latest() {
                "Latest"
            } else {
                "Historic"
            }
            .to_string(),
            build_source: plan.build.source().map(String::from),
            encoding_size: plan.manifests.encoding.size,
            encoding_entries: plan.manifests.encoding.entry_count,
            root_size: plan.manifests.root.size,
            root_files: plan.manifests.root.file_count,
            install_manifest_size: plan.manifests.install.size,
            install_files: plan.manifests.install.file_count,
            archive_count: plan.archives.indices.len(),
            total_archive_size: plan.archives.total_archive_size,
            indices_loaded: plan.archives.indices.len(),
            target_directory: plan.target.directory.to_string_lossy().to_string(),
            platform: format!("{:?}", plan.target.platform),
            tags: plan.target.tags,
            file_count: plan.manifests.install.file_count,
            download_size: plan.archives.total_archive_size,
            total_install_size: plan.manifests.install.total_install_size,
            ready_to_execute: true, // Always true if plan was successfully created
        })
    }

    /// Format inspection output for display
    ///
    /// Creates a human-readable summary of the installation plan suitable
    /// for console output.
    ///
    /// # Errors
    ///
    /// Returns an error if the plan file cannot be read or parsed.
    pub fn format_output(&self) -> Result<String> {
        let result = self.inspect()?;

        Ok(format!(
            "Installation Plan Details:\n\
            \n\
            Product: {} ({})\n\
            Version: {}\n\
            Build: {} ({})\n\
            \n\
            Download size: {:.1} GB\n\
            Install size: {:.1} GB\n\
            Files: {} files\n\
            \n\
            Status: {}",
            result.product_code,
            result.product_name,
            result.version,
            result.build_id,
            result.build_type,
            result.download_size as f64 / 1_000_000_000.0,
            result.total_install_size as f64 / 1_000_000_000.0,
            format_with_commas(result.file_count),
            if result.ready_to_execute {
                "Ready to execute"
            } else {
                "Not ready"
            }
        ))
    }
}

/// Plan inspection result containing all extracted metadata
///
/// Holds detailed information about an installation plan including
/// product details, build information, file counts, and download sizes.
#[derive(Debug)]
#[allow(dead_code)]
pub struct PlanInspectionResult {
    /// Unique plan identifier
    pub id: String,
    /// Plan creation timestamp
    pub created_at: String,
    /// Product code (e.g., "wow", "`wow_classic`")
    pub product_code: String,
    /// Human-readable product name
    pub product_name: String,
    /// Build number
    pub build_id: u32,
    /// Version string
    pub version: String,
    /// Build type ("Latest" or "Historic")
    pub build_type: String,
    /// Build source for historic builds
    pub build_source: Option<String>,
    /// Encoding manifest size in bytes
    pub encoding_size: u64,
    /// Number of entries in encoding manifest
    pub encoding_entries: usize,
    /// Root manifest size in bytes
    pub root_size: u64,
    /// Number of files in root manifest
    pub root_files: usize,
    /// Install manifest size in bytes
    pub install_manifest_size: u64,
    /// Number of files in install manifest
    pub install_files: usize,
    /// Number of archives
    pub archive_count: usize,
    /// Total size of all archives in bytes
    pub total_archive_size: u64,
    /// Number of loaded archive indices
    pub indices_loaded: usize,
    /// Target installation directory
    pub target_directory: String,
    /// Target platform
    pub platform: String,
    /// Installation tags for filtering
    pub tags: Vec<String>,
    /// Total number of files to install
    pub file_count: usize,
    /// Total download size in bytes
    pub download_size: u64,
    /// Total install size in bytes
    pub total_install_size: u64,
    /// Whether plan is ready to execute
    pub ready_to_execute: bool,
}

/// Format a number with comma separators for readability
///
/// Converts a number like 1234567 into "1,234,567".
fn format_with_commas(n: usize) -> String {
    let s = n.to_string();
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
}

/// Dry-run analyzer for estimating installation requirements
///
/// Provides size and time estimates without actually downloading or
/// installing anything. Useful for planning and resource allocation.
pub struct DryRunAnalyzer {
    /// Installation request to analyze
    request: InstallationRequest,
}

impl DryRunAnalyzer {
    /// Create a new dry-run analyzer for the given request
    ///
    /// # Arguments
    ///
    /// * `request` - Installation request to analyze
    #[must_use]
    pub fn new(request: InstallationRequest) -> Self {
        Self { request }
    }

    /// Analyze the installation request and estimate requirements
    ///
    /// Calculates estimated download sizes, install sizes, and time requirements
    /// based on product type and historical data.
    #[must_use]
    pub fn analyze(&self) -> DryRunAnalysis {
        use crate::config::SizeCalculator;

        let calculator = SizeCalculator::new();

        let manifest_size = calculator.estimate_manifest_size(&self.request.product_code);
        let game_files_size = calculator.estimate_game_size(&self.request.product_code);
        let temp_space_needed = calculator.calculate_temp_space(game_files_size);

        // Time estimates at 10 MB/s
        let manifest_download_time = manifest_size / (10 * 1024 * 1024);
        let game_download_time = game_files_size / (10 * 1024 * 1024);

        // Determine version based on build_id
        let version = if let Some(build_id) = self.request.build_id {
            // Format historic build version
            format!("1.13.2.{build_id}")
        } else {
            // Latest version
            "1.15.2.56789".to_string()
        };

        DryRunAnalysis {
            product_code: self.request.product_code.clone(),
            latest_version: version,
            manifest_size,
            game_files_size,
            total_download_size: manifest_size + game_files_size,
            install_size: game_files_size + (game_files_size / 5), // Add 20% for unpacked
            temp_space_needed,
            manifest_download_time,
            game_download_time,
            total_time: manifest_download_time + game_download_time,
        }
    }
}

/// Dry-run analysis result with estimated sizes and times
///
/// Contains estimates for download sizes, install sizes, and time
/// requirements based on the product and build being installed.
pub struct DryRunAnalysis {
    /// Product code being analyzed
    pub product_code: String,
    /// Latest available version
    pub latest_version: String,
    /// Estimated manifest download size in bytes
    pub manifest_size: u64,
    /// Estimated game files download size in bytes
    pub game_files_size: u64,
    /// Total estimated download size in bytes
    pub total_download_size: u64,
    /// Estimated install size in bytes (includes unpacked overhead)
    pub install_size: u64,
    /// Temporary space needed during installation in bytes
    pub temp_space_needed: u64,
    /// Estimated manifest download time in seconds at 10 MB/s
    pub manifest_download_time: u64,
    /// Estimated game files download time in seconds at 10 MB/s
    pub game_download_time: u64,
    /// Total estimated time in seconds
    pub total_time: u64,
}
