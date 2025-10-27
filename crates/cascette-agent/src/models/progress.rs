//! Progress model for agent service
//!
//! Represents current progress metrics for an operation.
//! Based on data-model.md Progress embedded type specification.

use serde::{Deserialize, Serialize};

/// Progress metrics for an operation
///
/// Provides detailed progress information including:
/// - Current phase and percentage
/// - Bytes and files progress
/// - Download speed and ETA
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    /// Current operation phase (downloading, verifying, etc.)
    pub phase: String,

    /// Completion percentage (0.0-100.0)
    pub percentage: f64,

    /// Bytes downloaded so far
    pub bytes_downloaded: u64,

    /// Total bytes to download
    pub bytes_total: u64,

    /// Files completed so far
    pub files_completed: usize,

    /// Total files to process
    pub files_total: usize,

    /// Current file being processed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,

    /// Download speed in bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_speed_bps: Option<u64>,

    /// Estimated time to completion in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u64>,
}

// Future use: T078 (main.rs progress tracking)
#[allow(dead_code)]
impl Progress {
    /// Create a new progress tracker
    #[must_use]
    pub fn new(phase: String, bytes_total: u64, files_total: usize) -> Self {
        Self {
            phase,
            percentage: 0.0,
            bytes_downloaded: 0,
            bytes_total,
            files_completed: 0,
            files_total,
            current_file: None,
            download_speed_bps: None,
            eta_seconds: None,
        }
    }

    /// Update bytes progress and recalculate percentage
    pub fn update_bytes(&mut self, bytes_downloaded: u64) {
        self.bytes_downloaded = bytes_downloaded;
        self.recalculate_percentage();
    }

    /// Update files progress and recalculate percentage
    pub fn update_files(&mut self, files_completed: usize) {
        self.files_completed = files_completed;
        self.recalculate_percentage();
    }

    /// Set current file being processed
    pub fn set_current_file(&mut self, filename: String) {
        self.current_file = Some(filename);
    }

    /// Set download speed in bytes per second
    pub fn set_download_speed(&mut self, speed_bps: u64) {
        self.download_speed_bps = Some(speed_bps);
        self.recalculate_eta();
    }

    /// Recalculate completion percentage based on bytes
    fn recalculate_percentage(&mut self) {
        if self.bytes_total > 0 {
            self.percentage = (self.bytes_downloaded as f64 / self.bytes_total as f64) * 100.0;
            self.percentage = self.percentage.min(100.0); // Cap at 100%
        }
    }

    /// Recalculate ETA based on download speed
    fn recalculate_eta(&mut self) {
        if let Some(speed) = self.download_speed_bps {
            if speed > 0 {
                let remaining_bytes = self.bytes_total.saturating_sub(self.bytes_downloaded);
                self.eta_seconds = Some(remaining_bytes / speed);
            }
        }
    }

    /// Check if operation is complete
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.bytes_downloaded >= self.bytes_total && self.files_completed >= self.files_total
    }

    /// Format bytes as human-readable string
    #[must_use]
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{bytes} B")
        }
    }

    /// Format download speed as human-readable string
    #[must_use]
    pub fn format_speed(&self) -> String {
        if let Some(speed) = self.download_speed_bps {
            format!("{}/s", Self::format_bytes(speed))
        } else {
            "N/A".to_string()
        }
    }

    /// Format ETA as human-readable string
    #[must_use]
    pub fn format_eta(&self) -> String {
        if let Some(eta) = self.eta_seconds {
            let hours = eta / 3600;
            let minutes = (eta % 3600) / 60;
            let seconds = eta % 60;

            if hours > 0 {
                format!("{hours}h {minutes}m {seconds}s")
            } else if minutes > 0 {
                format!("{minutes}m {seconds}s")
            } else {
                format!("{seconds}s")
            }
        } else {
            "N/A".to_string()
        }
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            phase: "initializing".to_string(),
            percentage: 0.0,
            bytes_downloaded: 0,
            bytes_total: 0,
            files_completed: 0,
            files_total: 0,
            current_file: None,
            download_speed_bps: None,
            eta_seconds: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_progress() {
        let progress = Progress::new("downloading".to_string(), 1000000, 100);

        assert_eq!(progress.phase, "downloading");
        assert_eq!(progress.percentage, 0.0);
        assert_eq!(progress.bytes_total, 1000000);
        assert_eq!(progress.files_total, 100);
        assert!(!progress.is_complete());
    }

    #[test]
    fn test_update_bytes() {
        let mut progress = Progress::new("downloading".to_string(), 1000, 10);

        progress.update_bytes(500);
        assert_eq!(progress.bytes_downloaded, 500);
        assert_eq!(progress.percentage, 50.0);

        progress.update_bytes(1000);
        assert_eq!(progress.percentage, 100.0);

        // Test capping at 100%
        progress.update_bytes(1500);
        assert_eq!(progress.percentage, 100.0);
    }

    #[test]
    fn test_update_files() {
        let mut progress = Progress::new("downloading".to_string(), 1000, 10);

        progress.update_files(5);
        assert_eq!(progress.files_completed, 5);
    }

    #[test]
    fn test_is_complete() {
        let mut progress = Progress::new("downloading".to_string(), 1000, 10);

        assert!(!progress.is_complete());

        progress.update_bytes(1000);
        progress.update_files(10);

        assert!(progress.is_complete());
    }

    #[test]
    fn test_download_speed_and_eta() {
        let mut progress = Progress::new("downloading".to_string(), 10000, 10);

        progress.update_bytes(2000);
        progress.set_download_speed(1000); // 1000 bytes/sec

        assert_eq!(progress.download_speed_bps, Some(1000));
        assert_eq!(progress.eta_seconds, Some(8)); // (10000 - 2000) / 1000
    }

    #[test]
    fn test_current_file() {
        let mut progress = Progress::new("downloading".to_string(), 1000, 10);

        progress.set_current_file("data.015".to_string());
        assert_eq!(progress.current_file, Some("data.015".to_string()));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(Progress::format_bytes(512), "512 B");
        assert_eq!(Progress::format_bytes(1024), "1.00 KB");
        assert_eq!(Progress::format_bytes(1536), "1.50 KB");
        assert_eq!(Progress::format_bytes(1048576), "1.00 MB");
        assert_eq!(Progress::format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_speed() {
        let mut progress = Progress::new("downloading".to_string(), 1000, 10);

        assert_eq!(progress.format_speed(), "N/A");

        progress.set_download_speed(1024);
        assert_eq!(progress.format_speed(), "1.00 KB/s");
    }

    #[test]
    fn test_format_eta() {
        let mut progress = Progress::new("downloading".to_string(), 10000, 10);

        assert_eq!(progress.format_eta(), "N/A");

        progress.eta_seconds = Some(30);
        assert_eq!(progress.format_eta(), "30s");

        progress.eta_seconds = Some(90);
        assert_eq!(progress.format_eta(), "1m 30s");

        progress.eta_seconds = Some(3665);
        assert_eq!(progress.format_eta(), "1h 1m 5s");
    }

    #[test]
    fn test_serialization() {
        let progress = Progress::new("downloading".to_string(), 1000000, 100);

        let json = serde_json::to_string(&progress).expect("Failed to serialize JSON");
        let deserialized: Progress =
            serde_json::from_str(&json).expect("Failed to deserialize JSON");

        assert_eq!(deserialized.phase, progress.phase);
        assert_eq!(deserialized.bytes_total, progress.bytes_total);
    }
}
