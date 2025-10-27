//! Progress tracking for installation

/// Progress callback trait for reporting download progress
pub trait ProgressCallback: Send + Sync {
    /// Called when a file download starts
    fn on_file_start(&mut self, path: &str, size: u64);

    /// Called periodically during download
    #[allow(dead_code)] // Future progress reporting
    fn on_progress(&mut self, downloaded: u64, total: u64);

    /// Called when a file download completes
    fn on_file_complete(&mut self, path: &str);

    /// Called when an error occurs
    fn on_error(&mut self, error: &str);

    /// Called after successful completion to clean up any persistent state
    fn on_completion_cleanup(&self) {
        // Default implementation does nothing
    }
}

/// No-op progress callback
pub struct NoOpProgress;

impl ProgressCallback for NoOpProgress {
    fn on_file_start(&mut self, _path: &str, _size: u64) {}
    fn on_progress(&mut self, _downloaded: u64, _total: u64) {}
    fn on_file_complete(&mut self, _path: &str) {}
    fn on_error(&mut self, _error: &str) {}
}
