//! Product model for agent service
//!
//! Represents a game or application that can be installed, updated, or removed.
//! Based on data-model.md Product entity specification.

use serde::{Deserialize, Serialize};

/// Product status enum
///
/// Represents the current state of a product in the agent's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductStatus {
    /// Product can be installed but is not currently installed
    Available,
    /// Installation in progress
    Installing,
    /// Product is installed and ready to use
    Installed,
    /// Update in progress
    Updating,
    /// Repair operation in progress
    Repairing,
    /// Verification in progress
    Verifying,
    /// Removal in progress
    Uninstalling,
    /// Installed but verification failed
    Corrupted,
}

/// Installation mode enum
///
/// Determines how product files are organized on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationMode {
    /// Container-based installation (full CASC structure)
    Casc,
    /// Direct file installation (no CASC container)
    Containerless,
}

impl Default for InstallationMode {
    fn default() -> Self {
        Self::Casc
    }
}

/// Product model
///
/// Represents a game or application that can be managed by the agent.
/// Implements validation rules and state transitions from data-model.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    /// Unique product identifier (e.g., wow, `wow_classic`, wowt)
    pub product_code: String,

    /// Human-readable product name
    pub name: String,

    /// Current product state
    pub status: ProductStatus,

    /// Installed version number (null if not installed)
    pub version: Option<String>,

    /// Absolute path to installation directory
    pub install_path: Option<String>,

    /// Total installation size in bytes
    pub size_bytes: Option<u64>,

    /// CDN region (us, eu, kr, cn, tw)
    pub region: Option<String>,

    /// Game locale (enUS, deDE, etc.)
    pub locale: Option<String>,

    /// Installation mode (CASC or containerless)
    pub installation_mode: Option<InstallationMode>,

    /// Whether an update is available for this product (T087)
    pub is_update_available: Option<bool>,

    /// Version number available for update (T087)
    pub available_version: Option<String>,

    /// When product was first seen
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last modification time
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// Future use: T078 (main.rs product management)
#[allow(dead_code)]
impl Product {
    /// Create a new product in Available state
    #[must_use]
    pub fn new(product_code: String, name: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            product_code,
            name,
            status: ProductStatus::Available,
            version: None,
            install_path: None,
            size_bytes: None,
            region: None,
            locale: None,
            installation_mode: None,
            is_update_available: None,
            available_version: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validate product data according to data-model.md rules
    ///
    /// # Validation Rules
    ///
    /// - `product_code` must match pattern `^[a-z0-9_]+$`
    /// - region must be one of: us, eu, kr, cn, tw (if not null)
    /// - locale must match pattern `^[a-z]{2}[A-Z]{2}$` (if not null)
    /// - If status requires installation data, all fields must be set
    pub fn validate(&self) -> Result<(), String> {
        // Validate product_code pattern
        if !self
            .product_code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err("Invalid product_code: must match ^[a-z0-9_]+$".to_string());
        }

        // Validate region if set
        if let Some(ref region) = self.region {
            if !matches!(region.as_str(), "us" | "eu" | "kr" | "cn" | "tw") {
                return Err("Invalid region: must be us, eu, kr, cn, or tw".to_string());
            }
        }

        // Validate locale format if set
        if let Some(ref locale) = self.locale {
            if locale.len() != 4 {
                return Err("Invalid locale: must be 4 characters (e.g., enUS)".to_string());
            }
            let chars: Vec<char> = locale.chars().collect();
            if !chars[0].is_ascii_lowercase()
                || !chars[1].is_ascii_lowercase()
                || !chars[2].is_ascii_uppercase()
                || !chars[3].is_ascii_uppercase()
            {
                return Err("Invalid locale: must match pattern [a-z]{2}[A-Z]{2}".to_string());
            }
        }

        // Validate installation data requirements based on status
        match self.status {
            ProductStatus::Installed
            | ProductStatus::Updating
            | ProductStatus::Repairing
            | ProductStatus::Verifying
            | ProductStatus::Corrupted => {
                if self.version.is_none() {
                    return Err("version must be set for installed products".to_string());
                }
                if self.install_path.is_none() {
                    return Err("install_path must be set for installed products".to_string());
                }
                if self.region.is_none() {
                    return Err("region must be set for installed products".to_string());
                }
                if self.locale.is_none() {
                    return Err("locale must be set for installed products".to_string());
                }
                if self.installation_mode.is_none() {
                    return Err("installation_mode must be set for installed products".to_string());
                }
            }
            ProductStatus::Available | ProductStatus::Installing | ProductStatus::Uninstalling => {
                // These states allow optional installation data
            }
        }

        Ok(())
    }

    /// Check if a state transition is valid
    ///
    /// Implements state transition rules from data-model.md:
    /// - Available → Installing
    /// - Installing → Installed
    /// - Installed → Verifying, Repairing, Updating, Uninstalling
    /// - Updating → Installed
    /// - Repairing → Installed
    /// - Verifying → Installed, Corrupted
    /// - Uninstalling → Available
    #[must_use]
    pub fn can_transition_to(&self, new_status: ProductStatus) -> bool {
        match (self.status, new_status) {
            // Available can only transition to Installing
            (ProductStatus::Available, ProductStatus::Installing) => true,

            // Installing can transition to Installed
            (ProductStatus::Installing, ProductStatus::Installed) => true,

            // Installed can transition to various operations
            (ProductStatus::Installed, ProductStatus::Verifying) => true,
            (ProductStatus::Installed, ProductStatus::Repairing) => true,
            (ProductStatus::Installed, ProductStatus::Updating) => true,
            (ProductStatus::Installed, ProductStatus::Uninstalling) => true,

            // Operation states can transition back to Installed or to Corrupted
            (ProductStatus::Updating, ProductStatus::Installed) => true,
            (ProductStatus::Repairing, ProductStatus::Installed) => true,
            (ProductStatus::Verifying, ProductStatus::Installed) => true,
            (ProductStatus::Verifying, ProductStatus::Corrupted) => true,

            // Corrupted can transition to Repairing or Uninstalling
            (ProductStatus::Corrupted, ProductStatus::Repairing) => true,
            (ProductStatus::Corrupted, ProductStatus::Uninstalling) => true,

            // Uninstalling can transition to Available
            (ProductStatus::Uninstalling, ProductStatus::Available) => true,

            // Same state is always valid (no-op)
            (a, b) if a == b => true,

            // All other transitions are invalid
            _ => false,
        }
    }

    /// Update the product status with timestamp
    pub fn set_status(&mut self, new_status: ProductStatus) {
        self.status = new_status;
        self.updated_at = chrono::Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_product() {
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        assert_eq!(product.product_code, "wow");
        assert_eq!(product.name, "World of Warcraft");
        assert_eq!(product.status, ProductStatus::Available);
        assert!(product.version.is_none());
        assert!(product.validate().is_ok());
    }

    #[test]
    fn test_product_code_validation() {
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());
        assert!(product.validate().is_ok());

        product.product_code = "WoW".to_string(); // Uppercase not allowed
        assert!(product.validate().is_err());

        product.product_code = "wow-classic".to_string(); // Hyphen not allowed
        assert!(product.validate().is_err());

        product.product_code = "wow_classic".to_string(); // Underscore allowed
        assert!(product.validate().is_ok());
    }

    #[test]
    fn test_region_validation() {
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        product.region = Some("us".to_string());
        assert!(product.validate().is_ok());

        product.region = Some("invalid".to_string());
        assert!(product.validate().is_err());

        product.region = Some("eu".to_string());
        assert!(product.validate().is_ok());
    }

    #[test]
    fn test_locale_validation() {
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        product.locale = Some("enUS".to_string());
        assert!(product.validate().is_ok());

        product.locale = Some("deDE".to_string());
        assert!(product.validate().is_ok());

        product.locale = Some("invalid".to_string());
        assert!(product.validate().is_err());

        product.locale = Some("ENUS".to_string()); // Wrong case
        assert!(product.validate().is_err());
    }

    #[test]
    fn test_installed_product_validation() {
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());
        product.status = ProductStatus::Installed;

        // Missing required fields
        assert!(product.validate().is_err());

        // Set all required fields
        product.version = Some("10.2.0.52607".to_string());
        product.install_path = Some("/games/wow".to_string());
        product.region = Some("us".to_string());
        product.locale = Some("enUS".to_string());
        product.installation_mode = Some(InstallationMode::Casc);

        assert!(product.validate().is_ok());
    }

    #[test]
    fn test_state_transitions() {
        let product = Product::new("wow".to_string(), "World of Warcraft".to_string());

        // Available → Installing (valid)
        assert!(product.can_transition_to(ProductStatus::Installing));

        // Available → Installed (invalid, must go through Installing)
        assert!(!product.can_transition_to(ProductStatus::Installed));

        let mut product = product;
        product.set_status(ProductStatus::Installing);

        // Installing → Installed (valid)
        assert!(product.can_transition_to(ProductStatus::Installed));

        product.set_status(ProductStatus::Installed);

        // Installed → Updating (valid)
        assert!(product.can_transition_to(ProductStatus::Updating));

        // Installed → Available (invalid, must go through Uninstalling)
        assert!(!product.can_transition_to(ProductStatus::Available));
    }

    #[test]
    fn test_corrupted_transitions() {
        let mut product = Product::new("wow".to_string(), "World of Warcraft".to_string());
        product.set_status(ProductStatus::Verifying);

        // Verifying → Corrupted (valid)
        assert!(product.can_transition_to(ProductStatus::Corrupted));

        product.set_status(ProductStatus::Corrupted);

        // Corrupted → Repairing (valid)
        assert!(product.can_transition_to(ProductStatus::Repairing));

        // Corrupted → Uninstalling (valid)
        assert!(product.can_transition_to(ProductStatus::Uninstalling));

        // Corrupted → Available (invalid)
        assert!(!product.can_transition_to(ProductStatus::Available));
    }
}
