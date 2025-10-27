//! Archive optimization module for efficient block mapping and range coalescing
//!
//! This module implements intelligent analysis of file locations within archives
//! to minimize the number of download requests and optimize bandwidth usage.

use cascette_formats::archive::IndexEntry;
use std::collections::HashMap;

/// Represents a range of data within an archive
#[derive(Debug, Clone)]
pub struct FileRange {
    /// Starting offset in the archive
    pub offset: u64,
    /// Size of the data
    pub size: u64,
    /// Path of the file this range belongs to
    #[allow(dead_code)]
    pub file_path: String,
}

/// HTTP range request format
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRange {
    /// Starting offset
    pub start: u64,
    /// Ending offset (inclusive)
    pub end: u64,
}

impl HttpRange {
    /// Create a new HTTP range
    #[must_use]
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    /// Get the size of this range
    #[must_use]
    pub fn size(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Check if this range can be merged with another
    #[must_use]
    pub fn can_merge(&self, other: &Self, max_gap: u64) -> bool {
        // Check if ranges overlap or are close enough
        if self.end + 1 + max_gap >= other.start && other.end >= self.start {
            return true;
        }
        if other.end + 1 + max_gap >= self.start && self.end >= other.start {
            return true;
        }
        false
    }

    /// Merge this range with another
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Maps archive content for optimized access patterns
pub struct ArchiveBlockMap {
    /// Map of archive hash to file ranges within that archive
    archive_files: HashMap<String, Vec<FileRange>>,
    /// Maximum gap between ranges to consider merging (in bytes)
    max_merge_gap: u64,
}

impl ArchiveBlockMap {
    /// Create a new archive block map with default settings
    #[must_use]
    pub fn new() -> Self {
        Self {
            archive_files: HashMap::new(),
            max_merge_gap: 1024 * 1024, // 1MB default gap
        }
    }

    /// Create a new archive block map with custom merge gap
    #[must_use]
    pub fn with_max_merge_gap(max_merge_gap: u64) -> Self {
        Self {
            archive_files: HashMap::new(),
            max_merge_gap,
        }
    }

    /// Add a file range to the map
    pub fn add_file_range(
        &mut self,
        archive_hash: String,
        offset: u64,
        size: u32,
        file_path: String,
    ) {
        let range = FileRange {
            offset,
            size: u64::from(size),
            file_path,
        };

        self.archive_files
            .entry(archive_hash)
            .or_default()
            .push(range);
    }

    /// Analyze install files and build the block map
    pub fn analyze_install_files(
        &mut self,
        files: &[(String, String, IndexEntry)], // (file_path, archive_hash, entry)
    ) {
        for (file_path, archive_hash, entry) in files {
            self.add_file_range(
                archive_hash.clone(),
                entry.offset,
                entry.size,
                file_path.clone(),
            );
        }

        // Sort ranges by offset for efficient processing
        for ranges in self.archive_files.values_mut() {
            ranges.sort_by_key(|r| r.offset);
        }
    }

    /// Get optimized ranges for downloading from an archive
    #[must_use]
    pub fn get_required_ranges(&self, archive: &str) -> Vec<HttpRange> {
        let Some(file_ranges) = self.archive_files.get(archive) else {
            return Vec::new();
        };

        if file_ranges.is_empty() {
            return Vec::new();
        }

        // Convert file ranges to HTTP ranges
        let mut ranges: Vec<HttpRange> = file_ranges
            .iter()
            .map(|fr| HttpRange::new(fr.offset, fr.offset + fr.size - 1))
            .collect();

        // Sort by start offset
        ranges.sort_by_key(|r| r.start);

        // Merge nearby ranges to reduce request count
        let mut merged = Vec::new();
        let mut current = ranges[0].clone();

        for range in ranges.iter().skip(1) {
            if current.can_merge(range, self.max_merge_gap) {
                current = current.merge(range);
            } else {
                merged.push(current);
                current = range.clone();
            }
        }
        merged.push(current);

        merged
    }

    /// Get statistics about the block map
    pub fn get_statistics(&self) -> BlockMapStatistics {
        let total_archives = self.archive_files.len();
        let total_files: usize = self.archive_files.values().map(std::vec::Vec::len).sum();

        let mut total_size = 0u64;
        let mut optimized_size = 0u64;

        for (archive, ranges) in &self.archive_files {
            // Original size is sum of all individual ranges
            let archive_original: u64 = ranges.iter().map(|r| r.size).sum();
            total_size += archive_original;

            // Optimized size is the size of merged ranges
            let merged_ranges = self.get_required_ranges(archive);
            let archive_optimized: u64 = merged_ranges.iter().map(HttpRange::size).sum();
            optimized_size += archive_optimized;
        }

        let savings_percent = if total_size > 0 && optimized_size < total_size {
            ((total_size - optimized_size) as f64 / total_size as f64) * 100.0
        } else if total_size > 0 {
            // Optimized size is larger (due to gaps), so we have negative savings
            -((optimized_size - total_size) as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };

        BlockMapStatistics {
            total_archives,
            total_files,
            total_size,
            optimized_size,
            savings_percent,
        }
    }

    /// Get file ranges for a specific archive
    #[allow(dead_code)]
    #[must_use]
    pub fn get_archive_files(&self, archive: &str) -> Option<&Vec<FileRange>> {
        self.archive_files.get(archive)
    }

    /// Check if an archive has any files
    #[allow(dead_code)]
    #[must_use]
    pub fn has_archive(&self, archive: &str) -> bool {
        self.archive_files.contains_key(archive)
    }

    /// Get all archives in the map
    #[allow(dead_code)]
    pub fn archives(&self) -> impl Iterator<Item = &String> {
        self.archive_files.keys()
    }
}

impl Default for ArchiveBlockMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the block mapping optimization
#[derive(Debug)]
#[allow(dead_code)]
pub struct BlockMapStatistics {
    /// Number of archives with files
    pub total_archives: usize,
    /// Total number of files across all archives
    pub total_files: usize,
    /// Total size of all individual file ranges
    pub total_size: u64,
    /// Size after merging nearby ranges
    pub optimized_size: u64,
    /// Percentage of bandwidth saved
    pub savings_percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_range_merge() {
        let range1 = HttpRange::new(0, 99);
        let range2 = HttpRange::new(100, 199);

        assert!(range1.can_merge(&range2, 0));
        let merged = range1.merge(&range2);
        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 199);
        assert_eq!(merged.size(), 200);
    }

    #[test]
    fn test_http_range_gap_merge() {
        let range1 = HttpRange::new(0, 99);
        let range2 = HttpRange::new(200, 299);

        // Too far apart with no gap allowance
        assert!(!range1.can_merge(&range2, 0));

        // Close enough with 100 byte gap allowance
        assert!(range1.can_merge(&range2, 100));
    }

    #[test]
    fn test_archive_block_map_basic() {
        let mut map = ArchiveBlockMap::new();

        map.add_file_range("archive1".to_string(), 0, 100, "file1.txt".to_string());
        map.add_file_range("archive1".to_string(), 200, 100, "file2.txt".to_string());
        map.add_file_range("archive2".to_string(), 0, 50, "file3.txt".to_string());

        assert!(map.has_archive("archive1"));
        assert!(map.has_archive("archive2"));
        assert!(!map.has_archive("archive3"));

        let files = map
            .get_archive_files("archive1")
            .expect("archive1 should exist");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_optimize_ranges() {
        let mut map = ArchiveBlockMap::with_max_merge_gap(1000);

        // Add files that are close together
        map.add_file_range("archive1".to_string(), 0, 100, "file1.txt".to_string());
        map.add_file_range("archive1".to_string(), 500, 100, "file2.txt".to_string());
        map.add_file_range("archive1".to_string(), 1000, 100, "file3.txt".to_string());

        // Files should be merged into one range due to small gaps
        let ranges = map.get_required_ranges("archive1");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 1099);
    }

    #[test]
    fn test_no_merge_large_gaps() {
        let mut map = ArchiveBlockMap::with_max_merge_gap(100);

        // Add files that are far apart
        map.add_file_range("archive1".to_string(), 0, 100, "file1.txt".to_string());
        map.add_file_range("archive1".to_string(), 10000, 100, "file2.txt".to_string());
        map.add_file_range("archive1".to_string(), 20000, 100, "file3.txt".to_string());

        // Files should not be merged due to large gaps
        let ranges = map.get_required_ranges("archive1");
        assert_eq!(ranges.len(), 3);
    }

    #[test]
    fn test_statistics() {
        let mut map = ArchiveBlockMap::with_max_merge_gap(1000);

        map.add_file_range("archive1".to_string(), 0, 1000, "file1.txt".to_string());
        map.add_file_range("archive1".to_string(), 1500, 1000, "file2.txt".to_string());
        map.add_file_range("archive2".to_string(), 0, 500, "file3.txt".to_string());

        let stats = map.get_statistics();
        assert_eq!(stats.total_archives, 2);
        assert_eq!(stats.total_files, 3);
        assert_eq!(stats.total_size, 2500);
        // After merging with 1000 byte gap, archive1 becomes one range
        assert!(stats.optimized_size > stats.total_size);
    }
}
