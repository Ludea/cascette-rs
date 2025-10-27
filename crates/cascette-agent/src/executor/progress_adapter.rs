//! Progress adapter for cascette-installation integration
//!
//! Adapts between cascette-installation's trait-based `ProgressCallback`
//! and cascette-agent's Progress model for operation tracking.

use crate::models::Progress;
use cascette_installation::progress::ProgressCallback;
use std::sync::Arc;
use std::sync::Mutex;

/// Adapter that implements `ProgressCallback` and updates agent Progress model
///
/// Converts cascette-installation progress events into agent progress updates
/// with proper field mapping and state tracking.
pub struct ProgressAdapter {
    /// Shared progress state
    progress: Arc<Mutex<Progress>>,

    /// Total files to process (set on first file start)
    total_files: Arc<Mutex<usize>>,

    /// Files completed so far
    files_completed: Arc<Mutex<usize>>,
}

impl ProgressAdapter {
    /// Create a new progress adapter
    pub fn new(progress: Arc<Mutex<Progress>>) -> Self {
        Self {
            progress,
            total_files: Arc::new(Mutex::new(0)),
            files_completed: Arc::new(Mutex::new(0)),
        }
    }
}

impl ProgressCallback for ProgressAdapter {
    fn on_file_start(&mut self, path: &str, size: u64) {
        let progress = self.progress.clone();
        let total_files = self.total_files.clone();
        let path = path.to_string();

        tokio::spawn(async move {
            let mut progress = progress.lock().expect("Mutex poisoned");
            let mut total = total_files.lock().expect("Mutex poisoned");

            // Track total files (increment on each file start)
            *total += 1;

            // Update current file
            progress.set_current_file(path);

            // Add to total bytes if not already counted
            // Note: This is an approximation as we don't have total from cascette-installation
            if progress.bytes_total == 0 {
                progress.bytes_total = size;
            } else {
                progress.bytes_total += size;
            }
        });
    }

    fn on_progress(&mut self, downloaded: u64, _total: u64) {
        let progress = self.progress.clone();

        tokio::spawn(async move {
            let mut progress = progress.lock().expect("Mutex poisoned");
            progress.update_bytes(downloaded);
        });
    }

    fn on_file_complete(&mut self, path: &str) {
        let progress = self.progress.clone();
        let files_completed = self.files_completed.clone();
        let total_files = self.total_files.clone();
        let path = path.to_string();

        tokio::spawn(async move {
            let mut progress = progress.lock().expect("Mutex poisoned");
            let mut completed = files_completed.lock().expect("Mutex poisoned");
            let total = total_files.lock().expect("Mutex poisoned");

            // Increment completed files
            *completed += 1;

            // Update progress model
            progress.files_total = *total;
            progress.update_files(*completed);

            // Clear current file if this is the last one
            if *completed >= *total {
                progress.current_file = None;
            }

            tracing::debug!(
                file = %path,
                completed = *completed,
                total = *total,
                "File download completed"
            );
        });
    }

    fn on_error(&mut self, error: &str) {
        tracing::error!(error = %error, "Installation progress error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_progress_adapter_file_tracking() {
        let progress = Arc::new(Mutex::new(Progress::default()));
        let mut adapter = ProgressAdapter::new(progress.clone());

        // Simulate file downloads
        adapter.on_file_start("file1.dat", 1000);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        adapter.on_progress(500, 1000);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        adapter.on_file_complete("file1.dat");
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let progress = progress.lock().expect("Mutex poisoned");
        assert_eq!(progress.files_completed, 1);
        assert!(progress.bytes_downloaded > 0);
    }

    #[tokio::test]
    async fn test_progress_adapter_error_handling() {
        let progress = Arc::new(Mutex::new(Progress::default()));
        let mut adapter = ProgressAdapter::new(progress.clone());

        // Should not panic
        adapter.on_error("Test error");
    }
}
