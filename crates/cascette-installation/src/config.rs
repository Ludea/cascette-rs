//! Configuration file handling for installation plans

/// Size calculator for dry-run analysis
pub struct SizeCalculator;

impl Default for SizeCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeCalculator {
    /// Create a new size calculator
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Estimate manifest download size for a product
    ///
    /// # Arguments
    ///
    /// * `product` - Product code to estimate for
    ///
    /// # Returns
    ///
    /// Estimated manifest size in bytes
    #[must_use]
    pub fn estimate_manifest_size(&self, product: &str) -> u64 {
        // Rough estimates based on product
        match product {
            "wow_classic" => 25_000_000,
            "wow_retail" => 35_000_000,
            _ => 20_000_000,
        }
    }

    /// Estimate total game files download size for a product
    ///
    /// # Arguments
    ///
    /// * `product` - Product code to estimate for
    ///
    /// # Returns
    ///
    /// Estimated game files size in bytes
    #[must_use]
    pub fn estimate_game_size(&self, product: &str) -> u64 {
        // Rough estimates based on product
        match product {
            "wow_classic" => 15_000_000_000,
            "wow_retail" => 25_000_000_000,
            _ => 10_000_000_000,
        }
    }

    /// Calculate temporary space needed during installation
    ///
    /// # Arguments
    ///
    /// * `game_size` - Total game files size in bytes
    ///
    /// # Returns
    ///
    /// Required temporary space in bytes (125% of game size)
    #[must_use]
    pub fn calculate_temp_space(&self, game_size: u64) -> u64 {
        // Need extra space for temp files during installation
        game_size + (game_size / 4) // 125% of game size
    }
}
