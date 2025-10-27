//! Enhanced progress tracking with persistence and statistics
//!
//! This module provides detailed progress tracking that persists across sessions
//! and calculates useful statistics like ETA and bandwidth usage.

use super::error::{InstallationError, Result};
use super::progress::ProgressCallback;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Persistent progress tracking state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressState {
    /// Total number of files to install
    pub total_files: usize,
    /// Number of completed files
    pub completed_files: usize,
    /// Total bytes to download
    pub total_bytes: u64,
    /// Bytes already downloaded
    pub downloaded_bytes: u64,
    /// Individual file progress
    pub file_progress: HashMap<String, FileProgress>,
    /// Timestamp of last update
    pub last_updated: SystemTime,
    /// Installation start time
    pub started_at: SystemTime,
    /// Current download rate (bytes per second)
    pub download_rate: f64,
}

/// Progress information for individual files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProgress {
    /// File path
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Bytes downloaded so far
    pub downloaded: u64,
    /// Whether the file is complete
    pub complete: bool,
    /// Time taken to download (milliseconds)
    pub download_time_ms: Option<u64>,
}

/// Enhanced console progress callback with persistence
pub struct PersistentProgressCallback {
    state: ProgressState,
    state_path: PathBuf,
    current_file: Option<String>,
    current_file_start: Option<Instant>,
    last_save: Instant,
    save_interval: Duration,
    /// Running average of download speeds for ETA calculation
    speed_samples: Vec<f64>,
    max_speed_samples: usize,
    /// Whether to show detailed progress
    verbose: bool,
}

impl PersistentProgressCallback {
    /// Create a new persistent progress callback
    #[must_use]
    pub fn new(target_dir: &Path, total_files: usize, total_bytes: u64, verbose: bool) -> Self {
        let state_path = target_dir.join(".cascette").join("progress.json");

        // Try to load existing progress state
        let state = if state_path.exists() {
            match Self::load_state(&state_path) {
                Ok(existing) => {
                    println!(
                        "→ Resuming progress tracking: {}/{} files ({:.1}% complete)",
                        existing.completed_files,
                        existing.total_files,
                        existing.completion_percentage()
                    );
                    existing
                }
                Err(_) => {
                    // Create new state if loading fails
                    ProgressState::new(total_files, total_bytes)
                }
            }
        } else {
            ProgressState::new(total_files, total_bytes)
        };

        Self {
            state,
            state_path,
            current_file: None,
            current_file_start: None,
            last_save: Instant::now(),
            save_interval: Duration::from_secs(5), // Save every 5 seconds
            speed_samples: Vec::with_capacity(20),
            max_speed_samples: 20,
            verbose,
        }
    }

    /// Load progress state from disk
    fn load_state(path: &Path) -> Result<ProgressState> {
        let json = fs::read_to_string(path)
            .map_err(|e| InstallationError::Other(format!("Failed to read progress state: {e}")))?;

        serde_json::from_str(&json)
            .map_err(|e| InstallationError::Other(format!("Failed to parse progress state: {e}")))
    }

    /// Save progress state to disk
    pub fn save(&self) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                InstallationError::Other(format!("Failed to create progress directory: {e}"))
            })?;
        }

        let json = serde_json::to_string_pretty(&self.state).map_err(|e| {
            InstallationError::Other(format!("Failed to serialize progress state: {e}"))
        })?;

        fs::write(&self.state_path, json).map_err(|e| {
            InstallationError::Other(format!("Failed to write progress state: {e}"))
        })?;

        Ok(())
    }

    /// Update download speed tracking
    fn update_speed(&mut self, bytes: u64, duration: Duration) {
        if duration.as_millis() > 0 {
            let speed = bytes as f64 / duration.as_secs_f64();
            self.speed_samples.push(speed);
            if self.speed_samples.len() > self.max_speed_samples {
                self.speed_samples.remove(0);
            }

            // Update average download rate
            if !self.speed_samples.is_empty() {
                self.state.download_rate =
                    self.speed_samples.iter().sum::<f64>() / self.speed_samples.len() as f64;
            }
        }
    }

    /// Calculate estimated time remaining
    #[must_use]
    pub fn calculate_eta(&self) -> Option<Duration> {
        if self.state.download_rate > 0.0 {
            let remaining_bytes = self
                .state
                .total_bytes
                .saturating_sub(self.state.downloaded_bytes);
            let seconds_remaining = remaining_bytes as f64 / self.state.download_rate;
            Some(Duration::from_secs_f64(seconds_remaining))
        } else {
            None
        }
    }

    /// Format duration for display
    fn format_duration(duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{hours}h {minutes}m {seconds}s")
        } else if minutes > 0 {
            format!("{minutes}m {seconds}s")
        } else {
            format!("{seconds}s")
        }
    }

    /// Format bytes for display
    fn format_bytes(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        format!("{:.2} {}", size, UNITS[unit_index])
    }

    /// Display current progress summary
    pub fn display_summary(&self) {
        let percentage = self.state.completion_percentage();
        let downloaded = Self::format_bytes(self.state.downloaded_bytes);
        let total = Self::format_bytes(self.state.total_bytes);

        println!("\n📊 Installation Progress:");
        println!(
            "  Files: {}/{} ({:.1}%)",
            self.state.completed_files, self.state.total_files, percentage
        );
        println!("  Data: {downloaded} / {total}");

        if self.state.download_rate > 0.0 {
            let speed = Self::format_bytes(self.state.download_rate as u64);
            println!("  Speed: {speed}/s");

            if let Some(eta) = self.calculate_eta() {
                println!("  ETA: {}", Self::format_duration(eta));
            }
        }

        // Progress bar
        let bar_width = 40;
        let filled = (bar_width as f32 * percentage / 100.0) as usize;
        let empty = bar_width - filled;
        print!("  [");
        print!("{}", "█".repeat(filled));
        print!("{}", "░".repeat(empty));
        println!("] {percentage:.1}%");
    }

    /// Check if we should save state
    fn should_save(&self) -> bool {
        self.last_save.elapsed() >= self.save_interval
    }

    /// Clear the progress state after successful completion
    pub fn clear(&self) -> Result<()> {
        if self.state_path.exists() {
            fs::remove_file(&self.state_path).map_err(|e| {
                InstallationError::Other(format!("Failed to remove progress file: {e}"))
            })?;
        }
        Ok(())
    }
}

impl ProgressState {
    /// Create a new progress state
    fn new(total_files: usize, total_bytes: u64) -> Self {
        Self {
            total_files,
            completed_files: 0,
            total_bytes,
            downloaded_bytes: 0,
            file_progress: HashMap::new(),
            last_updated: SystemTime::now(),
            started_at: SystemTime::now(),
            download_rate: 0.0,
        }
    }

    /// Calculate completion percentage
    #[must_use]
    pub fn completion_percentage(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.downloaded_bytes as f32 / self.total_bytes as f32) * 100.0
    }

    /// Check if a file has been completed
    #[allow(dead_code)]
    #[must_use]
    pub fn is_file_complete(&self, path: &str) -> bool {
        self.file_progress.get(path).is_some_and(|fp| fp.complete)
    }
}

impl ProgressCallback for PersistentProgressCallback {
    fn on_file_start(&mut self, path: &str, size: u64) {
        self.current_file = Some(path.to_string());
        self.current_file_start = Some(Instant::now());

        // Add or update file progress entry
        self.state.file_progress.insert(
            path.to_string(),
            FileProgress {
                path: path.to_string(),
                size,
                downloaded: 0,
                complete: false,
                download_time_ms: None,
            },
        );

        if self.verbose {
            println!("  → Downloading: {} ({})", path, Self::format_bytes(size));
        }
    }

    fn on_progress(&mut self, downloaded: u64, total: u64) {
        if let Some(ref path) = self.current_file {
            if let Some(fp) = self.state.file_progress.get_mut(path) {
                let bytes_delta = downloaded.saturating_sub(fp.downloaded);
                fp.downloaded = downloaded;

                // Update total downloaded bytes
                self.state.downloaded_bytes += bytes_delta;

                // Update speed tracking
                if let Some(start) = self.current_file_start {
                    self.update_speed(downloaded, start.elapsed());
                }
            }
        }

        // Save state periodically
        if self.should_save() {
            self.state.last_updated = SystemTime::now();
            let _ = self.save();
            self.last_save = Instant::now();
        }

        // Display progress for large files
        if self.verbose && total > 1_000_000 {
            // Files > 1MB
            let percent = (downloaded as f64 / total as f64) * 100.0;
            print!("\r    Progress: {percent:.1}% ");
            let _ = io::stdout().flush();
        }
    }

    fn on_file_complete(&mut self, path: &str) {
        if let Some(fp) = self.state.file_progress.get_mut(path) {
            fp.complete = true;
            if let Some(start) = self.current_file_start {
                fp.download_time_ms = Some(start.elapsed().as_millis() as u64);
            }

            // Update counts
            self.state.completed_files += 1;
            self.state.downloaded_bytes = self.state.downloaded_bytes.max(fp.downloaded);
        }

        self.current_file = None;
        self.current_file_start = None;

        // Clear the progress line if we were showing it
        if self.verbose {
            print!("\r"); // Clear the line
            println!("  ✓ Completed: {path}");
        }

        // Show summary every 10 files or at milestones
        if self.state.completed_files % 10 == 0
            || self.state.completed_files == self.state.total_files
        {
            self.display_summary();
        }

        // Save state
        self.state.last_updated = SystemTime::now();
        let _ = self.save();
        self.last_save = Instant::now();
    }

    fn on_error(&mut self, error: &str) {
        println!("  ✗ Error: {error}");

        // Save state on error
        self.state.last_updated = SystemTime::now();
        let _ = self.save();
    }

    fn on_completion_cleanup(&self) {
        // Clear the persistent progress state after successful completion
        let _ = self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_progress_state_creation() {
        let state = ProgressState::new(100, 1_000_000);
        assert_eq!(state.total_files, 100);
        assert_eq!(state.total_bytes, 1_000_000);
        assert_eq!(state.completed_files, 0);
        assert_eq!(state.downloaded_bytes, 0);
        assert!((state.completion_percentage() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_progress_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut callback = PersistentProgressCallback::new(temp_dir.path(), 10, 10_000, false);

        callback.on_file_start("test.dat", 1000);
        callback.on_progress(500, 1000);
        callback.on_file_complete("test.dat");

        callback.save().expect("Failed to save progress");

        // Load the saved state
        let state_path = temp_dir.path().join(".cascette").join("progress.json");
        let loaded_state =
            PersistentProgressCallback::load_state(&state_path).expect("Failed to load progress");

        assert_eq!(loaded_state.completed_files, 1);
        assert!(loaded_state.is_file_complete("test.dat"));
    }

    #[test]
    fn test_eta_calculation() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut callback =
            PersistentProgressCallback::new(temp_dir.path(), 100, 100_000_000, false);

        // Simulate some download speed
        callback.state.download_rate = 1_000_000.0; // 1 MB/s
        callback.state.downloaded_bytes = 10_000_000; // 10 MB downloaded

        let eta = callback.calculate_eta().expect("Should calculate ETA");

        // Should be 90 seconds (90 MB remaining at 1 MB/s)
        assert_eq!(eta.as_secs(), 90);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            PersistentProgressCallback::format_duration(Duration::from_secs(45)),
            "45s"
        );
        assert_eq!(
            PersistentProgressCallback::format_duration(Duration::from_secs(125)),
            "2m 5s"
        );
        assert_eq!(
            PersistentProgressCallback::format_duration(Duration::from_secs(3665)),
            "1h 1m 5s"
        );
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(PersistentProgressCallback::format_bytes(512), "512.00 B");
        assert_eq!(PersistentProgressCallback::format_bytes(1536), "1.50 KB");
        assert_eq!(
            PersistentProgressCallback::format_bytes(1_048_576),
            "1.00 MB"
        );
        assert_eq!(
            PersistentProgressCallback::format_bytes(1_073_741_824),
            "1.00 GB"
        );
    }
}
