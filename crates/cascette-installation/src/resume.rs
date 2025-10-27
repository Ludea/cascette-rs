//! Installation resume support for interrupted downloads
//!
//! This module provides state persistence to allow installations to resume
//! after interruption without re-downloading completed files.

use super::error::{InstallationError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Installation state for resume capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationState {
    /// Set of completed file paths
    pub completed_files: HashSet<String>,
    /// Hash of the installation plan for validation
    pub plan_hash: String,
    /// Last update timestamp
    pub last_updated: SystemTime,
    /// Total files in the installation
    pub total_files: usize,
    /// Build ID being installed
    pub build_id: u32,
    /// Product code being installed
    pub product_code: String,
}

impl InstallationState {
    /// Create a new installation state
    #[must_use]
    pub fn new(plan_hash: String, total_files: usize, build_id: u32, product_code: String) -> Self {
        Self {
            completed_files: HashSet::new(),
            plan_hash,
            last_updated: SystemTime::now(),
            total_files,
            build_id,
            product_code,
        }
    }

    /// Mark a file as completed
    pub fn mark_completed(&mut self, file_path: String) {
        self.completed_files.insert(file_path);
        self.last_updated = SystemTime::now();
    }

    /// Check if a file has been completed
    #[must_use]
    pub fn is_completed(&self, file_path: &str) -> bool {
        self.completed_files.contains(file_path)
    }

    /// Get the number of completed files
    #[must_use]
    pub fn completed_count(&self) -> usize {
        self.completed_files.len()
    }

    /// Get the percentage of completion
    #[must_use]
    pub fn completion_percentage(&self) -> f32 {
        if self.total_files == 0 {
            return 0.0;
        }
        (self.completed_files.len() as f32 / self.total_files as f32) * 100.0
    }

    /// Save state to file
    pub fn save(&self, state_path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                InstallationError::Other(format!("Failed to create state directory: {e}"))
            })?;
        }

        let json = serde_json::to_string_pretty(self).map_err(|e| {
            InstallationError::Other(format!("Failed to serialize installation state: {e}"))
        })?;

        fs::write(state_path, json).map_err(|e| {
            InstallationError::Other(format!("Failed to write installation state: {e}"))
        })?;

        Ok(())
    }

    /// Load state from file
    pub fn load(state_path: &Path) -> Result<Self> {
        let json = fs::read_to_string(state_path).map_err(|e| {
            InstallationError::Other(format!("Failed to read installation state: {e}"))
        })?;

        let state = serde_json::from_str(&json).map_err(|e| {
            InstallationError::Other(format!("Failed to deserialize installation state: {e}"))
        })?;

        Ok(state)
    }

    /// Check if state exists
    #[must_use]
    pub fn exists(state_path: &Path) -> bool {
        state_path.exists()
    }

    /// Validate state against plan
    #[must_use]
    pub fn validate(&self, plan_hash: &str) -> bool {
        self.plan_hash == plan_hash
    }
}

/// Manager for installation resume functionality
pub struct ResumeManager {
    state_path: PathBuf,
    state: Option<InstallationState>,
}

impl ResumeManager {
    /// Create a new resume manager
    #[must_use]
    pub fn new(target_dir: &Path) -> Self {
        let state_path = target_dir.join(".cascette").join("installation-state.json");
        Self {
            state_path,
            state: None,
        }
    }

    /// Initialize or load state for an installation
    pub fn initialize(
        &mut self,
        plan_hash: String,
        total_files: usize,
        build_id: u32,
        product_code: String,
    ) -> Result<()> {
        // Try to load existing state
        if InstallationState::exists(&self.state_path) {
            let existing_state = InstallationState::load(&self.state_path)?;

            // Validate that it's for the same plan
            if existing_state.validate(&plan_hash) {
                println!(
                    "→ Resuming installation from previous state: {}/{} files completed ({:.1}%)",
                    existing_state.completed_count(),
                    existing_state.total_files,
                    existing_state.completion_percentage()
                );
                self.state = Some(existing_state);
                return Ok(());
            }
            println!("→ Previous installation state is for a different plan, starting fresh");
        }

        // Create new state
        let new_state = InstallationState::new(plan_hash, total_files, build_id, product_code);
        new_state.save(&self.state_path)?;
        self.state = Some(new_state);
        println!("→ Starting fresh installation with resume support enabled");
        Ok(())
    }

    /// Check if a file should be skipped (already completed)
    #[must_use]
    pub fn should_skip(&self, file_path: &str) -> bool {
        if let Some(state) = &self.state {
            state.is_completed(file_path)
        } else {
            false
        }
    }

    /// Mark a file as completed and persist state
    pub fn mark_completed(&mut self, file_path: String) -> Result<()> {
        if let Some(state) = &mut self.state {
            state.mark_completed(file_path);

            // Save state every 10 files to avoid too much I/O
            if state.completed_count() % 10 == 0 {
                state.save(&self.state_path)?;
            }
        }
        Ok(())
    }

    /// Force save current state
    pub fn save(&self) -> Result<()> {
        if let Some(state) = &self.state {
            state.save(&self.state_path)?;
        }
        Ok(())
    }

    /// Get current progress information
    #[must_use]
    pub fn get_progress(&self) -> Option<(usize, usize, f32)> {
        self.state.as_ref().map(|s| {
            (
                s.completed_count(),
                s.total_files,
                s.completion_percentage(),
            )
        })
    }

    /// Clear state after successful completion
    pub fn clear(&self) -> Result<()> {
        if self.state_path.exists() {
            fs::remove_file(&self.state_path).map_err(|e| {
                InstallationError::Other(format!("Failed to remove state file: {e}"))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_installation_state() {
        let mut state = InstallationState::new(
            "test_hash".to_string(),
            100,
            31650,
            "wow_classic".to_string(),
        );

        assert_eq!(state.completed_count(), 0);
        assert!((state.completion_percentage() - 0.0).abs() < f32::EPSILON);

        state.mark_completed("file1.dat".to_string());
        state.mark_completed("file2.dat".to_string());

        assert_eq!(state.completed_count(), 2);
        assert!((state.completion_percentage() - 2.0).abs() < f32::EPSILON);
        assert!(state.is_completed("file1.dat"));
        assert!(!state.is_completed("file3.dat"));
    }

    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let state_path = temp_dir.path().join("state.json");

        let mut state = InstallationState::new(
            "test_hash".to_string(),
            100,
            31650,
            "wow_classic".to_string(),
        );
        state.mark_completed("file1.dat".to_string());
        state.save(&state_path).expect("Failed to save state");

        let loaded_state = InstallationState::load(&state_path).expect("Failed to load state");
        assert_eq!(loaded_state.completed_count(), 1);
        assert!(loaded_state.is_completed("file1.dat"));
        assert!(loaded_state.validate("test_hash"));
    }

    #[test]
    fn test_resume_manager() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut manager = ResumeManager::new(temp_dir.path());

        manager
            .initialize(
                "plan_hash".to_string(),
                50,
                31650,
                "wow_classic".to_string(),
            )
            .expect("Failed to initialize manager");

        assert!(!manager.should_skip("file1.dat"));

        manager
            .mark_completed("file1.dat".to_string())
            .expect("Failed to mark completed");
        manager.save().expect("Failed to save manager state");

        // Create new manager to simulate restart
        let mut manager2 = ResumeManager::new(temp_dir.path());
        manager2
            .initialize(
                "plan_hash".to_string(),
                50,
                31650,
                "wow_classic".to_string(),
            )
            .expect("Failed to initialize manager2");

        assert!(manager2.should_skip("file1.dat"));
        assert!(!manager2.should_skip("file2.dat"));

        let progress = manager2.get_progress().expect("Failed to get progress");
        assert_eq!(progress.0, 1); // completed
        assert_eq!(progress.1, 50); // total
        assert!((progress.2 - 2.0).abs() < f32::EPSILON); // percentage
    }
}
